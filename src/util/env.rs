//! Environment variable interpolation helper functions.

use std::{borrow::Cow, env, sync::LazyLock};

use regex::{Captures, Regex, Replacer};

static ENV_VAR_REGEX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)\b").unwrap());

/// This is more wordy implementation, needed until [this] bug is resolved.
///
/// [this]: https://github.com/rust-lang/regex/issues/777
struct EnvReplacer;

impl Replacer for EnvReplacer {
    fn replace_append(&mut self, caps: &Captures<'_>, dst: &mut String) {
        // SAFETY: caps.get(0) always returns Some, according to documentation.
        if let Some(var_name) = caps.get(1) {
            match env::var(var_name.as_str()) {
                Ok(var) => dst.push_str(&var),
                Err(_) => dst.push_str(&caps[0]),
            }
        } else {
            dst.push_str(&caps[0]);
        }
    }
}

/// Parse all mentions of environment variables (such as `$NAME`) and replace them with
/// actual values from the process environment.
pub(crate) fn parse_env_vars<'h>(input: &'h str) -> Cow<'h, str> {
    ENV_VAR_REGEX.replace_all(input, EnvReplacer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_env_vars_tests() {
        unsafe {
            env::set_var("TEST01", "value_01");
            env::set_var("TEST02", "value_02");
            env::set_var("TEST03", "$TEST01");
            env::set_var("TEST04", "");
        }

        assert_eq!(parse_env_vars("no matches"), "no matches");
        assert_eq!(parse_env_vars("TEST01"), "TEST01");
        assert_eq!(
            parse_env_vars("no $TEST00 $TEST0123 value"),
            "no $TEST00 $TEST0123 value"
        );
        assert_eq!(parse_env_vars("matched $TEST01"), "matched value_01");
        assert_eq!(parse_env_vars("$TEST02 matched"), "value_02 matched");
        assert_eq!(parse_env_vars("$TEST02$TEST03"), "value_02$TEST01");
        assert_eq!(parse_env_vars("$TEST04"), "");
        assert_eq!(parse_env_vars("xx $TEST04 xx"), "xx  xx");
        assert_eq!(parse_env_vars("$TEST05"), "$TEST05");
    }
}
