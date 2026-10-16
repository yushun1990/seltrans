//! Configuration loaded from `$XDG_CONFIG_HOME/seltrans/config.toml`
//! (fallback `~/.config/seltrans/config.toml`). API keys are referenced by
//! environment-variable name and never stored in the file itself.

use std::fmt;
use std::path::{Path, PathBuf};

pub const OPENAI_COMPATIBLE_PROVIDER: &str = "openai-compatible";
pub const DEFAULT_API_KEY_ENV: &str = "SELTRANS_API_KEY";
pub const DEFAULT_TIMEOUT_SECS: u64 = 60;

/// The translation provider configuration. Only the OpenAI-compatible
/// HTTP dialect exists so far; more providers must be added behind the
/// same `provider` key instead of ad-hoc extra files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub provider: ProviderConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderConfig {
    OpenAiCompatible(OpenAiCompatibleConfig),
}

impl ProviderConfig {
    pub fn name(&self) -> &'static str {
        match self {
            Self::OpenAiCompatible(_) => OPENAI_COMPATIBLE_PROVIDER,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCompatibleConfig {
    /// API root without a trailing slash, for example `https://host/v1`.
    pub base_url: String,
    pub model: String,
    /// Environment variable the resident reads the API key from at
    /// request time.
    pub api_key_env: String,
    pub timeout_secs: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    NotFound(PathBuf),
    ReadFailed(PathBuf, String),
    Invalid { path: PathBuf, reason: String },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound(path) => write!(
                f,
                "no configuration file at {}; create it (see docs/config.example.toml) to enable translation",
                path.display()
            ),
            Self::ReadFailed(path, error) => {
                write!(f, "cannot read {}: {error}", path.display())
            }
            Self::Invalid { path, reason } => {
                write!(f, "invalid configuration in {}: {reason}", path.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Resolves and loads the user's configuration. A missing file is reported
/// as an error rather than silently defaulting to a remote endpoint.
pub fn load() -> Result<Config, ConfigError> {
    load_from(config_path())
}

pub fn load_from(path: PathBuf) -> Result<Config, ConfigError> {
    if !path.exists() {
        return Err(ConfigError::NotFound(path));
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| ConfigError::ReadFailed(path.clone(), error.to_string()))?;
    parse(&raw).map_err(|reason| ConfigError::Invalid { path, reason })
}

fn parse(raw: &str) -> Result<Config, String> {
    #[derive(serde::Deserialize)]
    struct RawConfig {
        provider: Option<String>,
        #[serde(default)]
        providers: RawProviders,
    }

    #[derive(serde::Deserialize, Default)]
    struct RawProviders {
        #[serde(rename = "openai-compatible")]
        openai_compatible: Option<RawOpenAiCompatible>,
    }

    #[derive(serde::Deserialize)]
    struct RawOpenAiCompatible {
        base_url: String,
        model: String,
        api_key_env: Option<String>,
        timeout_secs: Option<u64>,
    }

    let raw: RawConfig = toml::from_str(raw).map_err(|error| format!("TOML syntax: {error}"))?;
    let provider_name = raw
        .provider
        .as_deref()
        .unwrap_or(OPENAI_COMPATIBLE_PROVIDER);
    if provider_name != OPENAI_COMPATIBLE_PROVIDER {
        return Err(format!(
            "unknown provider '{provider_name}'; only '{OPENAI_COMPATIBLE_PROVIDER}' is supported"
        ));
    }

    let section = raw
        .providers
        .openai_compatible
        .ok_or_else(|| format!("missing [providers.{OPENAI_COMPATIBLE_PROVIDER}] section"))?;
    let base_url = section.base_url.trim().trim_end_matches('/').to_owned();
    if base_url.is_empty() {
        return Err("base_url must not be empty".to_owned());
    }
    if !base_url.starts_with("http://") && !base_url.starts_with("https://") {
        return Err("base_url must start with http:// or https://".to_owned());
    }
    if section.model.trim().is_empty() {
        return Err("model must not be empty".to_owned());
    }
    let api_key_env = section
        .api_key_env
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_API_KEY_ENV.to_owned());
    let timeout_secs = section.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    if timeout_secs == 0 {
        return Err("timeout_secs must be greater than zero".to_owned());
    }

    Ok(Config {
        provider: ProviderConfig::OpenAiCompatible(OpenAiCompatibleConfig {
            base_url,
            model: section.model.trim().to_owned(),
            api_key_env,
            timeout_secs,
        }),
    })
}

/// `$XDG_CONFIG_HOME/seltrans/config.toml`, falling back to
/// `~/.config/seltrans/config.toml`.
pub fn config_path() -> PathBuf {
    if let Some(dir) = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
    {
        dir.join("seltrans").join("config.toml")
    } else {
        home_config_directory().join("seltrans").join("config.toml")
    }
}

fn home_config_directory() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        PathBuf::from(home).join(".config")
    } else {
        Path::new("/tmp").to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn openai_section(extra: &str) -> String {
        format!(
            "[providers.openai-compatible]\nbase_url = \"https://api.example.com/v1\"\nmodel = \"test-model\"\n{extra}"
        )
    }

    #[test]
    fn parses_minimal_config_with_defaults() {
        let config = parse(&openai_section("")).unwrap();
        let ProviderConfig::OpenAiCompatible(openai) = config.provider;
        assert_eq!(openai.base_url, "https://api.example.com/v1");
        assert_eq!(openai.model, "test-model");
        assert_eq!(openai.api_key_env, "SELTRANS_API_KEY");
        assert_eq!(openai.timeout_secs, DEFAULT_TIMEOUT_SECS);
    }

    #[test]
    fn normalizes_trailing_slash_and_reads_overrides() {
        let raw = openai_section("api_key_env = \"MY_KEY\"\ntimeout_secs = 5\n")
            .replace("https://api.example.com/v1", "https://api.example.com/v1/");
        let config = parse(&raw).unwrap();
        let ProviderConfig::OpenAiCompatible(openai) = config.provider;
        assert_eq!(openai.base_url, "https://api.example.com/v1");
        assert_eq!(openai.api_key_env, "MY_KEY");
        assert_eq!(openai.timeout_secs, 5);
    }

    #[test]
    fn rejects_unknown_provider_and_broken_sections() {
        assert!(
            parse("provider = \"magical\"\n")
                .unwrap_err()
                .contains("unknown provider")
        );
        assert!(parse("").unwrap_err().contains("missing"));
        assert!(parse(&openai_section("")).is_ok());
        let no_url = "[providers.openai-compatible]\nmodel = \"m\"\n";
        assert!(parse(no_url).unwrap_err().contains("base_url"));
        let bad_url = openai_section("").replace("https://api.example.com/v1", "ftp://example.com");
        assert!(parse(&bad_url).unwrap_err().contains("http"));
    }

    #[test]
    fn load_from_reports_missing_file_and_invalid_toml() {
        let missing = load_from(PathBuf::from("/nonexistent/seltrans/config.toml"));
        assert!(matches!(missing, Err(ConfigError::NotFound(_))));

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "not toml {{{").unwrap();
        match load_from(path.clone()) {
            Err(ConfigError::Invalid { path: bad, .. }) => assert_eq!(bad, path),
            other => panic!("expected invalid config, got {other:?}"),
        }
    }
}
