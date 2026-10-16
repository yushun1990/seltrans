//! OpenAI-compatible chat-completions translation provider. This is the
//! only place that knows the HTTP dialect, header names and payload field
//! names; everything it returns is a validated domain object.

use std::error::Error as _;
use std::io::Read;
use std::time::Duration;

use crate::analysis::{ProviderError, Translation, TranslationProvider};
use crate::config::OpenAiCompatibleConfig;

pub const TRANSLATION_SCHEMA_VERSION: u32 = 1;
/// Generous caps guarding against garbage responses, not expected sizes.
const MAX_TRANSLATION_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_ERROR_BODY_BYTES: u64 = 64 * 1024;

const SYSTEM_PROMPT: &str = "You are the translation engine of an English reading \
assistant. Translate the user's English passage into natural, accurate Simplified \
Chinese. Preserve technical identifiers, code, commands and proper names \
unchanged, prefer natural Chinese over word-by-word rendering, and add no \
explanations of your own. Respond with ONLY a JSON object exactly of the form \
{\"schema_version\":1,\"translation\":\"...\"} with no markdown, no code fences \
and no extra keys.";

pub struct OpenAiCompatibleProvider {
    agent: ureq::Agent,
    config: OpenAiCompatibleConfig,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: OpenAiCompatibleConfig) -> Self {
        Self {
            agent: ureq::AgentBuilder::new().build(),
            config,
        }
    }

    fn api_key(&self) -> Result<String, ProviderError> {
        std::env::var(&self.config.api_key_env)
            .ok()
            .filter(|key| !key.trim().is_empty())
            .ok_or_else(|| ProviderError::MissingApiKey {
                env_var: self.config.api_key_env.clone(),
            })
    }
}

impl TranslationProvider for OpenAiCompatibleProvider {
    fn translate_passage(&self, passage: &str) -> Result<Translation, ProviderError> {
        let api_key = self.api_key()?;
        let url = format!("{}/chat/completions", self.config.base_url);
        let body = ureq::json!({
            "model": self.config.model,
            "messages": [
                {"role": "system", "content": SYSTEM_PROMPT},
                {"role": "user", "content": passage},
            ],
        });
        let request = self
            .agent
            .post(&url)
            .timeout(Duration::from_secs(self.config.timeout_secs))
            .set("Authorization", &format!("Bearer {api_key}"));
        match request.send_json(body) {
            Ok(response) => {
                let mut bytes = Vec::new();
                response
                    .into_reader()
                    .take(MAX_RESPONSE_BYTES + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|error| ProviderError::Network(error.to_string()))?;
                if bytes.len() as u64 > MAX_RESPONSE_BYTES {
                    return Err(ProviderError::MalformedResponse(format!(
                        "response exceeds {} bytes",
                        MAX_RESPONSE_BYTES
                    )));
                }
                parse_chat_completion(&String::from_utf8_lossy(&bytes))
            }
            Err(ureq::Error::Status(code, response)) => {
                let message = error_message(response.into_reader().take(MAX_ERROR_BODY_BYTES));
                match code {
                    401 | 403 => Err(ProviderError::Auth { status: code }),
                    429 => Err(ProviderError::RateLimited),
                    status => Err(ProviderError::Api { status, message }),
                }
            }
            Err(ureq::Error::Transport(transport)) => {
                Err(classify_transport(transport, self.config.timeout_secs))
            }
        }
    }
}

/// ureq surfaces request timeouts as `ErrorKind::Io` transports wrapping an
/// `io::Error(TimedOut)` (sometimes with an empty message), so the timeout
/// has to be detected in the source chain rather than by message text.
fn classify_transport(transport: ureq::Transport, timeout_secs: u64) -> ProviderError {
    let mut timed_out = transport
        .message()
        .is_some_and(|message| message.to_ascii_lowercase().contains("timed out"));
    let mut source = transport.source();
    while let Some(error) = source {
        if let Some(io_error) = error.downcast_ref::<std::io::Error>()
            && io_error.kind() == std::io::ErrorKind::TimedOut
        {
            timed_out = true;
            break;
        }
        source = error.source();
    }
    if timed_out {
        ProviderError::Timeout {
            after_secs: timeout_secs,
        }
    } else {
        let reason = format!(
            "{}: {}",
            transport.kind(),
            transport.message().unwrap_or("")
        );
        ProviderError::Network(truncate(&reason, 200))
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(max_chars).collect();
        format!("{cut}…")
    }
}

/// Pulls a human-readable message out of a non-2xx body, preferring the
/// conventional OpenAI-style `error.message` field.
fn error_message<R: Read>(reader: R) -> String {
    let mut bytes = Vec::new();
    let mut reader = reader;
    if reader.read_to_end(&mut bytes).is_err() || bytes.is_empty() {
        return "no error body".to_owned();
    }
    let body = String::from_utf8_lossy(&bytes);
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&body)
        && let Some(message) = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
    {
        return truncate(message, 200);
    }
    truncate(body.trim(), 200)
}

fn parse_chat_completion(body: &str) -> Result<Translation, ProviderError> {
    let value: serde_json::Value = serde_json::from_str(body)
        .map_err(|error| ProviderError::MalformedResponse(format!("body is not JSON: {error}")))?;
    let content = value
        .get("choices")
        .and_then(|choices| choices.get(0))
        .and_then(|choice| choice.get("message"))
        .and_then(|message| message.get("content"))
        .and_then(|content| content.as_str())
        .ok_or_else(|| {
            ProviderError::MalformedResponse(
                "no choices[0].message.content string in response".to_owned(),
            )
        })?;
    parse_translation(content)
}

#[derive(serde::Deserialize)]
struct ProviderPayload {
    schema_version: u32,
    translation: String,
}

/// Validates the model's content against the versioned translation schema.
/// Unknown extra keys are ignored so an M3-style richer payload still parses.
fn parse_translation(content: &str) -> Result<Translation, ProviderError> {
    let candidate = extract_json_object(content);
    let payload: ProviderPayload = serde_json::from_str(candidate).map_err(|error| {
        ProviderError::MalformedResponse(format!(
            "expected a schema_version {TRANSLATION_SCHEMA_VERSION} translation object: {error}"
        ))
    })?;
    if payload.schema_version != TRANSLATION_SCHEMA_VERSION {
        return Err(ProviderError::MalformedResponse(format!(
            "unsupported schema_version {} (expected {TRANSLATION_SCHEMA_VERSION})",
            payload.schema_version
        )));
    }
    let text = payload.translation.trim().to_owned();
    if text.is_empty() {
        return Err(ProviderError::MalformedResponse(
            "translation is empty".to_owned(),
        ));
    }
    if text.len() > MAX_TRANSLATION_BYTES {
        return Err(ProviderError::MalformedResponse(
            "translation exceeds 256 KiB".to_owned(),
        ));
    }
    Ok(Translation { text })
}

/// Models occasionally wrap the requested JSON in prose or code fences even
/// when told not to; recover the object before strict validation.
fn extract_json_object(content: &str) -> &str {
    let trimmed = content.trim();
    if let Some(fenced) = strip_code_fence(trimmed) {
        return fenced;
    }
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return trimmed;
    }
    if let (Some(start), Some(end)) = (trimmed.find('{'), trimmed.rfind('}'))
        && start < end
    {
        return &trimmed[start..=end];
    }
    trimmed
}

fn strip_code_fence(text: &str) -> Option<&str> {
    let rest = text.strip_prefix("```")?;
    let rest = rest.strip_prefix("json").unwrap_or(rest);
    let body = rest.trim_start().strip_suffix("```")?;
    Some(body.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content(inner: &str) -> String {
        format!("{{\"schema_version\":1,\"translation\":\"{inner}\"}}")
    }

    #[test]
    fn parses_plain_json_translation() {
        let translation =
            parse_translation(&content("与其说是性能问题，不如说是正确性问题。")).unwrap();
        assert_eq!(translation.text, "与其说是性能问题，不如说是正确性问题。");
    }

    #[test]
    fn parses_fenced_and_prose_wrapped_json() {
        let fenced = format!("```json\n{}\n```", content("你好"));
        assert_eq!(parse_translation(&fenced).unwrap().text, "你好");

        let chatty = format!(
            "Sure! Here is the JSON:\n{}\nHope that helps.",
            content("好的")
        );
        assert_eq!(parse_translation(&chatty).unwrap().text, "好的");
    }

    #[test]
    fn ignores_unknown_keys_for_forward_compatibility() {
        let richer = r#"{"schema_version":1,"translation":"x","vocabulary":[]}"#.to_string();
        assert_eq!(parse_translation(&richer).unwrap().text, "x");
    }

    #[test]
    fn rejects_wrong_version_missing_fields_and_oversized_text() {
        let wrong_version = "{\"schema_version\":2,\"translation\":\"x\"}";
        assert!(matches!(
            parse_translation(wrong_version),
            Err(ProviderError::MalformedResponse(_))
        ));
        assert!(parse_translation("{\"schema_version\":1}").is_err());
        assert!(parse_translation(&content("   ")).is_err());
        let huge = "x".repeat(MAX_TRANSLATION_BYTES + 1);
        assert!(parse_translation(&content(&huge)).is_err());
    }

    #[test]
    fn parses_chat_completion_envelope() {
        let body = format!(
            "{{\"choices\":[{{\"message\":{{\"content\":{}}}}}]}}",
            serde_json::to_string(&content("翻译")).unwrap()
        );
        assert_eq!(parse_chat_completion(&body).unwrap().text, "翻译");

        assert!(parse_chat_completion("{\"choices\":[]}").is_err());
        assert!(parse_chat_completion("not json").is_err());
    }
}
