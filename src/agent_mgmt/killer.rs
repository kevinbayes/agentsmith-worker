use std::path::PathBuf;
use std::time::Duration;

use sysinfo::{ProcessRefreshKind, RefreshKind, System};

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct KillResult {
    pub killed: Vec<u32>,
    pub failed: Vec<u32>,
    /// True if no matching processes were found at all.
    pub none_found: bool,
}

impl KillResult {
    pub fn merge(&mut self, other: KillResult) {
        self.killed.extend(other.killed);
        self.failed.extend(other.failed);
        self.none_found = self.none_found && other.none_found;
    }
}

/// Refresh sysinfo and return PIDs whose process basename matches
/// `binary_basename`, plus PIDs of `node` processes whose argv contains it
/// (covers Node-based CLIs like the original openclaw integration).
fn find_matching_pids(binary_basename: &str) -> Vec<u32> {
    let sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );

    let mut pids = Vec::new();
    for (pid, process) in sys.processes() {
        let proc_name = process.name().to_string_lossy().to_string();
        if proc_name == binary_basename {
            pids.push(pid.as_u32());
            continue;
        }
        if proc_name == "node"
            && process
                .cmd()
                .iter()
                .any(|arg| arg.to_string_lossy().contains(binary_basename))
        {
            pids.push(pid.as_u32());
        }
    }
    pids
}

fn binary_basename(binary: &str) -> String {
    PathBuf::from(binary)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| binary.to_string())
}

/// Send `signal` to `pid` via sysinfo. Returns true if delivery reported success.
fn signal_pid(pid: u32, signal: sysinfo::Signal) -> bool {
    let sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    match sys.process(sysinfo::Pid::from_u32(pid)) {
        Some(p) => p.kill_with(signal).unwrap_or(false),
        None => false,
    }
}

/// Returns true if a process with the given PID currently exists.
fn pid_alive(pid: u32) -> bool {
    let sys = System::new_with_specifics(
        RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
    );
    sys.process(sysinfo::Pid::from_u32(pid)).is_some()
}

/// Kill every process matching `binary_basename` (basename of the executable,
/// or a `node` process with the name in its argv). SIGTERM, wait `grace`,
/// then SIGKILL anything still alive.
pub async fn kill_by_binary(binary: &str, grace: Duration) -> KillResult {
    let basename = binary_basename(binary);
    let pids = find_matching_pids(&basename);

    if pids.is_empty() {
        return KillResult {
            killed: vec![],
            failed: vec![],
            none_found: true,
        };
    }

    let mut result = KillResult::default();
    let mut sigtermed = Vec::new();
    for pid in &pids {
        if signal_pid(*pid, sysinfo::Signal::Term) {
            sigtermed.push(*pid);
        } else {
            result.failed.push(*pid);
        }
    }

    if !sigtermed.is_empty() {
        tokio::time::sleep(grace).await;
    }

    for pid in sigtermed {
        if pid_alive(pid) {
            if signal_pid(pid, sysinfo::Signal::Kill) {
                result.killed.push(pid);
            } else {
                result.failed.push(pid);
            }
        } else {
            result.killed.push(pid);
        }
    }

    result
}

/// Kill a single PID with SIGTERM, wait `grace`, then SIGKILL if still alive.
pub async fn kill_by_pid(pid: u32, grace: Duration) -> KillResult {
    if !pid_alive(pid) {
        return KillResult {
            killed: vec![],
            failed: vec![],
            none_found: true,
        };
    }

    if !signal_pid(pid, sysinfo::Signal::Term) {
        return KillResult {
            killed: vec![],
            failed: vec![pid],
            none_found: false,
        };
    }

    tokio::time::sleep(grace).await;

    if pid_alive(pid) {
        if signal_pid(pid, sysinfo::Signal::Kill) {
            KillResult {
                killed: vec![pid],
                failed: vec![],
                none_found: false,
            }
        } else {
            KillResult {
                killed: vec![],
                failed: vec![pid],
                none_found: false,
            }
        }
    } else {
        KillResult {
            killed: vec![pid],
            failed: vec![],
            none_found: false,
        }
    }
}
