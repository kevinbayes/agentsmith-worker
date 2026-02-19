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
    /// Regular text to route to the active session
    Text(String),
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
        "/help" => Command::Help,
        "/status" => Command::Status,
        _ => Command::Text(trimmed.to_string()),
    }
}

pub fn help_text() -> &'static str {
    r#"*AgentSmith Remote Worker Commands:*
`/new claude` - Start a new Claude Code session
`/new gemini` - Start a new Gemini CLI session
`/new goose` - Start a new Goose session
`/list` - List all active sessions
`/switch <id>` - Switch to a different session
`/stop <id>` - Stop a specific session
`/stop all` - Stop all sessions
`/status` - Show daemon status
`/help` - Show this help message

Any other text is sent to your active session."#
}
