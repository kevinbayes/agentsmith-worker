use std::path::Path;
use std::time::Instant;

use chrono::Utc;
use tokio::process::Command;

use crate::config::Config;
use crate::scheduler::ScheduleRunResult;

/// Maximum output length to store (4000 chars).
const MAX_OUTPUT_LEN: usize = 4000;

/// Execute a scheduled job by spawning the tool in prompt mode.
///
/// Returns the result of the execution. This is always a one-shot,
/// non-interactive process with stdin closed.
pub async fn execute_scheduled_job(
    tool: &str,
    prompt: &str,
    config: &Config,
    working_dir: &Path,
    timeout_secs: u64,
) -> ScheduleRunResult {
    let start = Instant::now();

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        run_tool(tool, prompt, config, working_dir),
    )
    .await;

    let duration_secs = start.elapsed().as_secs();

    match result {
        Ok(Ok(output)) => ScheduleRunResult {
            success: true,
            output: truncate(&output, MAX_OUTPUT_LEN),
            duration_secs,
            completed_at: Utc::now(),
        },
        Ok(Err(e)) => ScheduleRunResult {
            success: false,
            output: truncate(&format!("Error: {}", e), MAX_OUTPUT_LEN),
            duration_secs,
            completed_at: Utc::now(),
        },
        Err(_) => ScheduleRunResult {
            success: false,
            output: format!("Execution timed out after {}s", timeout_secs),
            duration_secs,
            completed_at: Utc::now(),
        },
    }
}

async fn run_tool(
    tool: &str,
    prompt: &str,
    config: &Config,
    working_dir: &Path,
) -> anyhow::Result<String> {
    let (binary, args) = build_command(tool, prompt, config)?;

    tracing::debug!(
        "Scheduled job spawning: {} {}",
        binary,
        args.join(" ")
    );

    let output = Command::new(&binary)
        .args(&args)
        .current_dir(working_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await?;

    let mut result = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&stderr);
    }

    if !output.status.success() && result.is_empty() {
        result = format!("Process exited with {}", output.status);
    }

    Ok(result)
}

fn build_command(
    tool: &str,
    prompt: &str,
    config: &Config,
) -> anyhow::Result<(String, Vec<String>)> {
    match tool.to_lowercase().as_str() {
        "claude" => {
            let mut args = vec!["-p".to_string(), prompt.to_string()];
            if config.claude.skip_permissions {
                args.push("--dangerously-skip-permissions".to_string());
            }
            args.extend(config.claude.extra_args.clone());
            Ok((config.claude.binary.clone(), args))
        }
        "gemini" => {
            let mut args = vec!["-p".to_string(), prompt.to_string()];
            args.extend(config.gemini.extra_args.clone());
            Ok((config.gemini.binary.clone(), args))
        }
        "goose" => {
            let mut args = vec![
                "run".to_string(),
                "-t".to_string(),
                prompt.to_string(),
            ];
            args.extend(config.goose.extra_args.clone());
            Ok((config.goose.binary.clone(), args))
        }
        "zeroclaw" => {
            let mut args = vec!["-p".to_string(), prompt.to_string()];
            args.extend(config.zeroclaw.extra_args.clone());
            Ok((config.zeroclaw.binary.clone(), args))
        }
        _ => anyhow::bail!("Unknown tool: {}", tool),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut truncated = s[..max].to_string();
        truncated.push_str("\n... (output truncated)");
        truncated
    }
}
