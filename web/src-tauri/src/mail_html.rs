use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ammonia::{Builder, UrlRelative};

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct SanitizedMailHtml {
    pub fragment: String,
    pub has_remote_images: bool,
    pub has_complex_layout: bool,
}

pub(crate) fn sanitize_mail_html(source: &str) -> SanitizedMailHtml {
    let has_remote_images = Arc::new(AtomicBool::new(false));
    let remote_image_flag = Arc::clone(&has_remote_images);
    let mut builder = Builder::default();

    // Email layout depends heavily on inline styles, table attributes and
    // responsive <style> blocks. They remain isolated by the reader iframe's
    // no-script sandbox and CSP; active document features are never allowed.
    builder
        .clean_content_tags(HashSet::from(["script"]))
        .add_tags(&["font", "section", "style"])
        .add_generic_attributes(&[
            "align",
            "background",
            "bgcolor",
            "border",
            "cellpadding",
            "cellspacing",
            "class",
            "dir",
            "height",
            "id",
            "style",
            "valign",
            "width",
        ])
        .add_tag_attributes("a", &["name"])
        .add_tag_attributes("font", &["color", "face", "size"])
        .url_schemes(HashSet::from(["data", "http", "https", "mailto"]))
        .url_relative(UrlRelative::Deny)
        .link_rel(Some("noopener noreferrer"))
        .strip_comments(true)
        .attribute_filter(move |element, attribute, value| {
            if (element, attribute) == ("img", "src")
                && value
                    .trim_start()
                    .get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
                && !is_safe_image_data_url(value)
            {
                return None;
            }
            if (element, attribute) == ("a", "href")
                && value
                    .trim_start()
                    .get(..5)
                    .is_some_and(|prefix| prefix.eq_ignore_ascii_case("data:"))
            {
                return None;
            }
            if matches!((element, attribute), ("img", "src") | (_, "background"))
                && is_remote_url(value)
            {
                remote_image_flag.store(true, Ordering::Relaxed);
            }
            Some(value.into())
        });

    let fragment = builder.clean(source).to_string();
    let lower_fragment = fragment.to_ascii_lowercase();
    let has_remote_css_image = [
        "url(http://",
        "url(https://",
        "url('http://",
        "url('https://",
        "url(\"http://",
        "url(\"https://",
    ]
    .iter()
    .any(|needle| lower_fragment.contains(needle));
    let has_complex_layout = [
        "<table",
        "<img",
        "<picture",
        "<style",
        "<font",
        "<section",
        " background=",
        " bgcolor=",
    ]
    .iter()
    .any(|needle| lower_fragment.contains(needle));

    SanitizedMailHtml {
        fragment,
        has_remote_images: has_remote_images.load(Ordering::Relaxed) || has_remote_css_image,
        has_complex_layout,
    }
}

fn is_remote_url(value: &str) -> bool {
    let value = value.trim_start();
    value
        .get(..7)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("http://"))
        || value
            .get(..8)
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("https://"))
}

fn is_safe_image_data_url(value: &str) -> bool {
    let value = value.trim_start();
    [
        "data:image/gif;",
        "data:image/jpeg;",
        "data:image/png;",
        "data:image/webp;",
    ]
    .iter()
    .any(|prefix| {
        value
            .get(..prefix.len())
            .is_some_and(|value_prefix| value_prefix.eq_ignore_ascii_case(prefix))
    })
}

#[cfg(test)]
mod tests {
    use super::sanitize_mail_html;

    #[test]
    fn keeps_email_layout_but_removes_active_content_and_dangerous_urls() {
        let result = sanitize_mail_html(
            r#"<style>.mobile { display:none } @media(max-width:600px){.mobile{display:block}}</style>
               <script>window.top.location='https://evil.example'</script>
               <table width="640" style="color:#123"><tr><td class="mobile">Hello</td></tr></table>
               <img src="https://images.example/logo.png" onerror="alert(1)">
               <a href="javascript:alert(2)" onclick="alert(3)">unsafe</a>"#,
        );

        assert!(result.fragment.contains("<style>"));
        assert!(result.fragment.contains("@media"));
        assert!(result.fragment.contains("<table"));
        assert!(result.fragment.contains("style=\"color:#123\""));
        assert!(!result.fragment.contains("<script"));
        assert!(!result.fragment.contains("onerror"));
        assert!(!result.fragment.contains("onclick"));
        assert!(!result.fragment.contains("javascript:"));
        assert!(result.has_remote_images);
        assert!(result.has_complex_layout);
    }

    #[test]
    fn allows_safe_inline_images_without_marking_them_remote() {
        let result = sanitize_mail_html(
            r#"<img alt="logo" src="data:image/png;base64,AQID">
               <img alt="unsafe" src="data:image/svg+xml,<svg></svg>">
               <a href="data:text/html,unsafe">Unsafe data link</a>
               <a href="https://example.com">Open</a>"#,
        );

        assert!(result.fragment.contains("data:image/png;base64,AQID"));
        assert!(!result.fragment.contains("data:image/svg+xml"));
        assert!(!result.fragment.contains("data:text/html"));
        assert!(result.fragment.contains("https://example.com"));
        assert!(!result.has_remote_images);
        assert!(result.has_complex_layout);
    }

    #[test]
    fn recognizes_simple_html_that_can_use_the_native_themed_reader() {
        let result =
            sanitize_mail_html(r#"<div>Hello <strong>there</strong></div><p>A short reply.</p>"#);

        assert!(!result.has_remote_images);
        assert!(!result.has_complex_layout);
    }
}
