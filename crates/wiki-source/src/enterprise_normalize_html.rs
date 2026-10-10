//! Conservative sanitization for retained Wikimedia-rendered HTML.
//!
//! The canonical record must never carry active script/event-handler content
//! into a WebView. Visual resources are handled separately and are rejected
//! before this sanitizer until verified media joins exist.

use ammonia::Builder;

use super::enterprise_normalize::NormalizeError;

pub(crate) fn sanitize_rendered_html(input: &str) -> Result<String, NormalizeError> {
    let sanitized = Builder::default().clean(input).to_string();
    if sanitized.trim().is_empty() {
        return Err(NormalizeError::UnsafeHtml);
    }
    Ok(sanitized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_script_handlers_and_javascript_urls() {
        let output = sanitize_rendered_html(
            r#"<p onclick="x()">ok</p><script>bad()</script><a href="javascript:bad()">x</a>"#,
        )
        .unwrap();
        assert!(output.contains("ok"));
        assert!(!output.contains("<script"));
        assert!(!output.contains("bad()"));
        assert!(!output.contains("onclick"));
        assert!(!output.contains("javascript:"));
    }

    #[test]
    fn rejects_content_that_sanitizes_to_empty() {
        assert_eq!(
            sanitize_rendered_html("<script>bad()</script>"),
            Err(NormalizeError::UnsafeHtml)
        );
    }
}
