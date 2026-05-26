#!/usr/bin/env python3
"""
AgentSmith Scheduler MCP Server

Standalone MCP (Model Context Protocol) server that exposes AgentSmith's
schedule management REST API as tools for Claude Code.

Protocol: JSON-RPC 2.0 over stdio (one JSON object per line).
Dependencies: Python 3 stdlib only (json, os, sys, urllib.request).
"""

import json
import logging
import os
import sys
import urllib.request
import urllib.error

BASE_URL = os.environ.get("AGENTSMITH_URL", "http://127.0.0.1:3000")

logging.basicConfig(
    stream=sys.stderr,
    level=logging.DEBUG,
    format="[agentsmith-mcp] %(message)s",
)
log = logging.getLogger(__name__)

# ---------------------------------------------------------------------------
# HTTP helper
# ---------------------------------------------------------------------------

def api_request(method, path, body=None):
    """Make an HTTP request to the AgentSmith REST API."""
    url = BASE_URL.rstrip("/") + path
    data = json.dumps(body).encode("utf-8") if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("Content-Type", "application/json")
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        error_body = e.read().decode("utf-8", errors="replace")
        try:
            return json.loads(error_body)
        except Exception:
            return {"error": f"HTTP {e.code}: {error_body}"}
    except urllib.error.URLError as e:
        return {"error": f"Connection failed: {e.reason}"}

# ---------------------------------------------------------------------------
# Tool implementations
# ---------------------------------------------------------------------------

def list_schedules(_args):
    return api_request("GET", "/api/schedules")

def create_schedule(args):
    body = {
        "cron": args.get("cron", ""),
        "tool": args.get("tool", ""),
        "prompt": args.get("prompt", ""),
    }
    name = args.get("name")
    if name:
        body["name"] = name
    return api_request("POST", "/api/schedules", body)

def get_schedule(args):
    schedule_id = args.get("id")
    if schedule_id is None:
        return {"error": "Missing required parameter: id"}
    return api_request("GET", f"/api/schedules/{schedule_id}")

def delete_schedule(args):
    schedule_id = args.get("id")
    if schedule_id is None:
        return {"error": "Missing required parameter: id"}
    return api_request("DELETE", f"/api/schedules/{schedule_id}")

def pause_schedule(args):
    schedule_id = args.get("id")
    if schedule_id is None:
        return {"error": "Missing required parameter: id"}
    return api_request("POST", f"/api/schedules/{schedule_id}/pause")

def resume_schedule(args):
    schedule_id = args.get("id")
    if schedule_id is None:
        return {"error": "Missing required parameter: id"}
    return api_request("POST", f"/api/schedules/{schedule_id}/resume")

def trigger_schedule(args):
    schedule_id = args.get("id")
    if schedule_id is None:
        return {"error": "Missing required parameter: id"}
    return api_request("POST", f"/api/schedules/{schedule_id}/trigger")

# ---------------------------------------------------------------------------
# Tool definitions (MCP schema)
# ---------------------------------------------------------------------------

TOOLS = [
    {
        "name": "list_schedules",
        "description": (
            "List all scheduled tasks in AgentSmith. Returns each schedule's ID, name, "
            "cron expression, tool, prompt, status (active/paused), next run time, "
            "total run count, and consecutive failure count. Use this to find schedule IDs "
            "needed by other schedule management tools."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
    {
        "name": "create_schedule",
        "description": (
            "Create a new scheduled task in AgentSmith. The task will run the specified "
            "AI CLI tool with the given prompt on the cron schedule. "
            "Presets: @daily (8 AM), @hourly, @weekly (Mon 8 AM), @twice-daily (8 AM & 6 PM), "
            "@every-30m, @weekdays (Mon-Fri 8 AM). "
            "Standard 6-field cron: sec min hour day month weekday "
            "(e.g. '0 30 9 * * 1-5' = weekdays at 9:30 AM)."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "cron": {
                    "type": "string",
                    "description": (
                        "Cron expression or preset. "
                        "Presets: @daily, @hourly, @weekly, @twice-daily, @every-30m, @weekdays. "
                        "Or 6-field cron: 'sec min hour day month weekday'."
                    ),
                },
                "tool": {
                    "type": "string",
                    "enum": ["claude", "hermes", "zeroclaw"],
                    "description": "Which AI CLI tool to run.",
                },
                "prompt": {
                    "type": "string",
                    "description": "The prompt/task to send to the tool on each run.",
                },
                "name": {
                    "type": "string",
                    "description": "Optional human-readable name for this schedule.",
                },
            },
            "required": ["cron", "tool", "prompt"],
        },
    },
    {
        "name": "get_schedule",
        "description": (
            "Get full details of a specific schedule by ID, including its run history. "
            "Call list_schedules first to find the schedule ID."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID (from list_schedules).",
                },
            },
            "required": ["id"],
        },
    },
    {
        "name": "delete_schedule",
        "description": (
            "Permanently delete a schedule by ID. This cannot be undone. "
            "Call list_schedules first to find the schedule ID."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID (from list_schedules).",
                },
            },
            "required": ["id"],
        },
    },
    {
        "name": "pause_schedule",
        "description": (
            "Pause an active schedule so it stops running until resumed. "
            "Call list_schedules first to find the schedule ID."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID (from list_schedules).",
                },
            },
            "required": ["id"],
        },
    },
    {
        "name": "resume_schedule",
        "description": (
            "Resume a paused schedule so it starts running again on its cron timing. "
            "Call list_schedules first to find the schedule ID."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID (from list_schedules).",
                },
            },
            "required": ["id"],
        },
    },
    {
        "name": "trigger_schedule",
        "description": (
            "Trigger a schedule to run immediately, regardless of its cron timing or "
            "paused status. The schedule's next scheduled run is not affected. "
            "Call list_schedules first to find the schedule ID."
        ),
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID (from list_schedules).",
                },
            },
            "required": ["id"],
        },
    },
]

TOOL_DISPATCH = {
    "list_schedules": list_schedules,
    "create_schedule": create_schedule,
    "get_schedule": get_schedule,
    "delete_schedule": delete_schedule,
    "pause_schedule": pause_schedule,
    "resume_schedule": resume_schedule,
    "trigger_schedule": trigger_schedule,
}

# ---------------------------------------------------------------------------
# MCP JSON-RPC handling
# ---------------------------------------------------------------------------

def handle_request(msg):
    """Handle a single JSON-RPC request. Returns a response dict or None for notifications."""
    method = msg.get("method", "")
    msg_id = msg.get("id")
    params = msg.get("params", {})

    if method == "initialize":
        log.info("initialize received (protocol %s)", params.get("protocolVersion", "?"))
        return {
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {
                    "name": "agentsmith-scheduler",
                    "version": "1.0.0",
                },
                "instructions": (
                    "AgentSmith Scheduler: manage scheduled/recurring tasks. "
                    "Use these tools when the user asks about scheduling, cron jobs, "
                    "recurring tasks, or listing/managing scheduled work."
                ),
            },
        }

    if method == "notifications/initialized":
        log.info("client initialized")
        return None  # Notification — no response

    if method == "ping":
        return {"jsonrpc": "2.0", "id": msg_id, "result": {}}

    if method in ("resources/list", "resources/templates/list"):
        return {"jsonrpc": "2.0", "id": msg_id, "result": {"resources": []}}

    if method == "prompts/list":
        return {"jsonrpc": "2.0", "id": msg_id, "result": {"prompts": []}}

    if method == "tools/list":
        log.info("tools/list requested")
        return {
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {"tools": TOOLS},
        }

    if method == "tools/call":
        tool_name = params.get("name", "")
        arguments = params.get("arguments", {})
        log.info("tools/call: %s(%s)", tool_name, json.dumps(arguments))

        handler = TOOL_DISPATCH.get(tool_name)
        if handler is None:
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {
                    "content": [{"type": "text", "text": f"Unknown tool: {tool_name}"}],
                    "isError": True,
                },
            }

        try:
            result = handler(arguments)
            text = json.dumps(result, indent=2)
            is_error = isinstance(result, dict) and "error" in result
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {
                    "content": [{"type": "text", "text": text}],
                    "isError": is_error,
                },
            }
        except Exception as e:
            log.error("tools/call %s failed: %s", tool_name, e)
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {
                    "content": [{"type": "text", "text": f"Error: {e}"}],
                    "isError": True,
                },
            }

    # Unknown method
    log.warning("unknown method: %s", method)
    if msg_id is not None:
        return {
            "jsonrpc": "2.0",
            "id": msg_id,
            "error": {
                "code": -32601,
                "message": f"Method not found: {method}",
            },
        }

    return None

# ---------------------------------------------------------------------------
# Main loop
# ---------------------------------------------------------------------------

def main():
    log.info("starting (AGENTSMITH_URL=%s)", BASE_URL)
    while True:
        line = sys.stdin.readline()
        if not line:
            log.info("stdin closed, exiting")
            break  # EOF
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError as e:
            log.warning("invalid JSON: %s", e)
            continue

        response = handle_request(msg)
        if response is not None:
            sys.stdout.write(json.dumps(response) + "\n")
            sys.stdout.flush()

if __name__ == "__main__":
    main()
