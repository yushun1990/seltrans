//! The analysis boundary: what the application asks of a translation
//! provider and the domain-level results it may return. HTTP dialects,
//! JSON field names and status codes stay inside provider implementations.

/// A validated translation result in domain terms. It is deliberately not
/// the provider's raw JSON payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translation {
    pub text: String,
}

/// Domain-level provider failures, each with an actionable message for the
/// result window. Provider implementations map their transport-specific
/// errors onto these variants instead of leaking HTTP details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderError {
    MissingApiKey {
        env_var: String,
    },
    Timeout {
        after_secs: u64,
    },
    Auth {
        status: u16,
    },
    RateLimited,
    Network(String),
    /// The provider answered but its payload failed validation.
    MalformedResponse(String),
    /// The provider reported an application-level error body.
    Api {
        status: u16,
        message: String,
    },
}

impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingApiKey { env_var } => write!(
                f,
                "API key is not set; export {env_var} before starting seltrans"
            ),
            Self::Timeout { after_secs } => {
                write!(f, "the provider did not answer within {after_secs}s")
            }
            Self::Auth { status } => write!(
                f,
                "the provider rejected the API key (HTTP {status}); check the key and model access"
            ),
            Self::RateLimited => write!(
                f,
                "the provider is rate-limiting requests; try again shortly"
            ),
            Self::Network(reason) => write!(f, "cannot reach the provider: {reason}"),
            Self::MalformedResponse(reason) => {
                write!(f, "the provider response is not valid: {reason}")
            }
            Self::Api { status, message } => {
                write!(f, "provider error (HTTP {status}): {message}")
            }
        }
    }
}

impl std::error::Error for ProviderError {}

/// Translates an explicitly selected passage. Implementations must be safe
/// to call from a non-GUI worker thread; they are expected to block for the
/// duration of one network round trip bounded by their configured timeout.
pub trait TranslationProvider: Send + Sync {
    fn translate_passage(&self, passage: &str) -> Result<Translation, ProviderError>;
}
