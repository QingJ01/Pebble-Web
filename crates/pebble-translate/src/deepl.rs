use pebble_core::{PebbleError, Result};

use crate::deeplx::build_segments;
use crate::types::TranslateResult;

/// Build the form fields for DeepL's `/v2/translate` endpoint.
///
/// DeepL rejects an explicit `source_lang` of `auto` with HTTP 400
/// ("Value for 'source_lang' not supported"). The frontend always requests a
/// source of "auto", so when the source is `auto` (any case) or empty we omit
/// the field entirely and let DeepL auto-detect. A real language code is
/// upper-cased and forwarded as-is.
fn build_deepl_form(text: &str, from: &str, to: &str) -> Vec<(String, String)> {
    let from_trimmed = from.trim();
    let omit_source = from_trimmed.is_empty() || from_trimmed.eq_ignore_ascii_case("auto");
    let mut form = vec![
        ("text".to_string(), text.to_string()),
        ("target_lang".to_string(), to.trim().to_uppercase()),
    ];
    if !omit_source {
        form.push(("source_lang".to_string(), from_trimmed.to_uppercase()));
    }
    form
}

pub async fn translate(
    client: &reqwest::Client,
    api_key: &str,
    use_free_api: bool,
    text: &str,
    from: &str,
    to: &str,
) -> Result<TranslateResult> {
    let base = if use_free_api {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    };

    let form = build_deepl_form(text, from, to);

    let resp = client
        .post(base)
        .header("Authorization", format!("DeepL-Auth-Key {api_key}"))
        .form(&form)
        .send()
        .await
        .map_err(|e| PebbleError::Translate(format!("DeepL request failed: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(PebbleError::Translate(format!(
            "DeepL error {status}: {body}"
        )));
    }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| PebbleError::Translate(format!("DeepL parse failed: {e}")))?;

    let translated = json
        .get("translations")
        .and_then(|t| t.get(0))
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_string();

    Ok(TranslateResult {
        segments: build_segments(text, &translated),
        translated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn form_get<'a>(form: &'a [(String, String)], key: &str) -> Option<&'a str> {
        form.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    #[test]
    fn omits_source_lang_for_auto() {
        // Regression for #65: frontend sends source = "auto", which DeepL
        // rejects. The field must be dropped so DeepL auto-detects.
        let form = build_deepl_form("hello", "auto", "zh");
        assert!(form.iter().all(|(k, _)| k != "source_lang"));
        assert_eq!(form_get(&form, "text"), Some("hello"));
        assert_eq!(form_get(&form, "target_lang"), Some("ZH"));
    }

    #[test]
    fn omits_source_lang_when_empty_or_whitespace() {
        for src in ["", "   ", "auto", "AUTO", " Auto "] {
            let form = build_deepl_form("hi", src, "ZH");
            assert!(
                form.iter().all(|(k, _)| k != "source_lang"),
                "source_lang should be omitted for {src:?}"
            );
        }
    }

    #[test]
    fn forwards_uppercased_real_source_code() {
        let form = build_deepl_form("hello", "en", "zh");
        assert_eq!(form_get(&form, "source_lang"), Some("EN"));
        assert_eq!(form_get(&form, "target_lang"), Some("ZH"));
    }

    #[test]
    fn trims_and_uppercases_target() {
        let form = build_deepl_form("hello", "auto", "  zh-cn  ");
        assert_eq!(form_get(&form, "target_lang"), Some("ZH-CN"));
    }
}
