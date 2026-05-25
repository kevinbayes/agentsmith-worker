use std::path::PathBuf;

use sysinfo::{ProcessRefreshKind, RefreshKind, System};

use crate::config::Config;

/// Information about a single tool's installation and running state.
#[derive(Debug)]
pub struct ToolInfo {
    pub name: String,
    pub binary: String,
    pub installed: bool,
    pub path: Option<PathBuf>,
    pub version: Option<String>,
    pub running_instances: Vec<RunningInstance>,
}

/// A running OS process matching a tool.
#[derive(Debug)]
pub struct RunningInstance {
    pub pid: u32,
    pub name: String,
    pub cmd_summary: String,
}

/// Definition of a tool to monitor.
struct ToolDef {
    name: String,
    binary: String,
    /// If true, also match `node` processes whose command-line contains the binary name.
    match_node: bool,
}

/// Monitors tool installation and running processes.
pub struct Monitor {
    tools: Vec<ToolDef>,
}

impl Monitor {
    /// Build a Monitor from the daemon config, extracting binary names for each tool.
    pub fn from_config(config: &Config) -> Self {
        let tools = vec![
            ToolDef {
                name: "Claude Code".to_string(),
                binary: config.claude.binary.clone(),
                match_node: false,
            },
            ToolDef {
                name: "ZeroClaw".to_string(),
                binary: config.zeroclaw.binary.clone(),
                match_node: false,
            },
            ToolDef {
                name: "OpenClaw".to_string(),
                binary: "openclaw".to_string(),
                match_node: true,
            },
        ];
        Self { tools }
    }

    /// Check installation status and enumerate running processes for all tools.
    pub fn status(&self) -> Vec<ToolInfo> {
        let sys = System::new_with_specifics(
            RefreshKind::nothing().with_processes(ProcessRefreshKind::everything()),
        );

        self.tools
            .iter()
            .map(|def| {
                let (installed, path) = match which::which(&def.binary) {
                    Ok(p) => (true, Some(p)),
                    Err(_) => (false, None),
                };

                let version = if installed {
                    detect_version(&def.binary)
                } else {
                    None
                };

                let running_instances = self.find_processes(&sys, def);

                ToolInfo {
                    name: def.name.clone(),
                    binary: def.binary.clone(),
                    installed,
                    path,
                    version,
                    running_instances,
                }
            })
            .collect()
    }

    /// Produce a human-readable status report.
    pub fn format_status(&self) -> String {
        let infos = self.status();
        let mut lines = vec!["*Tool Monitor:*".to_string()];

        for info in &infos {
            let install_status = if info.installed {
                format!(
                    "installed ({})",
                    info.path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default()
                )
            } else {
                "not found".to_string()
            };

            let version_str = info
                .version
                .as_deref()
                .map(|v| format!(" [{}]", v))
                .unwrap_or_default();
            lines.push(format!(
                "  *{}* (`{}`): {}{}",
                info.name, info.binary, install_status, version_str
            ));

            if info.running_instances.is_empty() {
                lines.push("    No running instances".to_string());
            } else {
                for inst in &info.running_instances {
                    lines.push(format!(
                        "    PID {} - {} ({})",
                        inst.pid, inst.name, inst.cmd_summary
                    ));
                }
            }
        }

        lines.join("\n")
    }

    /// Find running processes that match a tool definition.
    fn find_processes(&self, sys: &System, def: &ToolDef) -> Vec<RunningInstance> {
        let binary_basename = PathBuf::from(&def.binary)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| def.binary.clone());

        let mut instances = Vec::new();

        for (pid, process) in sys.processes() {
            let proc_name = process.name().to_string_lossy().to_string();

            let matched = if proc_name == binary_basename {
                true
            } else if def.match_node && proc_name == "node" {
                // For Node.js tools, check if any command-line arg contains the binary name
                process
                    .cmd()
                    .iter()
                    .any(|arg| arg.to_string_lossy().contains(&binary_basename))
            } else {
                false
            };

            if matched {
                let cmd_summary: String = process
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(" ");
                let cmd_summary = if cmd_summary.len() > 120 {
                    format!("{}...", &cmd_summary[..117])
                } else {
                    cmd_summary
                };

                instances.push(RunningInstance {
                    pid: pid.as_u32(),
                    name: proc_name,
                    cmd_summary,
                });
            }
        }

        instances
    }
}

/// Run `<binary> --version` and return the first non-empty line of output.
fn detect_version(binary: &str) -> Option<String> {
    let output = std::process::Command::new(binary)
        .arg("--version")
        .output()
        .ok()?;

    // Some tools print version to stdout, others to stderr.
    let text = if !output.stdout.is_empty() {
        String::from_utf8_lossy(&output.stdout).to_string()
    } else {
        String::from_utf8_lossy(&output.stderr).to_string()
    };

    text.lines()
        .map(|l| l.trim())
        .find(|l| !l.is_empty())
        .map(|l| l.to_string())
}
