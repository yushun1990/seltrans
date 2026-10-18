//! OpenAI-compatible chat-completions analysis provider. This is the only
//! place that knows the HTTP dialect, header names and payload field
//! names; everything it returns is a validated domain object.

use std::error::Error as _;
use std::io::Read;
use std::time::Duration;

use crate::analysis::{
    ExpressionKind, ExpressionNote, PassageAnalysis, ProviderError, TranslationProvider,
    VocabularyNote,
};
use crate::config::OpenAiCompatibleConfig;

pub const ANALYSIS_SCHEMA_VERSION: u32 = 1;
/// Generous caps guarding against garbage responses, not expected sizes.
const MAX_TRANSLATION_BYTES: usize = 256 * 1024;
const MAX_RESPONSE_BYTES: u64 = 1024 * 1024;
const MAX_ERROR_BODY_BYTES: u64 = 64 * 1024;
const MAX_FIELD_CHARS: usize = 2000;
/// Vocabulary is selective; these caps only stop runaway model output.
const MAX_VOCABULARY_NOTES: usize = 16;
/// Expressions are exhaustive, so their cap is deliberately higher.
const MAX_EXPRESSION_NOTES: usize = 64;

const SYSTEM_PROMPT: &str = "You are the reading-analysis engine of an English learning \
assistant for a Chinese-speaking reader. For the user's English passage, produce: \
1. A translation into natural, accurate Simplified Chinese. Preserve technical \
identifiers, code, commands and proper names unchanged, prefer natural Chinese over \
word-by-word rendering, and add no explanations of your own. \
2. Vocabulary: SELECTIVE. List only the few words that are genuinely difficult, \
uncommon, technically important, or used with a surprising meaning in this context. \
Never mechanically list common words. \
3. Expressions: thorough. Report every materially useful item that occurs in the \
passage: phrasal verbs, idioms, slang, collocations, fixed expressions, non-literal \
or context-specific meanings, sentence patterns whose meaning cannot be read word by \
word (give the generalized pattern, e.g. \"not so much A as B\"), and technical terms \
whose conventional meaning differs from everyday usage. Do not invent items. \
All meanings, reasons and explanations must be in Simplified Chinese. Respond with \
ONLY a JSON object exactly of the form {\"schema_version\":1,\"translation\":\"...\",\
\"vocabulary\":[{\"surface\":\"...\",\"lemma\":\"...\",\"contextual_meaning\":\"...\",\
\"reason\":\"...\"}],\"expressions\":[{\"surface\":\"...\",\"kind\":\"phrasal_verb\",\
\"meaning\":\"...\",\"explanation\":\"...\"}]} with no markdown, no code fences and no \
extra keys. \"kind\" is one of phrasal_verb, idiom, slang, collocation, \
fixed_expression, contextual_meaning, sentence_pattern, technical_usage. Omit \
\"vocabulary\" or \"expressions\" when there is nothing worth reporting; lemma, \
reason and explanation are optional.";

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
    fn analyze_passage(&self, passage: &str) -> Result<PassageAnalysis, ProviderError> {
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

fn parse_chat_completion(body: &str) -> Result<PassageAnalysis, ProviderError> {
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
    parse_analysis(content)
}

#[derive(serde::Deserialize)]
struct ProviderPayload {
    schema_version: u32,
    translation: String,
    vocabulary: Option<Vec<serde_json::Value>>,
    expressions: Option<Vec<serde_json::Value>>,
}

#[derive(serde::Deserialize)]
struct RawVocabularyItem {
    surface: String,
    lemma: Option<String>,
    contextual_meaning: String,
    reason: Option<String>,
}

#[derive(serde::Deserialize)]
struct RawExpressionItem {
    surface: String,
    kind: ExpressionKind,
    meaning: String,
    explanation: Option<String>,
}

/// Validates the model's content against the versioned analysis schema.
/// The translation itself is validated strictly; individual notes are
/// converted leniently — a malformed or unknown-kind item is dropped
/// instead of failing the whole analysis, so one bad row cannot cost the
/// user the translation.
fn parse_analysis(content: &str) -> Result<PassageAnalysis, ProviderError> {
    let candidate = extract_json_object(content);
    let payload: ProviderPayload = serde_json::from_str(candidate).map_err(|error| {
        ProviderError::MalformedResponse(format!(
            "expected a schema_version {ANALYSIS_SCHEMA_VERSION} analysis object: {error}"
        ))
    })?;
    if payload.schema_version != ANALYSIS_SCHEMA_VERSION {
        return Err(ProviderError::MalformedResponse(format!(
            "unsupported schema_version {} (expected {ANALYSIS_SCHEMA_VERSION})",
            payload.schema_version
        )));
    }
    let translation = payload.translation.trim().to_owned();
    if translation.is_empty() {
        return Err(ProviderError::MalformedResponse(
            "translation is empty".to_owned(),
        ));
    }
    if translation.len() > MAX_TRANSLATION_BYTES {
        return Err(ProviderError::MalformedResponse(
            "translation exceeds 256 KiB".to_owned(),
        ));
    }
    let vocabulary = payload
        .vocabulary
        .unwrap_or_default()
        .into_iter()
        .filter_map(|raw| serde_json::from_value::<RawVocabularyItem>(raw).ok())
        .filter_map(|item| {
            Some(VocabularyNote {
                surface: bounded(&item.surface)?,
                lemma: bounded_opt(item.lemma),
                contextual_meaning: bounded(&item.contextual_meaning)?,
                reason: bounded_opt(item.reason),
            })
        })
        .take(MAX_VOCABULARY_NOTES)
        .collect();
    let expressions = payload
        .expressions
        .unwrap_or_default()
        .into_iter()
        .filter_map(|raw| serde_json::from_value::<RawExpressionItem>(raw).ok())
        .filter_map(|item| {
            Some(ExpressionNote {
                surface: bounded(&item.surface)?,
                kind: item.kind,
                meaning: bounded(&item.meaning)?,
                explanation: bounded_opt(item.explanation),
            })
        })
        .take(MAX_EXPRESSION_NOTES)
        .collect();
    Ok(PassageAnalysis {
        translation,
        vocabulary,
        expressions,
    })
}

/// Required field: present, trimmed and within the garbage cap.
fn bounded(field: &str) -> Option<String> {
    let value = field.trim();
    (!value.is_empty()).then(|| truncate(value, MAX_FIELD_CHARS))
}

/// Optional field: whitespace-only becomes `None`.
fn bounded_opt(field: Option<String>) -> Option<String> {
    field.as_deref().and_then(bounded)
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

    const FULL_PAYLOAD: &str = "{\"schema_version\":1,\
\"translation\":\"与其说是性能问题，不如说是正确性问题。\",\
\"vocabulary\":[{\"surface\":\"acknowledged\",\"lemma\":\"acknowledge\",\
\"contextual_meaning\":\"响应、确认（中断）\",\"reason\":\"技术语境下的特殊含义\"}],\
\"expressions\":[{\"surface\":\"not so much A as B\",\"kind\":\"sentence_pattern\",\
\"meaning\":\"与其说是 A，不如说是 B\",\"explanation\":\"不能逐字直读的句型\"}]}";

    #[test]
    fn parses_full_analysis_payload() {
        let analysis = parse_analysis(FULL_PAYLOAD).unwrap();
        assert_eq!(
            analysis.translation,
            "与其说是性能问题，不如说是正确性问题。"
        );
        assert_eq!(analysis.vocabulary.len(), 1);
        assert_eq!(analysis.vocabulary[0].surface, "acknowledged");
        assert_eq!(analysis.vocabulary[0].lemma.as_deref(), Some("acknowledge"));
        assert_eq!(
            analysis.vocabulary[0].contextual_meaning,
            "响应、确认（中断）"
        );
        assert_eq!(analysis.expressions.len(), 1);
        assert_eq!(
            analysis.expressions[0].kind,
            ExpressionKind::SentencePattern
        );
        assert_eq!(analysis.expressions[0].surface, "not so much A as B");
    }

    #[test]
    fn translation_only_payload_degrades_to_empty_notes() {
        let analysis = parse_analysis("{\"schema_version\":1,\"translation\":\"你好\"}").unwrap();
        assert_eq!(analysis.translation, "你好");
        assert!(analysis.vocabulary.is_empty());
        assert!(analysis.expressions.is_empty());
    }

    #[test]
    fn parses_fenced_and_prose_wrapped_json() {
        let fenced = format!("```json\n{FULL_PAYLOAD}\n```");
        assert_eq!(
            parse_analysis(&fenced).unwrap().translation,
            "与其说是性能问题，不如说是正确性问题。"
        );

        let chatty = format!("Sure! Here is the JSON:\n{FULL_PAYLOAD}\nHope that helps.");
        assert_eq!(
            parse_analysis(&chatty).unwrap().translation,
            "与其说是性能问题，不如说是正确性问题。"
        );
    }

    #[test]
    fn ignores_unknown_keys_for_forward_compatibility() {
        let mut richer = FULL_PAYLOAD[..FULL_PAYLOAD.len() - 1].to_owned();
        richer += ",\"future_field\":true}";
        assert_eq!(parse_analysis(&richer).unwrap().vocabulary.len(), 1);
    }

    #[test]
    fn rejects_wrong_version_missing_fields_and_oversized_text() {
        let wrong_version = "{\"schema_version\":2,\"translation\":\"x\"}";
        assert!(matches!(
            parse_analysis(wrong_version),
            Err(ProviderError::MalformedResponse(_))
        ));
        assert!(parse_analysis("{\"schema_version\":1}").is_err());
        assert!(parse_analysis("{\"schema_version\":1,\"translation\":\"   \"}").is_err());
        let huge = format!(
            "{{\"schema_version\":1,\"translation\":\"{}\"}}",
            "x".repeat(MAX_TRANSLATION_BYTES + 1)
        );
        assert!(parse_analysis(&huge).is_err());
    }

    #[test]
    fn drops_invalid_notes_and_truncates_runaway_lists() {
        // One healthy row, then rows that must each be dropped: missing
        // contextual_meaning, unknown kind, empty surface, non-object.
        let vocabulary = "{\"surface\":\"acknowledged\",\"contextual_meaning\":\"确认\"},\
             {\"surface\":\"orphan\"},\
             {\"surface\":\"  \",\"contextual_meaning\":\"empty surface\"},\
             \"not an object\"";
        let expressions = "{\"surface\":\"give in\",\"kind\":\"phrasal_verb\",\"meaning\":\"让步\"},\
             {\"surface\":\"dodgy\",\"kind\":\"metaphor\",\"meaning\":\"unknown kind\"},\
             {\"kind\":\"idiom\",\"meaning\":\"missing surface\"}";
        let payload = format!(
            "{{\"schema_version\":1,\"translation\":\"x\",\
\"vocabulary\":[{vocabulary}],\"expressions\":[{expressions}]}}"
        );
        let analysis = parse_analysis(&payload).unwrap();
        assert_eq!(analysis.vocabulary.len(), 1);
        assert_eq!(analysis.vocabulary[0].surface, "acknowledged");
        assert_eq!(analysis.expressions.len(), 1);
        assert_eq!(analysis.expressions[0].surface, "give in");

        let flood: Vec<String> = (0..MAX_EXPRESSION_NOTES + 10)
            .map(|i| format!("{{\"surface\":\"w{i}\",\"contextual_meaning\":\"m\"}}"))
            .collect();
        let flooded = format!(
            "{{\"schema_version\":1,\"translation\":\"x\",\"vocabulary\":[{}]}}",
            flood.join(",")
        );
        assert_eq!(
            parse_analysis(&flooded).unwrap().vocabulary.len(),
            MAX_VOCABULARY_NOTES
        );
    }

    #[test]
    fn parses_chat_completion_envelope() {
        let body = format!(
            "{{\"choices\":[{{\"message\":{{\"content\":{}}}}}]}}",
            serde_json::to_string(FULL_PAYLOAD).unwrap()
        );
        assert_eq!(
            parse_chat_completion(&body).unwrap().translation,
            "与其说是性能问题，不如说是正确性问题。"
        );

        assert!(parse_chat_completion("{\"choices\":[]}").is_err());
        assert!(parse_chat_completion("not json").is_err());
    }
}
