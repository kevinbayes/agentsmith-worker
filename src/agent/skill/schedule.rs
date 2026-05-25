use anyhow::Result;

use super::{SkillContext, SkillOutput};
use crate::scheduler::expand_preset;

/// Agent skill for managing scheduled tasks via natural language.
pub struct ScheduleSkill;

impl ScheduleSkill {
    pub async fn execute(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> Result<SkillOutput> {
        let scheduler = match &ctx.scheduler {
            Some(s) => s.clone(),
            None => {
                return Ok(SkillOutput::Reply(
                    "Scheduler is not enabled. Set `scheduler.enabled = true` in config.".to_string(),
                ));
            }
        };

        // Use LLM to parse the natural language scheduling request
        let system = r#"You are a scheduling assistant for AgentSmith. Parse the user's request about scheduled tasks.

Respond with ONLY a JSON object (no markdown fencing):
{
  "action": "add|list|delete|pause|resume|run|info",
  "cron": "<cron expression or preset alias>",
  "tool": "<claude|zeroclaw>",
  "prompt": "<the prompt to run>",
  "id": <schedule id number>,
  "reasoning": "<brief explanation>"
}

For "add" actions, extract the cron schedule, tool, and prompt. Use these preset aliases when appropriate:
- @daily (8 AM daily)
- @hourly (every hour)
- @weekly (8 AM Mondays)
- @twice-daily (8 AM and 6 PM)
- @every-30m (every 30 minutes)
- @weekdays (8 AM weekdays)

Or use a standard 6-field cron expression (sec min hour day month weekday): e.g. "0 0 8 * * *"

For "list" actions, just set action to "list".
For "delete", "pause", "resume", "run", "info" actions, include the schedule "id".

Only include fields relevant to the action."#;

        let response = ctx.llm.generate(system, input).await?;

        // Parse LLM response
        let json_str = extract_json(&response);
        let value: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => {
                return Ok(SkillOutput::Reply(
                    "I couldn't understand that scheduling request. Try something like:\n\
                     - \"schedule claude to check the weather every morning\"\n\
                     - \"list my schedules\"\n\
                     - \"delete schedule 3\""
                        .to_string(),
                ));
            }
        };

        let action = value["action"].as_str().unwrap_or("list");

        match action {
            "add" => {
                let cron = value["cron"].as_str().unwrap_or("@daily");
                let tool = value["tool"].as_str().unwrap_or("claude");
                let prompt = value["prompt"].as_str().unwrap_or("");

                if prompt.is_empty() {
                    return Ok(SkillOutput::Reply(
                        "I need a prompt to schedule. What should the tool do?".to_string(),
                    ));
                }

                let expanded = expand_preset(cron);
                let mut sched = scheduler.write().await;
                match sched.add_job(
                    &expanded,
                    tool,
                    prompt,
                    None,
                    &ctx.thread.platform.to_string(),
                    &ctx.thread.id,
                ) {
                    Ok(job) => {
                        let next = job
                            .next_run
                            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "N/A".to_string());
                        Ok(SkillOutput::Reply(format!(
                            "Schedule #{} created!\nName: `{}`\nTool: {}\nPrompt: {}\nCron: `{}`\nNext run: {}",
                            job.id, job.name, tool, prompt, job.cron_expr, next
                        )))
                    }
                    Err(e) => Ok(SkillOutput::Reply(format!(
                        "Failed to create schedule: {}",
                        e
                    ))),
                }
            }
            "list" => {
                let sched = scheduler.read().await;
                let jobs = sched.list_jobs();
                if jobs.is_empty() {
                    Ok(SkillOutput::Reply(
                        "No scheduled tasks yet. Tell me to schedule something, like:\n\
                         \"schedule claude to check the weather every morning\""
                            .to_string(),
                    ))
                } else {
                    let mut lines = vec!["*Your Scheduled Tasks:*".to_string()];
                    for job in jobs {
                        let next = job
                            .next_run
                            .map(|t| t.format("%m-%d %H:%M").to_string())
                            .unwrap_or_else(|| "-".to_string());
                        lines.push(format!(
                            "  #{} `{}` [{}] {} (next: {})",
                            job.id, job.name, job.status, job.tool, next
                        ));
                    }
                    Ok(SkillOutput::Reply(lines.join("\n")))
                }
            }
            "delete" => {
                let id = value["id"].as_u64().unwrap_or(0);
                if id == 0 {
                    return Ok(SkillOutput::Reply(
                        "Which schedule should I delete? Give me the schedule number.".to_string(),
                    ));
                }
                let mut sched = scheduler.write().await;
                match sched.delete_job(id) {
                    Ok(()) => Ok(SkillOutput::Reply(format!("Schedule #{} deleted.", id))),
                    Err(e) => Ok(SkillOutput::Reply(format!("Failed: {}", e))),
                }
            }
            "pause" => {
                let id = value["id"].as_u64().unwrap_or(0);
                if id == 0 {
                    return Ok(SkillOutput::Reply(
                        "Which schedule should I pause?".to_string(),
                    ));
                }
                let mut sched = scheduler.write().await;
                match sched.pause_job(id) {
                    Ok(()) => Ok(SkillOutput::Reply(format!("Schedule #{} paused.", id))),
                    Err(e) => Ok(SkillOutput::Reply(format!("Failed: {}", e))),
                }
            }
            "resume" => {
                let id = value["id"].as_u64().unwrap_or(0);
                if id == 0 {
                    return Ok(SkillOutput::Reply(
                        "Which schedule should I resume?".to_string(),
                    ));
                }
                let mut sched = scheduler.write().await;
                match sched.resume_job(id) {
                    Ok(()) => Ok(SkillOutput::Reply(format!("Schedule #{} resumed.", id))),
                    Err(e) => Ok(SkillOutput::Reply(format!("Failed: {}", e))),
                }
            }
            "run" => {
                let id = value["id"].as_u64().unwrap_or(0);
                if id == 0 {
                    return Ok(SkillOutput::Reply(
                        "Which schedule should I trigger now?".to_string(),
                    ));
                }
                let mut sched = scheduler.write().await;
                match sched.trigger_now(id) {
                    Ok(()) => Ok(SkillOutput::Reply(format!(
                        "Schedule #{} triggered! It will run shortly.",
                        id
                    ))),
                    Err(e) => Ok(SkillOutput::Reply(format!("Failed: {}", e))),
                }
            }
            "info" => {
                let id = value["id"].as_u64().unwrap_or(0);
                if id == 0 {
                    return Ok(SkillOutput::Reply(
                        "Which schedule would you like details about?".to_string(),
                    ));
                }
                let sched = scheduler.read().await;
                match sched.get_job(id) {
                    Some(job) => {
                        let next = job
                            .next_run
                            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "N/A".to_string());
                        let last = job
                            .last_run
                            .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                            .unwrap_or_else(|| "never".to_string());
                        Ok(SkillOutput::Reply(format!(
                            "*Schedule #{}:*\nName: `{}`\nStatus: {}\nTool: {}\nPrompt: {}\nCron: `{}`\nNext run: {}\nLast run: {}\nRun count: {}",
                            job.id, job.name, job.status, job.tool, job.prompt,
                            job.cron_expr, next, last, job.run_count
                        )))
                    }
                    None => Ok(SkillOutput::Reply(format!(
                        "Schedule #{} not found.",
                        id
                    ))),
                }
            }
            _ => Ok(SkillOutput::Reply(
                "I can help you schedule tasks. Try:\n\
                 - \"schedule claude to check the weather every morning\"\n\
                 - \"list my schedules\"\n\
                 - \"delete schedule 3\""
                    .to_string(),
            )),
        }
    }
}

fn extract_json(response: &str) -> &str {
    if let Some(start) = response.find('{') {
        if let Some(end) = response.rfind('}') {
            return &response[start..=end];
        }
    }
    response
}
