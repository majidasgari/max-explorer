//! Product logic for max-explorer. No Qt, no CEF: this crate compiles unchanged
//! for a future Android shell.

/// Turn what the user typed in the address bar into something loadable.
/// Anything we cannot recognise as an address comes back untouched, so the
/// caller can send it to a search provider instead.
pub fn normalize_url_input(input: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        return String::new();
    }
    // shortcut: dot/localhost heuristic instead of a real parser,
    // swap for the `url` crate when search providers and IDN arrive
    let has_scheme = input.contains("://")
        || ["about:", "mailto:", "file:", "data:", "view-source:"]
            .iter()
            .any(|s| input.starts_with(s));

    let looks_like_host = !input.contains(char::is_whitespace)
        && (input.contains('.') || input.starts_with("localhost"));

    // absolute paths (POSIX, home, or a Windows drive) are not hosts
    let looks_like_path = input.starts_with('/')
        || input.starts_with("~/")
        || (input.len() >= 2 && input.as_bytes()[1] == b':');

    if has_scheme || looks_like_path || !looks_like_host {
        input.to_string()
    } else {
        format!("https://{input}")
    }
}

#[cfg(test)]
mod tests {
    use super::normalize_url_input;

    #[test]
    fn plain_host_gets_https() {
        assert_eq!(normalize_url_input("example.com"), "https://example.com");
        assert_eq!(
            normalize_url_input("  kde.org/start  "),
            "https://kde.org/start"
        );
    }

    #[test]
    fn existing_scheme_is_kept() {
        assert_eq!(
            normalize_url_input("http://localhost:8080"),
            "http://localhost:8080"
        );
        assert_eq!(normalize_url_input("about:blank"), "about:blank");
    }

    #[test]
    fn text_is_left_alone_for_search() {
        assert_eq!(normalize_url_input("rust vs c++"), "rust vs c++");
        assert_eq!(normalize_url_input(""), "");
    }

    #[test]
    fn localhost_and_file_paths() {
        assert_eq!(normalize_url_input("localhost"), "https://localhost");
        assert_eq!(normalize_url_input("/tmp/page.html"), "/tmp/page.html");
    }
}
