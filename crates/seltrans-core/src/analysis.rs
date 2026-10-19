//! The analysis boundary: what the application asks of a translation
//! provider and the domain-level results it may return. HTTP dialects,
//! JSON field names and status codes stay inside provider implementations.

/// A word worth learning from the passage. Vocabulary is deliberately
/// selective: only genuinely difficult, uncommon, technically important or
/// contextually surprising words belong here, never a mechanical word list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VocabularyNote {
    /// The form as it appears in the passage.
    pub surface: String,
    /// Canonical/lemma form when the provider can normalize it.
    pub lemma: Option<String>,
    /// What the word means in this passage, in the reader's language.
    pub contextual_meaning: String,
    /// Why the word was selected, when the provider explains it.
    pub reason: Option<String>,
}

/// The kinds of useful language phenomena a passage may contain beyond
/// single difficult words. Expressions are surfaced exhaustively rather
/// than aggressively filtered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExpressionKind {
    PhrasalVerb,
    Idiom,
    Slang,
    Collocation,
    FixedExpression,
    ContextualMeaning,
    SentencePattern,
    TechnicalUsage,
}

impl std::fmt::Display for ExpressionKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            Self::PhrasalVerb => "phrasal verb",
            Self::Idiom => "idiom",
            Self::Slang => "slang",
            Self::Collocation => "collocation",
            Self::FixedExpression => "fixed expression",
            Self::ContextualMeaning => "contextual meaning",
            Self::SentencePattern => "sentence pattern",
            Self::TechnicalUsage => "technical usage",
        };
        f.write_str(label)
    }
}

/// A useful expression, structure or context-specific meaning detected in
/// the passage. `surface` is usually the exact wording; for sentence
/// patterns it may be the generalized pattern itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpressionNote {
    pub surface: String,
    pub kind: ExpressionKind,
    /// The meaning in the reader's language.
    pub meaning: String,
    /// A short note on usage or why it cannot be read word by word.
    pub explanation: Option<String>,
}

/// A validated passage analysis in domain terms. It is deliberately not
/// the provider's raw JSON payload. The source text stays with the caller;
/// the provider never echoes it back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassageAnalysis {
    pub translation: String,
    pub vocabulary: Vec<VocabularyNote>,
    pub expressions: Vec<ExpressionNote>,
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

/// Analyzes an explicitly selected passage: translation plus selective
/// vocabulary plus exhaustive expressions. Implementations must be safe to
/// call from a non-GUI worker thread; they are expected to block for the
/// duration of one network round trip bounded by their configured timeout.
pub trait TranslationProvider: Send + Sync {
    fn analyze_passage(&self, passage: &str) -> Result<PassageAnalysis, ProviderError>;
}
