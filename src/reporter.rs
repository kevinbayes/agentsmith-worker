use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sysinfo::System;

use crate::config::ReporterConfig;
use crate::monitor::{Monitor, ToolInfo};
use crate::session::{SessionManager, SessionInfo};

// ── Payload types ───────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct StateReport {
    worker: WorkerIdentity,
    agents: Vec<AgentReport>,
    sessions: Vec<SessionReport>,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct WorkerIdentity {
    hostname: String,
    worker_name: String,
    version: String,
}

#[derive(Debug, Serialize)]
struct AgentReport {
    name: String,
    binary: String,
    installed: bool,
    version: Option<String>,
    path: Option<String>,
    running_instances: Vec<ProcessReport>,
}

#[derive(Debug, Serialize)]
struct ProcessReport {
    pid: u32,
    name: String,
    cmd_summary: String,
}

#[derive(Debug, Serialize)]
struct SessionReport {
    id: u64,
    agent: String,
    status: String,
}

// ── OIDC token response ─────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    #[allow(dead_code)]
    token_type: String,
}

// ── Conversions ─────────────────────────────────────────────────────────

impl AgentReport {
    fn from_tool_info(info: &ToolInfo) -> Self {
        Self {
            name: info.name.clone(),
            binary: info.binary.clone(),
            installed: info.installed,
            version: info.version.clone(),
            path: info.path.as_ref().map(|p| p.display().to_string()),
            running_instances: info
                .running_instances
                .iter()
                .map(|ri| ProcessReport {
                    pid: ri.pid,
                    name: ri.name.clone(),
                    cmd_summary: ri.cmd_summary.clone(),
                })
                .collect(),
        }
    }
}

impl SessionReport {
    fn from_session_info(info: &SessionInfo) -> Self {
        Self {
            id: info.id,
            agent: info.tool.to_string(),
            status: info.status.to_string(),
        }
    }
}

// ── Reporter ────────────────────────────────────────────────────────────

pub struct Reporter {
    client: reqwest::Client,
    endpoint: String,
    token_url: String,
    client_id: String,
    client_secret: String,
    worker_name: String,
    hostname: String,
    interval: Duration,
    last_report: Instant,
    cached_token: Option<String>,
    token_expiry: Instant,
}

impl Reporter {
    /// Create a Reporter from config. Returns `None` if disabled or
    /// missing required fields (endpoint, token_url, client_id, client_secret).
    pub fn new(config: &ReporterConfig) -> Option<Self> {
        if !config.enabled {
            return None;
        }

        if config.endpoint.is_empty()
            || config.token_url.is_empty()
            || config.client_id.is_empty()
            || config.client_secret.is_empty()
        {
            tracing::warn!(
                "Reporter enabled but missing endpoint, token_url, client_id, or client_secret — disabling"
            );
            return None;
        }

        let hostname = System::host_name().unwrap_or_else(|| "unknown".to_string());
        let worker_name = config
            .worker_name
            .clone()
            .unwrap_or_else(|| hostname.clone());
        let interval = Duration::from_secs(config.interval_secs);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .ok()?;

        tracing::info!(
            "State reporter enabled — endpoint={}, token_url={}, interval={}s, worker={}",
            config.endpoint,
            config.token_url,
            config.interval_secs,
            worker_name,
        );

        Some(Self {
            client,
            endpoint: config.endpoint.clone(),
            token_url: config.token_url.clone(),
            client_id: config.client_id.clone(),
            client_secret: config.client_secret.clone(),
            worker_name,
            hostname,
            interval,
            // Fire immediately on first tick by pretending last report was long ago.
            last_report: Instant::now() - interval,
            cached_token: None,
            token_expiry: Instant::now(),
        })
    }

    /// Fetch a new access token from the OIDC token endpoint using the
    /// client credentials grant.
    async fn fetch_token(&self) -> anyhow::Result<TokenResponse> {
        let resp = self
            .client
            .post(&self.token_url)
            .form(&[
                ("grant_type", "client_credentials"),
                ("client_id", &self.client_id),
                ("client_secret", &self.client_secret),
            ])
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Token endpoint returned HTTP {}: {}", status, body);
        }

        Ok(resp.json::<TokenResponse>().await?)
    }

    /// Return a valid bearer token, fetching a new one if the cached token
    /// is missing or within 30 seconds of expiry.
    async fn get_valid_token(&mut self) -> anyhow::Result<String> {
        let buffer = Duration::from_secs(30);
        if let Some(ref token) = self.cached_token {
            if self.token_expiry.checked_duration_since(Instant::now()).unwrap_or_default() > buffer {
                return Ok(token.clone());
            }
        }

        let token_resp = self.fetch_token().await?;
        self.token_expiry = Instant::now() + Duration::from_secs(token_resp.expires_in);
        self.cached_token = Some(token_resp.access_token.clone());
        Ok(token_resp.access_token)
    }

    /// Check if it's time to report; if so, obtain a valid bearer token,
    /// build the payload, and spawn an async POST (fire-and-forget).
    pub async fn maybe_report(&mut self, monitor: &Monitor, session_mgr: &SessionManager) {
        if self.last_report.elapsed() < self.interval {
            return;
        }
        self.last_report = Instant::now();

        let token = match self.get_valid_token().await {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!("Failed to obtain OIDC token: {}", e);
                return;
            }
        };

        let report = self.build_report(monitor, session_mgr);

        let client = self.client.clone();
        let endpoint = self.endpoint.clone();

        tokio::spawn(async move {
            if let Err(e) = Self::send_report(client, &endpoint, &token, &report).await {
                tracing::warn!("Failed to send state report: {}", e);
            }
        });
    }

    fn build_report(&self, monitor: &Monitor, session_mgr: &SessionManager) -> StateReport {
        let tools = monitor.status();
        let sessions = session_mgr.list_sessions();

        StateReport {
            worker: WorkerIdentity {
                hostname: self.hostname.clone(),
                worker_name: self.worker_name.clone(),
                version: env!("CARGO_PKG_VERSION").to_string(),
            },
            agents: tools.iter().map(AgentReport::from_tool_info).collect(),
            sessions: sessions.iter().map(SessionReport::from_session_info).collect(),
            timestamp: Utc::now(),
        }
    }

    async fn send_report(
        client: reqwest::Client,
        endpoint: &str,
        token: &str,
        report: &StateReport,
    ) -> anyhow::Result<()> {
        let resp = client
            .post(endpoint)
            .bearer_auth(token)
            .json(report)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            tracing::warn!("Control center returned HTTP {}", status);
        }

        Ok(())
    }
}
