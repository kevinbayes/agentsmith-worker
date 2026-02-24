use std::path::{Path, PathBuf};

use crate::scheduler::ScheduledJob;

/// Persists scheduled jobs to a JSON file.
pub struct ScheduleStore {
    path: PathBuf,
}

impl ScheduleStore {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("schedules.json"),
        }
    }

    /// Load jobs from disk. Returns an empty Vec on missing or corrupt file.
    pub fn load(&self) -> Vec<ScheduledJob> {
        match std::fs::read_to_string(&self.path) {
            Ok(content) => match serde_json::from_str(&content) {
                Ok(jobs) => jobs,
                Err(e) => {
                    tracing::warn!("Failed to parse schedules.json: {}, starting fresh", e);
                    Vec::new()
                }
            },
            Err(_) => Vec::new(),
        }
    }

    /// Atomic write: write to .tmp then rename.
    pub fn save(&self, jobs: &[ScheduledJob]) -> anyhow::Result<()> {
        let tmp = self.path.with_extension("json.tmp");
        let content = serde_json::to_string_pretty(jobs)?;
        std::fs::write(&tmp, content)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}
