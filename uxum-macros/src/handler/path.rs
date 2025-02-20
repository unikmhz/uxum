use std::{borrow::Cow, str::Split};

/// Split URL path into parts.
#[must_use]
fn path_segments(path: &str) -> Split<'_, char> {
    path.trim_start_matches('/').split('/')
}

/// Extract path parameters from an URL path.
pub(crate) fn extract_path_params(path: &str) -> impl Iterator<Item = &str> {
    path_segments(path).filter_map(|segment| match segment.get(..1) {
        Some(":") if segment.len() > 1 => segment.strip_prefix(':'),
        _ => None,
    })
}

/// Reformat [`axum`](../axum) route path with path parameters for use in OpenAPI.
#[must_use]
pub(crate) fn format_path_for_spec(path: &str) -> String {
    path_segments(path)
        .map(|segment| match segment.get(..1) {
            Some(":") if segment.len() > 1 => Cow::Owned(format!("{{{}}}", &segment[1..])),
            Some(_) => Cow::Borrowed(segment),
            // TODO: maybe somehow add wildcard support?
            None => Cow::Borrowed(""),
        })
        .fold(String::new(), |mut acc, segment| {
            acc.push('/');
            acc.push_str(&segment);
            acc
        })
}
