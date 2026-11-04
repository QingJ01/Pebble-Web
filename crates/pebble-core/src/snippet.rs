/// Build a snippet from plain text: first 200 chars, whitespace normalized.
pub fn make_snippet(text: &str) -> String {
    let normalized: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.chars().count() > 200 {
        let truncated: String = normalized.chars().take(200).collect();
        format!("{truncated}...")
    } else {
        normalized
    }
}

/// Strip HTML tags and style/script blocks to extract readable plain text for snippets.
pub fn strip_html_for_snippet(html: &str) -> String {
    let mut result = String::with_capacity(html.len());

    // First pass: remove <style>...</style> and <script>...</script> blocks
    let mut cleaned = String::with_capacity(html.len());
    for tag in &["<style", "<script"] {
        let source = if cleaned.is_empty() { html } else { &cleaned };
        let source_lower = source.to_ascii_lowercase();
        let mut new_cleaned = String::with_capacity(source.len());
        let mut p = 0;
        let close_tag = format!("</{}", &tag[1..]);
        while let Some(start) = source_lower[p..].find(tag) {
            new_cleaned.push_str(&source[p..p + start]);
            let abs_start = p + start;
            if let Some(end) = source_lower[abs_start..].find(&close_tag) {
                let close_end = source_lower[abs_start + end..].find('>');
                p = abs_start + end + close_end.map(|e| e + 1).unwrap_or(close_tag.len());
            } else {
                p = source.len();
                break;
            }
        }
        new_cleaned.push_str(&source[p..]);
        cleaned = new_cleaned;
    }

    // Second pass: strip all remaining HTML tags
    let mut in_tag = false;
    for ch in cleaned.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                result.push(' ');
            }
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    // Decode common HTML entities
    result
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

/// Build a clean snippet from body_text and body_html, stripping any HTML/CSS.
pub fn build_snippet(body_text: &str, body_html: &str) -> String {
    let clean_text = strip_html_for_snippet(body_text);
    let clean = clean_text.trim();
    if clean.is_empty() && !body_html.is_empty() {
        make_snippet(&strip_html_for_snippet(body_html))
    } else {
        make_snippet(clean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_removes_style_blocks() {
        let html = r#"<html><head><style>body, p { margin: 0; padding: 0; } .mail-container { max-width: 600px; }</style></head><body><p>Hello World</p></body></html>"#;
        let result = strip_html_for_snippet(html);
        assert!(!result.contains("margin"));
        assert!(!result.contains("padding"));
        assert!(result.contains("Hello World"));
    }

    #[test]
    fn test_strip_html_removes_script_blocks() {
        let html = r#"<p>Before</p><script>alert('xss')</script><p>After</p>"#;
        let result = strip_html_for_snippet(html);
        assert!(!result.contains("alert"));
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
    }

    #[test]
    fn test_strip_html_decodes_entities() {
        let html = "<p>A &amp; B &lt; C &gt; D</p>";
        let result = strip_html_for_snippet(html);
        assert!(result.contains("A & B < C > D"));
    }

    #[test]
    fn test_make_snippet_truncates() {
        let long = "word ".repeat(50);
        let snippet = make_snippet(&long);
        assert!(snippet.ends_with("..."));
        let without_ellipsis = snippet.trim_end_matches("...");
        assert_eq!(without_ellipsis.chars().count(), 200);
    }

    #[test]
    fn test_build_snippet_prefers_body_text() {
        let snippet = build_snippet("Plain text content", "<p>HTML content</p>");
        assert!(snippet.contains("Plain text content"));
    }

    #[test]
    fn test_build_snippet_falls_back_to_html() {
        let snippet = build_snippet("", "<p>HTML only content</p>");
        assert!(snippet.contains("HTML only content"));
    }

    #[test]
    fn test_build_snippet_strips_css_from_body_text() {
        let body_text = "<style>body { margin: 0; }</style><p>Actual content</p>";
        let snippet = build_snippet(body_text, "");
        assert!(!snippet.contains("margin"));
        assert!(snippet.contains("Actual content"));
    }
}
