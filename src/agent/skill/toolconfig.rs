use std::path::PathBuf;

use super::{SkillContext, SkillOutput};

/// Manage MCP servers, custom commands, and permissions for CLI tools.
pub struct ToolConfigSkill;

impl ToolConfigSkill {
    pub async fn execute(
        &self,
        input: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        // Use LLM to interpret the configuration request
        let system = r#"You are a tool configuration parser for AgentSmith. Given the user's request about CLI tool configuration, determine the action.

Supported operations:
1. List MCP servers for Claude Code (reads ~/.claude/settings.json)
2. Add/install an MCP server for Claude Code
3. Remove an MCP server from Claude Code
4. List Claude Code permissions
5. Show Claude Code settings

Respond with ONLY a JSON object (no markdown fencing):
{"action": "list_mcp|add_mcp|remove_mcp|list_permissions|show_settings", "tool": "claude", "details": "<relevant details like server name>"}"#;

        let parsed = ctx.llm.generate(system, input).await?;

        // Parse LLM response
        let action = parse_action(&parsed);

        match action {
            ConfigAction::ListMcp { tool } => self.list_mcp_servers(&tool).await,
            ConfigAction::AddMcp { tool, details } => {
                self.add_mcp_server(&tool, &details, ctx).await
            }
            ConfigAction::RemoveMcp { tool, name } => self.remove_mcp_server(&tool, &name).await,
            ConfigAction::ListPermissions { tool } => self.list_permissions(&tool).await,
            ConfigAction::ShowSettings { tool } => self.show_settings(&tool).await,
            ConfigAction::Unknown => Ok(SkillOutput::Reply(
                "I couldn't understand that configuration request. Try:\n\
                 - \"list claude mcp servers\"\n\
                 - \"show claude settings\"\n\
                 - \"list claude permissions\""
                    .to_string(),
            )),
        }
    }

    async fn list_mcp_servers(&self, tool: &str) -> anyhow::Result<SkillOutput> {
        let settings_path = match tool {
            "claude" => claude_settings_path(),
            _ => {
                return Ok(SkillOutput::Reply(format!(
                    "MCP server management is currently only supported for Claude Code, not '{}'.",
                    tool
                )));
            }
        };

        match std::fs::read_to_string(&settings_path) {
            Ok(content) => {
                let value: serde_json::Value =
                    serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);
                let servers = value.get("mcpServers").cloned().unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

                if let Some(obj) = servers.as_object() {
                    if obj.is_empty() {
                        return Ok(SkillOutput::Reply(
                            "No MCP servers configured for Claude Code.".to_string(),
                        ));
                    }
                    let mut lines = vec!["**Claude Code MCP Servers:**".to_string()];
                    for (name, config) in obj {
                        let cmd = config
                            .get("command")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown");
                        let disabled = config
                            .get("disabled")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        let status = if disabled { " (disabled)" } else { "" };
                        lines.push(format!("  - **{}**: `{}`{}", name, cmd, status));
                    }
                    Ok(SkillOutput::Reply(lines.join("\n")))
                } else {
                    Ok(SkillOutput::Reply(
                        "No MCP servers configured for Claude Code.".to_string(),
                    ))
                }
            }
            Err(_) => Ok(SkillOutput::Reply(format!(
                "Could not read Claude Code settings at {}",
                settings_path.display()
            ))),
        }
    }

    async fn add_mcp_server(
        &self,
        tool: &str,
        details: &str,
        ctx: &mut SkillContext<'_>,
    ) -> anyhow::Result<SkillOutput> {
        if tool != "claude" {
            return Ok(SkillOutput::Reply(format!(
                "MCP server management is currently only supported for Claude Code, not '{}'.",
                tool
            )));
        }

        // Use LLM to generate the MCP server configuration
        let system = r#"Generate an MCP server configuration entry for Claude Code's settings.json.
The mcpServers format is:
{
  "server_name": {
    "command": "executable",
    "args": ["arg1", "arg2"],
    "env": {}
  }
}

Common MCP servers:
- filesystem: {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem", "/path"]}
- github: {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"]}
- postgres: {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-postgres", "connection_string"]}
- sqlite: {"command": "npx", "args": ["-y", "@modelcontextprotocol/server-sqlite", "db_path"]}

Respond with ONLY a JSON object (no markdown fencing):
{"name": "server_name", "config": {"command": "...", "args": [...], "env": {}}}"#;

        let parsed = ctx.llm.generate(system, details).await?;

        // Parse the generated config
        let json_str = extract_json(&parsed);
        let value: serde_json::Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => {
                return Ok(SkillOutput::Reply(format!(
                    "Could not generate a valid MCP server configuration for: {}",
                    details
                )));
            }
        };

        let name = value
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let config = value.get("config").cloned().unwrap_or(serde_json::Value::Null);

        if config.is_null() {
            return Ok(SkillOutput::Reply(
                "Could not generate a valid MCP server configuration.".to_string(),
            ));
        }

        // Read existing settings
        let settings_path = claude_settings_path();
        let mut settings: serde_json::Value = match std::fs::read_to_string(&settings_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(_) => serde_json::json!({}),
        };

        // Add the MCP server
        if settings.get("mcpServers").is_none() {
            settings["mcpServers"] = serde_json::json!({});
        }
        settings["mcpServers"][name] = config;

        // Write back
        let content = serde_json::to_string_pretty(&settings)?;
        if let Some(parent) = settings_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&settings_path, content)?;

        Ok(SkillOutput::Reply(format!(
            "Added MCP server '{}' to Claude Code settings at {}",
            name,
            settings_path.display()
        )))
    }

    async fn remove_mcp_server(&self, tool: &str, name: &str) -> anyhow::Result<SkillOutput> {
        if tool != "claude" {
            return Ok(SkillOutput::Reply(format!(
                "MCP server management is currently only supported for Claude Code, not '{}'.",
                tool
            )));
        }

        let settings_path = claude_settings_path();
        let mut settings: serde_json::Value = match std::fs::read_to_string(&settings_path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or(serde_json::json!({})),
            Err(_) => {
                return Ok(SkillOutput::Reply(
                    "No Claude Code settings file found.".to_string(),
                ));
            }
        };

        if let Some(servers) = settings.get_mut("mcpServers").and_then(|v| v.as_object_mut()) {
            if servers.remove(name).is_some() {
                let content = serde_json::to_string_pretty(&settings)?;
                std::fs::write(&settings_path, content)?;
                Ok(SkillOutput::Reply(format!(
                    "Removed MCP server '{}' from Claude Code settings.",
                    name
                )))
            } else {
                Ok(SkillOutput::Reply(format!(
                    "MCP server '{}' not found in Claude Code settings.",
                    name
                )))
            }
        } else {
            Ok(SkillOutput::Reply(
                "No MCP servers configured for Claude Code.".to_string(),
            ))
        }
    }

    async fn list_permissions(&self, tool: &str) -> anyhow::Result<SkillOutput> {
        if tool != "claude" {
            return Ok(SkillOutput::Reply(format!(
                "Permission management is currently only supported for Claude Code, not '{}'.",
                tool
            )));
        }

        let settings_path = claude_settings_path();
        match std::fs::read_to_string(&settings_path) {
            Ok(content) => {
                let value: serde_json::Value =
                    serde_json::from_str(&content).unwrap_or(serde_json::Value::Null);

                let mut lines = vec!["**Claude Code Permissions:**".to_string()];

                if let Some(allowed) = value.get("allowedTools").and_then(|v| v.as_array()) {
                    if !allowed.is_empty() {
                        lines.push("Allowed tools:".to_string());
                        for tool in allowed {
                            if let Some(s) = tool.as_str() {
                                lines.push(format!("  - {}", s));
                            }
                        }
                    }
                }

                if let Some(denied) = value.get("deniedTools").and_then(|v| v.as_array()) {
                    if !denied.is_empty() {
                        lines.push("Denied tools:".to_string());
                        for tool in denied {
                            if let Some(s) = tool.as_str() {
                                lines.push(format!("  - {}", s));
                            }
                        }
                    }
                }

                if lines.len() == 1 {
                    lines.push("No explicit permission overrides configured.".to_string());
                }

                Ok(SkillOutput::Reply(lines.join("\n")))
            }
            Err(_) => Ok(SkillOutput::Reply(
                "Could not read Claude Code settings file.".to_string(),
            )),
        }
    }

    async fn show_settings(&self, tool: &str) -> anyhow::Result<SkillOutput> {
        let settings_path = match tool {
            "claude" => claude_settings_path(),
            _ => {
                return Ok(SkillOutput::Reply(format!(
                    "Settings display is currently only supported for Claude Code, not '{}'.",
                    tool
                )));
            }
        };

        match std::fs::read_to_string(&settings_path) {
            Ok(content) => {
                let value: serde_json::Value = serde_json::from_str(&content)
                    .unwrap_or(serde_json::Value::Null);
                let pretty = serde_json::to_string_pretty(&value)
                    .unwrap_or_else(|_| content.clone());
                Ok(SkillOutput::Reply(format!(
                    "**Claude Code Settings** ({})\n```json\n{}\n```",
                    settings_path.display(),
                    pretty
                )))
            }
            Err(_) => Ok(SkillOutput::Reply(format!(
                "No settings file found at {}",
                settings_path.display()
            ))),
        }
    }
}

enum ConfigAction {
    ListMcp { tool: String },
    AddMcp { tool: String, details: String },
    RemoveMcp { tool: String, name: String },
    ListPermissions { tool: String },
    ShowSettings { tool: String },
    Unknown,
}

fn parse_action(response: &str) -> ConfigAction {
    let json_str = extract_json(response);
    let value: serde_json::Value = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return ConfigAction::Unknown,
    };

    let action = value
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let tool = value
        .get("tool")
        .and_then(|v| v.as_str())
        .unwrap_or("claude")
        .to_string();
    let details = value
        .get("details")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    match action {
        "list_mcp" => ConfigAction::ListMcp { tool },
        "add_mcp" => ConfigAction::AddMcp { tool, details },
        "remove_mcp" => ConfigAction::RemoveMcp { tool, name: details },
        "list_permissions" => ConfigAction::ListPermissions { tool },
        "show_settings" => ConfigAction::ShowSettings { tool },
        _ => ConfigAction::Unknown,
    }
}

fn extract_json(s: &str) -> &str {
    if let Some(start) = s.find('{') {
        if let Some(end) = s.rfind('}') {
            return &s[start..=end];
        }
    }
    s
}

fn claude_settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".claude").join("settings.json")
}
