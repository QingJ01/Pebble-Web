use pebble_core::{PebbleError, Result};

use crate::types::{BilingualSegment, TranslateResult};

/// Build the JSON body for a DeepLX server.
///
/// Most DeepLX servers reject `source_lang = "AUTO"` (the frontend always
/// requests a source of "auto"). When the source is `auto` (any case) or empty
/// we omit the field so the server auto-detects; a real language code is
/// upper-cased and forwarded as-is.
fn build_deeplx_body(text: &str, from: &str, to: &str) -> serde_json::Value {
    let from_trimmed = from.trim();
    let target = to.trim().to_uppercase();
    let omit_source = from_trimmed.is_empty() || from_trimmed.eq_ignore_ascii_case("auto");
    if omit_source {
        serde_json::json!({ "text": text, "target_lang": target })
    } else {
        serde_json::json!({
            "text": text,
            "source_lang": from_trimmed.to_uppercase(),
            "target_lang": target,
        })
    }
}

pub async fn translate(
    client: &reqwest::Client,
    endpoint: &str,
    text: &str,
    from: &str,
    to: &str,
) -> Result<TranslateResult> {
    let body = build_deeplx_body(text, from, to);

    let resp = client
        .post(endpoint)
        .json(&body)
        .send()
        .await
        .map_err(|e| PebbleError::Translate(format!("DeepLX request failed: {e}")))?;

    let status = resp.status();
    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| PebbleError::Translate(format!("DeepLX response parse failed: {e}")))?;

    if !status.is_success() {
        return Err(PebbleError::Translate(format!(
            "DeepLX error {status}: {json}"
        )));
    }

    let translated = json
        .get("data")
        .and_then(|d| d.as_str())
        .unwrap_or("")
        .to_string();

    Ok(TranslateResult {
        segments: build_segments(text, &translated),
        translated,
    })
}

pub fn build_segments(source: &str, target: &str) -> Vec<BilingualSegment> {
    source
        .split('\n')
        .zip(target.split('\n'))
        .filter(|(s, _)| !s.trim().is_empty())
        .map(|(s, t)| BilingualSegment {
            source: s.to_string(),
            target: t.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_segments() {
        let segments = build_segments("Hello\nWorld\n\nFoo", "你好\n世界\n\nFoo翻译");
        assert_eq!(segments.len(), 3);
        assert_eq!(segments[0].source, "Hello");
        assert_eq!(segments[0].target, "你好");
        assert_eq!(segments[1].source, "World");
        assert_eq!(segments[1].target, "世界");
    }

    #[test]
    fn test_build_segments_uneven() {
        let segments = build_segments("Line1\nLine2\nLine3", "译1\n译2");
        // zip stops at shorter
        assert_eq!(segments.len(), 2);
    }

    #[test]
    fn deeplx_body_omits_source_for_auto() {
        let body = build_deeplx_body("hello", "auto", "zh");
        assert!(body.get("source_lang").is_none());
        assert_eq!(body["target_lang"], "ZH");
        assert_eq!(body["text"], "hello");
    }

    #[test]
    fn deeplx_body_includes_uppercased_source_for_real_code() {
        let body = build_deeplx_body("hello", "en", "zh");
        assert_eq!(body["source_lang"], "EN");
        assert_eq!(body["target_lang"], "ZH");
    }
}
