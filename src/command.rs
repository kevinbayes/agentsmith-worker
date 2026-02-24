/// Parsed user command.
#[derive(Debug, Clone)]
pub enum Command {
    /// Create a new AI session: /new claude, /new gemini
    New { tool: String },
    /// List all sessions: /list
    List,
    /// Switch active session: /switch <id>
    Switch { session_id: u64 },
    /// Stop a session: /stop <id> or /stop all
    Stop { target: StopTarget },
    /// Show help text: /help
    Help,
    /// Show daemon status: /status
    Status,
    /// Monitor tool installations and processes: /monitor
    Monitor { action: MonitorAction },
    /// Enter agent mode: /agent
    Agent,
    /// Return to session mode: /session
    Session,
    /// Alias for /session: /back
    Back,
    /// Clear agent conversation context: /clear
    Clear,
    /// Schedule management commands: /schedule
    Schedule { action: ScheduleAction },
    /// Regular text to route to the active session
    Text(String),
}

#[derive(Debug, Clone)]
pub enum MonitorAction {
    /// Show tool installation and process status
    Status,
    /// Kill processes for a specific tool
    Kill { tool: String },
}

#[derive(Debug, Clone)]
pub enum ScheduleAction {
    /// Add a new schedule: /schedule add "0 8 * * *" claude check the weather
    Add {
        cron_expr: String,
        tool: String,
        prompt: String,
    },
    /// List all schedules: /schedule list
    List,
    /// Delete a schedule: /schedule delete 1
    Delete { id: u64 },
    /// Pause a schedule: /schedule pause 1
    Pause { id: u64 },
    /// Resume a schedule: /schedule resume 1
    Resume { id: u64 },
    /// Trigger a schedule now: /schedule run 1
    Run { id: u64 },
    /// Show schedule details: /schedule info 1
    Info { id: u64 },
    /// Install MCP server for a CLI agent: /schedule install-tool claude
    InstallTool { agent: String },
}

#[derive(Debug, Clone)]
pub enum StopTarget {
    Session(u64),
    All,
}

/// Parse a raw message into a Command.
pub fn parse_command(input: &str) -> Command {
    let trimmed = input.trim();

    if !trimmed.starts_with('/') {
        return Command::Text(trimmed.to_string());
    }

    // Use splitn(2, ' ') for the command word, then handle the rest per-command
    let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
    let cmd = parts[0].to_lowercase();

    match cmd.as_str() {
        "/new" => {
            let tool = parts
                .get(1)
                .map(|s| s.to_lowercase())
                .unwrap_or_else(|| "claude".to_string());
            Command::New { tool }
        }
        "/list" => Command::List,
        "/switch" => {
            let session_id = parts
                .get(1)
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);
            Command::Switch { session_id }
        }
        "/stop" => {
            let target = match parts.get(1).map(|s| s.to_lowercase()).as_deref() {
                Some("all") => StopTarget::All,
                Some(id) => {
                    if let Ok(n) = id.parse::<u64>() {
                        StopTarget::Session(n)
                    } else {
                        StopTarget::All
                    }
                }
                None => StopTarget::All,
            };
            Command::Stop { target }
        }
        "/agent" => Command::Agent,
        "/session" => Command::Session,
        "/back" => Command::Back,
        "/clear" => Command::Clear,
        "/help" => Command::Help,
        "/status" => Command::Status,
        "/monitor" => {
            match parts.get(1).map(|s| s.to_lowercase()).as_deref() {
                Some("kill") => {
                    let tool = parts
                        .get(2)
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();
                    Command::Monitor {
                        action: MonitorAction::Kill { tool },
                    }
                }
                _ => Command::Monitor {
                    action: MonitorAction::Status,
                },
            }
        }
        "/schedule" | "/sched" | "/cron" => parse_schedule_command(trimmed),
        _ => Command::Text(trimmed.to_string()),
    }
}

fn parse_schedule_command(input: &str) -> Command {
    // Split into: /schedule <subcommand> <rest>
    let after_cmd = input.splitn(2, ' ').nth(1).unwrap_or("").trim();

    if after_cmd.is_empty() {
        return Command::Schedule {
            action: ScheduleAction::List,
        };
    }

    let (sub, rest) = match after_cmd.splitn(2, ' ').collect::<Vec<_>>().as_slice() {
        [sub, rest] => (sub.to_lowercase(), rest.trim().to_string()),
        [sub] => (sub.to_lowercase(), String::new()),
        _ => return Command::Schedule {
            action: ScheduleAction::List,
        },
    };

    match sub.as_str() {
        "add" | "create" => {
            match parse_schedule_add(&rest) {
                Some(action) => Command::Schedule { action },
                None => Command::Text(format!(
                    "Usage: /schedule add \"<cron>\" <tool> <prompt>\n\
                     Example: /schedule add \"0 8 * * *\" claude check the weather\n\
                     Or: /schedule add @daily claude check the weather"
                )),
            }
        }
        "list" | "ls" => Command::Schedule {
            action: ScheduleAction::List,
        },
        "delete" | "rm" | "remove" => {
            match rest.parse::<u64>() {
                Ok(id) => Command::Schedule {
                    action: ScheduleAction::Delete { id },
                },
                Err(_) => Command::Text("Usage: /schedule delete <id>".to_string()),
            }
        }
        "pause" => {
            match rest.parse::<u64>() {
                Ok(id) => Command::Schedule {
                    action: ScheduleAction::Pause { id },
                },
                Err(_) => Command::Text("Usage: /schedule pause <id>".to_string()),
            }
        }
        "resume" => {
            match rest.parse::<u64>() {
                Ok(id) => Command::Schedule {
                    action: ScheduleAction::Resume { id },
                },
                Err(_) => Command::Text("Usage: /schedule resume <id>".to_string()),
            }
        }
        "run" | "trigger" => {
            match rest.parse::<u64>() {
                Ok(id) => Command::Schedule {
                    action: ScheduleAction::Run { id },
                },
                Err(_) => Command::Text("Usage: /schedule run <id>".to_string()),
            }
        }
        "info" | "show" => {
            match rest.parse::<u64>() {
                Ok(id) => Command::Schedule {
                    action: ScheduleAction::Info { id },
                },
                Err(_) => Command::Text("Usage: /schedule info <id>".to_string()),
            }
        }
        "install-tool" | "install" => {
            let agent = rest.trim().to_lowercase();
            if agent.is_empty() {
                Command::Text(
                    "Usage: /schedule install-tool <agent>\nSupported agents: claude, gemini"
                        .to_string(),
                )
            } else {
                Command::Schedule {
                    action: ScheduleAction::InstallTool { agent },
                }
            }
        }
        _ => Command::Schedule {
            action: ScheduleAction::List,
        },
    }
}

/// Parse the "add" subcommand arguments.
/// Formats:
///   /schedule add "0 8 * * *" claude check the weather
///   /schedule add @daily claude check the weather
fn parse_schedule_add(input: &str) -> Option<ScheduleAction> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }

    let (cron_expr, remainder) = if input.starts_with('"') {
        // Quoted cron expression
        let end_quote = input[1..].find('"')?;
        let expr = input[1..=end_quote].to_string();
        let rest = input[end_quote + 2..].trim().to_string();
        (expr, rest)
    } else if input.starts_with('@') {
        // Preset alias
        let space = input.find(' ')?;
        let alias = input[..space].to_string();
        let rest = input[space + 1..].trim().to_string();
        (alias, rest)
    } else {
        return None;
    };

    // remainder should be: <tool> <prompt...>
    let (tool, prompt) = match remainder.splitn(2, ' ').collect::<Vec<_>>().as_slice() {
        [tool, prompt] => (tool.to_lowercase(), prompt.trim().to_string()),
        _ => return None,
    };

    if prompt.is_empty() {
        return None;
    }

    Some(ScheduleAction::Add {
        cron_expr,
        tool,
        prompt,
    })
}

pub fn help_text() -> &'static str {
    r#"*AgentSmith Remote Worker Commands:*
`/new claude` - Start a new Claude Code session
`/new gemini` - Start a new Gemini CLI session
`/new goose` - Start a new Goose session
`/new zeroclaw` - Start a new ZeroClaw session
`/list` - List all active sessions
`/switch <id>` - Switch to a different session
`/stop <id>` - Stop a specific session
`/stop all` - Stop all sessions
`/status` - Show daemon status
`/monitor` - Show tool installation & running processes
`/monitor kill openclaw` - Kill OpenClaw processes
`/agent` - Enter agent mode (AI assistant)
`/session` - Return to session mode (direct passthrough)
`/back` - Alias for /session
`/clear` - Clear agent conversation context

*Scheduled Tasks:* (aliases: `/sched`, `/cron`)
`/schedule list` - List all scheduled tasks
`/schedule add "<cron>" <tool> <prompt>` - Add a schedule
`/schedule add @daily claude check the weather` - Add with preset
`/schedule delete <id>` - Delete a schedule
`/schedule pause <id>` - Pause a schedule
`/schedule resume <id>` - Resume a paused schedule
`/schedule run <id>` - Trigger a schedule immediately
`/schedule info <id>` - Show schedule details
`/schedule install-tool <agent>` - Install MCP server (claude, gemini)
Presets: `@daily`, `@hourly`, `@weekly`, `@twice-daily`, `@every-30m`, `@weekdays`

`/help` - Show this help message

In *session mode* (default), text goes directly to your active CLI session.
In *agent mode*, text goes to the AI assistant which can chat, delegate tasks, check status, and more."#
}
