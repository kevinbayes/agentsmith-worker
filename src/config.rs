use serde::Deserialize;
use std::path::{Path, PathBuf};

use crate::llm::LlmConfig;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    #[serde(default)]
    pub daemon: DaemonConfig,
    #[serde(default)]
    pub signal: SignalConfig,
    #[serde(default)]
    pub slack: SlackConfig,
    #[serde(default)]
    pub session_defaults: SessionDefaultsConfig,
    #[serde(default)]
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub gemini: GeminiConfig,
    #[serde(default)]
    pub goose: GooseConfig,
    #[serde(default)]
    pub interaction_agent: InteractionAgentConfig,
}

#[derive(Debug, Deserialize, Clone)]
pub struct DaemonConfig {
    #[serde(default = "default_working_dir")]
    pub working_dir: PathBuf,
    #[serde(default = "default_log_level")]
    pub log_level: String,
    #[serde(default = "default_data_dir")]
    pub data_dir: PathBuf,
}

impl Default for DaemonConfig {
    fn default() -> Self {
        Self {
            working_dir: default_working_dir(),
            log_level: default_log_level(),
            data_dir: default_data_dir(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SignalConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_device_name")]
    pub device_name: String,
    #[serde(default)]
    pub db_path: Option<PathBuf>,
    #[serde(default)]
    pub authorized_user: Option<String>,
}

impl Default for SignalConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            device_name: default_device_name(),
            db_path: None,
            authorized_user: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SlackConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub app_token: Option<String>,
    #[serde(default)]
    pub bot_token: Option<String>,
    #[serde(default = "default_true")]
    pub dm_only: bool,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            app_token: None,
            bot_token: None,
            dm_only: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SessionDefaultsConfig {
    #[serde(default = "default_tool")]
    pub default_tool: String,
    #[serde(default = "default_max_sessions")]
    pub max_sessions: usize,
    #[serde(default = "default_flush_interval")]
    pub output_flush_interval_ms: u64,
    #[serde(default = "default_max_chunk")]
    pub max_chunk_size: usize,
}

impl Default for SessionDefaultsConfig {
    fn default() -> Self {
        Self {
            default_tool: default_tool(),
            max_sessions: default_max_sessions(),
            output_flush_interval_ms: default_flush_interval(),
            max_chunk_size: default_max_chunk(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct ClaudeConfig {
    #[serde(default = "default_claude_binary")]
    pub binary: String,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub prompt_mode: bool,
    #[serde(default)]
    pub skip_permissions: bool,
}

impl Default for ClaudeConfig {
    fn default() -> Self {
        Self {
            binary: default_claude_binary(),
            extra_args: Vec::new(),
            prompt_mode: false,
            skip_permissions: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct GeminiConfig {
    #[serde(default = "default_gemini_binary")]
    pub binary: String,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub prompt_mode: bool,
    #[serde(default)]
    pub skip_permissions: bool,
}

impl Default for GeminiConfig {
    fn default() -> Self {
        Self {
            binary: default_gemini_binary(),
            extra_args: Vec::new(),
            prompt_mode: false,
            skip_permissions: false,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct GooseConfig {
    #[serde(default = "default_goose_binary")]
    pub binary: String,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

impl Default for GooseConfig {
    fn default() -> Self {
        Self {
            binary: default_goose_binary(),
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct InteractionAgentConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default = "default_quiet_timeout")]
    pub quiet_timeout_ms: u64,
}

impl Default for InteractionAgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            llm: LlmConfig::default(),
            quiet_timeout_ms: default_quiet_timeout(),
        }
    }
}

fn default_working_dir() -> PathBuf {
    dirs_or_home()
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_data_dir() -> PathBuf {
    home_dir().join(".agentsmith")
}

fn default_device_name() -> String {
    "agentsmith-worker".to_string()
}

fn default_true() -> bool {
    true
}

fn default_tool() -> String {
    "claude".to_string()
}

fn default_max_sessions() -> usize {
    5
}

fn default_flush_interval() -> u64 {
    500
}

fn default_max_chunk() -> usize {
    3000
}

fn default_claude_binary() -> String {
    "claude".to_string()
}

fn default_gemini_binary() -> String {
    "gemini".to_string()
}

fn default_goose_binary() -> String {
    "goose".to_string()
}

fn default_quiet_timeout() -> u64 {
    3000
}

fn home_dir() -> PathBuf {
    dirs_or_home()
}

fn dirs_or_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

impl Config {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let mut config = if path.exists() {
            let content = std::fs::read_to_string(path)?;
            toml::from_str::<Config>(&content)?
        } else {
            tracing::warn!("Config file not found at {}, using defaults", path.display());
            Config {
                daemon: DaemonConfig::default(),
                signal: SignalConfig::default(),
                slack: SlackConfig::default(),
                session_defaults: SessionDefaultsConfig::default(),
                claude: ClaudeConfig::default(),
                gemini: GeminiConfig::default(),
                goose: GooseConfig::default(),
                interaction_agent: InteractionAgentConfig::default(),
            }
        };

        // Apply env var overrides
        config.apply_env_overrides();
        Ok(config)
    }

    fn apply_env_overrides(&mut self) {
        if let Ok(val) = std::env::var("AGENTSMITH_DAEMON_WORKING_DIR") {
            self.daemon.working_dir = PathBuf::from(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_DAEMON_LOG_LEVEL") {
            self.daemon.log_level = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_DAEMON_DATA_DIR") {
            self.daemon.data_dir = PathBuf::from(val);
        }

        if let Ok(val) = std::env::var("AGENTSMITH_SIGNAL_ENABLED") {
            self.signal.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SIGNAL_DEVICE_NAME") {
            self.signal.device_name = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SIGNAL_DB_PATH") {
            self.signal.db_path = Some(PathBuf::from(val));
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SIGNAL_AUTHORIZED_USER") {
            self.signal.authorized_user = Some(val);
        }

        if let Ok(val) = std::env::var("AGENTSMITH_SLACK_ENABLED") {
            self.slack.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SLACK_APP_TOKEN") {
            self.slack.app_token = Some(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SLACK_BOT_TOKEN") {
            self.slack.bot_token = Some(val);
        }

        // Interaction agent overrides
        if let Ok(val) = std::env::var("AGENTSMITH_INTERACTION_AGENT_ENABLED") {
            self.interaction_agent.enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_INTERACTION_AGENT_PROVIDER") {
            self.interaction_agent.llm.provider = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_INTERACTION_AGENT_API_KEY") {
            self.interaction_agent.llm.api_key = Some(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_INTERACTION_AGENT_MODEL") {
            self.interaction_agent.llm.model = Some(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_INTERACTION_AGENT_QUIET_TIMEOUT_MS") {
            if let Ok(ms) = val.parse() {
                self.interaction_agent.quiet_timeout_ms = ms;
            }
        }

        // Claude overrides
        if let Ok(val) = std::env::var("AGENTSMITH_CLAUDE_PROMPT_MODE") {
            self.claude.prompt_mode = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_CLAUDE_SKIP_PERMISSIONS") {
            self.claude.skip_permissions = val.parse().unwrap_or(false);
        }

        // Gemini overrides
        if let Ok(val) = std::env::var("AGENTSMITH_GEMINI_PROMPT_MODE") {
            self.gemini.prompt_mode = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_GEMINI_SKIP_PERMISSIONS") {
            self.gemini.skip_permissions = val.parse().unwrap_or(false);
        }

        // Goose overrides
        if let Ok(val) = std::env::var("AGENTSMITH_GOOSE_BINARY") {
            self.goose.binary = val;
        }

        // Signal db_path default if not set
        if self.signal.db_path.is_none() {
            self.signal.db_path = Some(self.daemon.data_dir.join("signal-db"));
        }
    }

    pub fn signal_db_path(&self) -> PathBuf {
        self.signal
            .db_path
            .clone()
            .unwrap_or_else(|| self.daemon.data_dir.join("signal-db"))
    }
}
