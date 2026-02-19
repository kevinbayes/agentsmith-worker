use serde::Deserialize;

use super::anthropic::AnthropicProvider;
use super::gemini::GeminiProvider;
use super::openai_compat::OpenAICompatProvider;

/// Which LLM provider to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Anthropic,
    OpenAI,
    Cerebras,
    Groq,
    Grok,
    SambaNova,
    Gemini,
}

impl ProviderKind {
    /// Parse from a string, case-insensitive. Supports "supernova" alias for SambaNova.
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "anthropic" => Some(Self::Anthropic),
            "openai" => Some(Self::OpenAI),
            "cerebras" => Some(Self::Cerebras),
            "groq" => Some(Self::Groq),
            "grok" => Some(Self::Grok),
            "sambanova" | "supernova" => Some(Self::SambaNova),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }

    /// Default model for each provider.
    pub fn default_model(&self) -> &'static str {
        match self {
            Self::Anthropic => "claude-haiku-4-5-20251001",
            Self::OpenAI => "gpt-4o-mini",
            Self::Cerebras => "llama-4-scout-17b-16e-instruct",
            Self::Groq => "llama-3.3-70b-versatile",
            Self::Grok => "grok-3-mini-fast",
            Self::SambaNova => "Meta-Llama-3.1-8B-Instruct",
            Self::Gemini => "gemini-2.0-flash",
        }
    }

    /// Environment variable name for the API key.
    pub fn api_key_env_var(&self) -> &'static str {
        match self {
            Self::Anthropic => "ANTHROPIC_API_KEY",
            Self::OpenAI => "OPENAI_API_KEY",
            Self::Cerebras => "CEREBRAS_API_KEY",
            Self::Groq => "GROQ_API_KEY",
            Self::Grok => "GROK_API_KEY",
            Self::SambaNova => "SAMBANOVA_API_KEY",
            Self::Gemini => "GEMINI_API_KEY",
        }
    }

    /// Default base URL for each provider.
    pub fn default_base_url(&self) -> &'static str {
        match self {
            Self::Anthropic => "https://api.anthropic.com",
            Self::OpenAI => "https://api.openai.com",
            Self::Cerebras => "https://api.cerebras.ai",
            Self::Groq => "https://api.groq.com/openai",
            Self::Grok => "https://api.x.ai",
            Self::SambaNova => "https://api.sambanova.ai",
            Self::Gemini => "https://generativelanguage.googleapis.com",
        }
    }
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Anthropic => write!(f, "anthropic"),
            Self::OpenAI => write!(f, "openai"),
            Self::Cerebras => write!(f, "cerebras"),
            Self::Groq => write!(f, "groq"),
            Self::Grok => write!(f, "grok"),
            Self::SambaNova => write!(f, "sambanova"),
            Self::Gemini => write!(f, "gemini"),
        }
    }
}

/// Configuration for an LLM provider, typically nested in another config struct.
#[derive(Debug, Deserialize, Clone)]
pub struct LlmConfig {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
}

fn default_provider() -> String {
    "anthropic".to_string()
}

fn default_max_tokens() -> u32 {
    256
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            api_key: None,
            model: None,
            base_url: None,
            max_tokens: default_max_tokens(),
        }
    }
}

impl LlmConfig {
    /// Resolve the provider kind from the config string.
    pub fn provider_kind(&self) -> Option<ProviderKind> {
        ProviderKind::from_str(&self.provider)
    }

    /// Resolve the model, falling back to the provider's default.
    pub fn resolved_model(&self) -> String {
        if let Some(ref m) = self.model {
            if !m.is_empty() {
                return m.clone();
            }
        }
        self.provider_kind()
            .map(|p| p.default_model().to_string())
            .unwrap_or_else(|| "claude-haiku-4-5-20251001".to_string())
    }

    /// Resolve the API key: check config field first, then the provider-specific env var.
    pub fn resolved_api_key(&self) -> Option<String> {
        if let Some(ref key) = self.api_key {
            if !key.is_empty() {
                return Some(key.clone());
            }
        }
        if let Some(kind) = self.provider_kind() {
            std::env::var(kind.api_key_env_var()).ok()
        } else {
            None
        }
    }

    /// Resolve the base URL, falling back to provider default.
    pub fn resolved_base_url(&self) -> String {
        if let Some(ref url) = self.base_url {
            if !url.is_empty() {
                return url.clone();
            }
        }
        self.provider_kind()
            .map(|p| p.default_base_url().to_string())
            .unwrap_or_else(|| "https://api.anthropic.com".to_string())
    }
}

/// Enum-dispatched LLM provider. Wraps the three API families.
pub enum LlmProvider {
    Anthropic(AnthropicProvider),
    OpenAICompat(OpenAICompatProvider),
    Gemini(GeminiProvider),
}

impl LlmProvider {
    /// Construct the appropriate provider variant from config.
    pub fn from_config(config: &LlmConfig) -> anyhow::Result<Self> {
        let kind = config
            .provider_kind()
            .ok_or_else(|| anyhow::anyhow!("Unknown LLM provider: {}", config.provider))?;

        let api_key = config
            .resolved_api_key()
            .ok_or_else(|| anyhow::anyhow!("No API key for provider '{}'. Set {} or configure api_key.", kind, kind.api_key_env_var()))?;

        let model = config.resolved_model();
        let base_url = config.resolved_base_url();
        let max_tokens = config.max_tokens;

        match kind {
            ProviderKind::Anthropic => Ok(Self::Anthropic(AnthropicProvider::new(
                api_key, model, base_url, max_tokens,
            ))),
            ProviderKind::OpenAI
            | ProviderKind::Cerebras
            | ProviderKind::Groq
            | ProviderKind::Grok
            | ProviderKind::SambaNova => Ok(Self::OpenAICompat(OpenAICompatProvider::new(
                api_key, model, base_url, max_tokens,
            ))),
            ProviderKind::Gemini => Ok(Self::Gemini(GeminiProvider::new(
                api_key, model, base_url, max_tokens,
            ))),
        }
    }

    /// Generate a response given a system prompt and user message.
    pub async fn generate(&self, system: &str, user: &str) -> anyhow::Result<String> {
        match self {
            Self::Anthropic(p) => p.generate(system, user).await,
            Self::OpenAICompat(p) => p.generate(system, user).await,
            Self::Gemini(p) => p.generate(system, user).await,
        }
    }
}
