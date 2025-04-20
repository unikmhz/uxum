use std::str::Split;

use regex::Regex;

/// Split URL path into parts.
#[must_use]
fn path_segments(path: &str) -> Split<'_, char> {
    path.trim_start_matches('/').split('/')
}

/// Extract path parameters from an URL path.
pub(crate) fn extract_path_params(path: &str) -> impl Iterator<Item = &str> {
    // SAFETY: regex compilation is done at compile-time, so that potential bugs will not affect
    // the resulting binary.
    let rx = Regex::new(r"^\{(.+)\}$").unwrap();
    path_segments(path).filter_map(move |segment| match rx.captures(segment) {
        Some(cap) => cap.get(1).map(|m| m.as_str()),
        None => None,
    })
}
