pub mod api;
pub mod executor;
pub mod output;
pub mod store;

use std::collections::HashMap;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;

use crate::config::SchedulerConfig;
use self::store::ScheduleStore;

pub type ScheduleId = u64;

/// Status of a scheduled job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScheduleStatus {
    Active,
    Paused,
    Running,
}

impl std::fmt::Display for ScheduleStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ScheduleStatus::Active => write!(f, "active"),
            ScheduleStatus::Paused => write!(f, "paused"),
            ScheduleStatus::Running => write!(f, "running"),
        }
    }
}

/// Serializable thread origin for a schedule.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedThreadId {
    pub platform: String,
    pub thread_id: String,
}

/// Result of a single schedule execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleRunResult {
    pub success: bool,
    pub output: String,
    pub duration_secs: u64,
    pub completed_at: DateTime<Utc>,
}

/// A scheduled job definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: ScheduleId,
    pub name: String,
    pub cron_expr: String,
    pub tool: String,
    pub prompt: String,
    pub status: ScheduleStatus,
    pub origin: PersistedThreadId,
    pub created_at: DateTime<Utc>,
    pub next_run: Option<DateTime<Utc>>,
    pub last_run: Option<DateTime<Utc>>,
    pub last_result: Option<ScheduleRunResult>,
    pub run_count: u64,
    pub consecutive_failures: u32,
}

/// In-flight execution tracked by the scheduler.
struct InFlightExecution {
    handle: JoinHandle<ScheduleRunResult>,
}

/// Core scheduler. Manages scheduled jobs, persistence, and in-flight executions.
pub struct Scheduler {
    jobs: Vec<ScheduledJob>,
    store: ScheduleStore,
    in_flight: HashMap<ScheduleId, InFlightExecution>,
    next_id: ScheduleId,
    config: SchedulerConfig,
}

impl Scheduler {
    /// Create a new scheduler, loading persisted jobs from disk.
    pub fn new(config: SchedulerConfig, store: ScheduleStore) -> Self {
        let mut jobs = store.load();

        // Recovery: reset any Running jobs back to Active and recalculate next_run
        for job in &mut jobs {
            if job.status == ScheduleStatus::Running {
                job.status = ScheduleStatus::Active;
            }
            if job.status == ScheduleStatus::Active {
                job.next_run = calculate_next_run(&job.cron_expr);
            }
        }

        let next_id = jobs.iter().map(|j| j.id).max().unwrap_or(0) + 1;

        if !jobs.is_empty() {
            tracing::info!("Loaded {} scheduled jobs from disk", jobs.len());
        }

        Self {
            jobs,
            store,
            in_flight: HashMap::new(),
            next_id,
            config,
        }
    }

    /// Add a new scheduled job.
    pub fn add_job(
        &mut self,
        cron_expr: &str,
        tool: &str,
        prompt: &str,
        name: Option<&str>,
        origin_platform: &str,
        origin_thread_id: &str,
    ) -> anyhow::Result<&ScheduledJob> {
        if self.jobs.len() >= self.config.max_schedules {
            anyhow::bail!(
                "Maximum schedules ({}) reached. Delete one first.",
                self.config.max_schedules
            );
        }

        // Validate tool name
        match tool.to_lowercase().as_str() {
            "claude" | "gemini" | "goose" | "zeroclaw" => {}
            _ => anyhow::bail!("Unknown tool '{}'. Use claude, gemini, goose, or zeroclaw.", tool),
        }

        // Expand preset aliases and validate cron expression
        let expanded = expand_preset(cron_expr);
        let _ = cron::Schedule::from_str(&expanded)
            .map_err(|e| anyhow::anyhow!("Invalid cron expression '{}': {}", expanded, e))?;

        let auto_name = name
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("{}-{}", tool, self.next_id));

        let next_run = calculate_next_run(&expanded);

        let job = ScheduledJob {
            id: self.next_id,
            name: auto_name,
            cron_expr: expanded,
            tool: tool.to_lowercase(),
            prompt: prompt.to_string(),
            status: ScheduleStatus::Active,
            origin: PersistedThreadId {
                platform: origin_platform.to_string(),
                thread_id: origin_thread_id.to_string(),
            },
            created_at: Utc::now(),
            next_run,
            last_run: None,
            last_result: None,
            run_count: 0,
            consecutive_failures: 0,
        };

        self.next_id += 1;
        self.jobs.push(job);
        self.persist();

        Ok(self.jobs.last().unwrap())
    }

    /// Delete a job by ID.
    pub fn delete_job(&mut self, id: ScheduleId) -> anyhow::Result<()> {
        let idx = self.jobs.iter().position(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Schedule #{} not found", id))?;
        self.jobs.remove(idx);
        // Abort in-flight execution if any
        if let Some(exec) = self.in_flight.remove(&id) {
            exec.handle.abort();
        }
        self.persist();
        Ok(())
    }

    /// Pause a job.
    pub fn pause_job(&mut self, id: ScheduleId) -> anyhow::Result<()> {
        let job = self.jobs.iter_mut().find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Schedule #{} not found", id))?;
        job.status = ScheduleStatus::Paused;
        job.next_run = None;
        self.persist();
        Ok(())
    }

    /// Resume a paused job.
    pub fn resume_job(&mut self, id: ScheduleId) -> anyhow::Result<()> {
        let job = self.jobs.iter_mut().find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Schedule #{} not found", id))?;
        if job.status != ScheduleStatus::Paused {
            anyhow::bail!("Schedule #{} is not paused (status: {})", id, job.status);
        }
        job.status = ScheduleStatus::Active;
        job.next_run = calculate_next_run(&job.cron_expr);
        job.consecutive_failures = 0;
        self.persist();
        Ok(())
    }

    /// Force-trigger a job immediately (sets next_run to now).
    pub fn trigger_now(&mut self, id: ScheduleId) -> anyhow::Result<()> {
        let job = self.jobs.iter_mut().find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Schedule #{} not found", id))?;
        if job.status == ScheduleStatus::Running {
            anyhow::bail!("Schedule #{} is already running", id);
        }
        job.status = ScheduleStatus::Active;
        job.next_run = Some(Utc::now());
        Ok(())
    }

    /// List all jobs.
    pub fn list_jobs(&self) -> &[ScheduledJob] {
        &self.jobs
    }

    /// Get a specific job.
    pub fn get_job(&self, id: ScheduleId) -> Option<&ScheduledJob> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// Collect IDs of jobs that are due to run.
    pub fn collect_due_jobs(&mut self) -> Vec<ScheduleId> {
        let now = Utc::now();
        let mut due = Vec::new();

        for job in &mut self.jobs {
            if job.status != ScheduleStatus::Active {
                continue;
            }
            if let Some(next) = job.next_run {
                if next <= now {
                    due.push(job.id);
                    job.status = ScheduleStatus::Running;
                }
            }
        }

        due
    }

    /// Check if we can accept more concurrent executions.
    pub fn can_execute(&self) -> bool {
        self.in_flight.len() < self.config.max_concurrent_executions
    }

    /// Register an in-flight execution.
    pub fn register_execution(&mut self, id: ScheduleId, handle: JoinHandle<ScheduleRunResult>) {
        self.in_flight.insert(id, InFlightExecution { handle });
    }

    /// Collect finished executions. Returns (job_id, result) pairs.
    pub fn collect_finished(&mut self) -> Vec<(ScheduleId, ScheduleRunResult)> {
        let mut finished = Vec::new();
        let mut completed_ids = Vec::new();

        for (&id, exec) in &self.in_flight {
            if exec.handle.is_finished() {
                completed_ids.push(id);
            }
        }

        for id in completed_ids {
            if let Some(exec) = self.in_flight.remove(&id) {
                match exec.handle.now_or_never() {
                    Some(Ok(result)) => {
                        finished.push((id, result));
                    }
                    Some(Err(e)) => {
                        tracing::error!("Scheduled job #{} panicked: {}", id, e);
                        finished.push((
                            id,
                            ScheduleRunResult {
                                success: false,
                                output: format!("Task panicked: {}", e),
                                duration_secs: 0,
                                completed_at: Utc::now(),
                            },
                        ));
                    }
                    None => {
                        // Shouldn't happen since we checked is_finished
                        tracing::warn!("Job #{} reported finished but result not ready", id);
                    }
                }
            }
        }

        finished
    }

    /// Record a completed execution, update job state, and advance next_run.
    pub fn record_completion(&mut self, id: ScheduleId, result: &ScheduleRunResult) {
        if let Some(job) = self.jobs.iter_mut().find(|j| j.id == id) {
            job.last_run = Some(Utc::now());
            job.last_result = Some(result.clone());
            job.run_count += 1;

            if result.success {
                job.consecutive_failures = 0;
                job.status = ScheduleStatus::Active;
                job.next_run = calculate_next_run(&job.cron_expr);
            } else {
                job.consecutive_failures += 1;
                if job.consecutive_failures >= self.config.max_consecutive_failures {
                    tracing::warn!(
                        "Schedule #{} '{}' auto-paused after {} consecutive failures",
                        job.id, job.name, job.consecutive_failures
                    );
                    job.status = ScheduleStatus::Paused;
                    job.next_run = None;
                } else {
                    job.status = ScheduleStatus::Active;
                    job.next_run = calculate_next_run(&job.cron_expr);
                }
            }

            self.persist();
        }
    }

    /// Get the execution timeout from config.
    pub fn execution_timeout_secs(&self) -> u64 {
        self.config.execution_timeout_secs
    }

    /// Get scheduler summary for status display.
    pub fn summary(&self) -> SchedulerSummary {
        let mut active = 0;
        let mut paused = 0;
        let mut running = 0;
        for job in &self.jobs {
            match job.status {
                ScheduleStatus::Active => active += 1,
                ScheduleStatus::Paused => paused += 1,
                ScheduleStatus::Running => running += 1,
            }
        }
        SchedulerSummary {
            total: self.jobs.len(),
            active,
            paused,
            running,
        }
    }

    fn persist(&self) {
        if let Err(e) = self.store.save(&self.jobs) {
            tracing::error!("Failed to persist schedules: {}", e);
        }
    }
}

pub struct SchedulerSummary {
    pub total: usize,
    pub active: usize,
    pub paused: usize,
    pub running: usize,
}

/// Expand preset cron aliases to standard cron expressions.
pub fn expand_preset(expr: &str) -> String {
    match expr.trim().to_lowercase().as_str() {
        "@daily" => "0 0 8 * * *".to_string(),
        "@hourly" => "0 0 * * * *".to_string(),
        "@weekly" => "0 0 8 * * 1".to_string(),
        "@twice-daily" => "0 0 8,18 * * *".to_string(),
        "@every-30m" => "0 */30 * * * *".to_string(),
        "@weekdays" => "0 0 8 * * 1-5".to_string(),
        _ => expr.to_string(),
    }
}

/// Calculate the next run time for a cron expression.
fn calculate_next_run(cron_expr: &str) -> Option<DateTime<Utc>> {
    cron::Schedule::from_str(cron_expr)
        .ok()
        .and_then(|sched| sched.upcoming(Utc).next())
}

/// Helper for JoinHandle polling (futures 0.3 FutureExt).
trait NowOrNever {
    type Output;
    fn now_or_never(self) -> Option<Self::Output>;
}

impl<T> NowOrNever for JoinHandle<T> {
    type Output = Result<T, tokio::task::JoinError>;
    fn now_or_never(self) -> Option<Self::Output> {
        // Since we checked is_finished, we can block briefly
        futures::FutureExt::now_or_never(self)
    }
}
