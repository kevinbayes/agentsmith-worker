use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::agent_mgmt::recipe::Recipe;
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
    pub telegram: TelegramConfig,
    #[serde(default)]
    pub web: WebConfig,
    #[serde(default)]
    pub session_defaults: SessionDefaultsConfig,
    #[serde(default)]
    pub claude: ClaudeConfig,
    #[serde(default)]
    pub zeroclaw: ZeroclawConfig,
    #[serde(default)]
    pub interaction_agent: InteractionAgentConfig,
    #[serde(default)]
    pub reporter: ReporterConfig,
    #[serde(default)]
    pub agent: AgentConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub agent_management: AgentManagementConfig,
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
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Bot token from @BotFather
    #[serde(default)]
    pub bot_token: Option<String>,
    /// Optional: restrict to a single authorized Telegram user ID
    #[serde(default)]
    pub authorized_user: Option<i64>,
    /// Only respond to direct messages (ignore group chats)
    #[serde(default = "default_true")]
    pub dm_only: bool,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_token: None,
            authorized_user: None,
            dm_only: true,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct WebConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_web_host")]
    pub host: String,
    #[serde(default = "default_web_port")]
    pub port: u16,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_web_host(),
            port: default_web_port(),
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
pub struct ZeroclawConfig {
    #[serde(default = "default_zeroclaw_binary")]
    pub binary: String,
    #[serde(default)]
    pub extra_args: Vec<String>,
    #[serde(default)]
    pub prompt_mode: bool,
}

impl Default for ZeroclawConfig {
    fn default() -> Self {
        Self {
            binary: default_zeroclaw_binary(),
            extra_args: Vec::new(),
            prompt_mode: false,
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

#[derive(Debug, Deserialize, Clone)]
pub struct ReporterConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub endpoint: String,
    #[serde(default)]
    pub token_url: String,
    #[serde(default)]
    pub client_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub worker_name: Option<String>,
    #[serde(default = "default_report_interval_secs")]
    pub interval_secs: u64,
}

impl Default for ReporterConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            endpoint: String::new(),
            token_url: String::new(),
            client_id: String::new(),
            client_secret: String::new(),
            worker_name: None,
            interval_secs: default_report_interval_secs(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct AgentConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub auto_agent_mode: bool,
    #[serde(default = "default_max_context_turns")]
    pub max_context_turns: usize,
    #[serde(default)]
    pub llm: LlmConfig,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_agent_mode: false,
            max_context_turns: default_max_context_turns(),
            llm: LlmConfig::default(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct SchedulerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max_schedules")]
    pub max_schedules: usize,
    #[serde(default = "default_max_concurrent_executions")]
    pub max_concurrent_executions: usize,
    #[serde(default = "default_execution_timeout_secs")]
    pub execution_timeout_secs: u64,
    #[serde(default = "default_max_consecutive_failures")]
    pub max_consecutive_failures: u32,
    #[serde(default)]
    pub gcs: Option<GcsConfig>,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_schedules: default_max_schedules(),
            max_concurrent_executions: default_max_concurrent_executions(),
            execution_timeout_secs: default_execution_timeout_secs(),
            max_consecutive_failures: default_max_consecutive_failures(),
            gcs: None,
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct AgentManagementConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Directory where entry-point symlinks are placed. Must be on the
    /// operator's `$PATH` for installed agents to be invokable from any
    /// shell. Defaults to `~/.local/bin`.
    #[serde(default = "default_bin_dir")]
    pub bin_dir: PathBuf,
    /// Per-recipe overrides keyed by agent name. Overrides built-in recipes.
    #[serde(default)]
    pub recipes: HashMap<String, Recipe>,
    /// Grace period between SIGTERM and SIGKILL when killing processes.
    #[serde(default = "default_kill_grace_ms")]
    pub kill_grace_ms: u64,
}

impl Default for AgentManagementConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bin_dir: default_bin_dir(),
            recipes: HashMap::new(),
            kill_grace_ms: default_kill_grace_ms(),
        }
    }
}

fn default_bin_dir() -> PathBuf {
    home_dir().join(".local").join("bin")
}

fn default_kill_grace_ms() -> u64 {
    5_000
}

#[derive(Debug, Deserialize, Clone)]
pub struct GcsConfig {
    pub bucket: String,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub credentials_path: Option<std::path::PathBuf>,
}

fn default_max_schedules() -> usize {
    50
}

fn default_max_concurrent_executions() -> usize {
    3
}

fn default_execution_timeout_secs() -> u64 {
    300
}

fn default_max_consecutive_failures() -> u32 {
    3
}

fn default_max_context_turns() -> usize {
    50
}

fn default_report_interval_secs() -> u64 {
    30
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

fn default_zeroclaw_binary() -> String {
    "zeroclaw".to_string()
}

fn default_quiet_timeout() -> u64 {
    3000
}

fn default_web_host() -> String {
    "127.0.0.1".to_string()
}

fn default_web_port() -> u16 {
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
                telegram: TelegramConfig::default(),
                web: WebConfig::default(),
                session_defaults: SessionDefaultsConfig::default(),
                claude: ClaudeConfig::default(),
                zeroclaw: ZeroclawConfig::default(),
                interaction_agent: InteractionAgentConfig::default(),
                reporter: ReporterConfig::default(),
                agent: AgentConfig::default(),
                scheduler: SchedulerConfig::default(),
                agent_management: AgentManagementConfig::default(),
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

        if let Ok(val) = std::env::var("AGENTSMITH_TELEGRAM_ENABLED") {
            self.telegram.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_TELEGRAM_BOT_TOKEN") {
            self.telegram.bot_token = Some(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_TELEGRAM_AUTHORIZED_USER") {
            if let Ok(id) = val.parse() {
                self.telegram.authorized_user = Some(id);
            }
        }

        // Web overrides
        if let Ok(val) = std::env::var("AGENTSMITH_WEB_ENABLED") {
            self.web.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_WEB_HOST") {
            self.web.host = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_WEB_PORT") {
            if let Ok(port) = val.parse() {
                self.web.port = port;
            }
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

        // ZeroClaw overrides
        if let Ok(val) = std::env::var("AGENTSMITH_ZEROCLAW_BINARY") {
            self.zeroclaw.binary = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_ZEROCLAW_PROMPT_MODE") {
            self.zeroclaw.prompt_mode = val.parse().unwrap_or(false);
        }

        // Reporter overrides
        if let Ok(val) = std::env::var("AGENTSMITH_REPORTER_ENABLED") {
            self.reporter.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_REPORTER_ENDPOINT") {
            self.reporter.endpoint = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_REPORTER_TOKEN_URL") {
            self.reporter.token_url = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_REPORTER_CLIENT_ID") {
            self.reporter.client_id = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_REPORTER_CLIENT_SECRET") {
            self.reporter.client_secret = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_REPORTER_WORKER_NAME") {
            self.reporter.worker_name = Some(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_REPORTER_INTERVAL_SECS") {
            if let Ok(secs) = val.parse() {
                self.reporter.interval_secs = secs;
            }
        }

        // Agent overrides
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_ENABLED") {
            self.agent.enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_AUTO_AGENT_MODE") {
            self.agent.auto_agent_mode = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_MAX_CONTEXT_TURNS") {
            if let Ok(n) = val.parse() {
                self.agent.max_context_turns = n;
            }
        }
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_PROVIDER") {
            self.agent.llm.provider = val;
        }
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_API_KEY") {
            self.agent.llm.api_key = Some(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_MODEL") {
            self.agent.llm.model = Some(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_MAX_TOKENS") {
            if let Ok(n) = val.parse() {
                self.agent.llm.max_tokens = n;
            }
        }

        // Scheduler overrides
        if let Ok(val) = std::env::var("AGENTSMITH_SCHEDULER_ENABLED") {
            self.scheduler.enabled = val.parse().unwrap_or(false);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SCHEDULER_MAX_SCHEDULES") {
            if let Ok(n) = val.parse() {
                self.scheduler.max_schedules = n;
            }
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SCHEDULER_MAX_CONCURRENT") {
            if let Ok(n) = val.parse() {
                self.scheduler.max_concurrent_executions = n;
            }
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SCHEDULER_TIMEOUT_SECS") {
            if let Ok(n) = val.parse() {
                self.scheduler.execution_timeout_secs = n;
            }
        }
        if let Ok(val) = std::env::var("AGENTSMITH_SCHEDULER_MAX_FAILURES") {
            if let Ok(n) = val.parse() {
                self.scheduler.max_consecutive_failures = n;
            }
        }

        // Agent management overrides
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_MANAGEMENT_ENABLED") {
            self.agent_management.enabled = val.parse().unwrap_or(true);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_MANAGEMENT_BIN_DIR") {
            self.agent_management.bin_dir = PathBuf::from(val);
        }
        if let Ok(val) = std::env::var("AGENTSMITH_AGENT_MANAGEMENT_KILL_GRACE_MS") {
            if let Ok(n) = val.parse() {
                self.agent_management.kill_grace_ms = n;
            }
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
