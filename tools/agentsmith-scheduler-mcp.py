#!/usr/bin/env python3
"""
AgentSmith Scheduler MCP Server

Standalone MCP (Model Context Protocol) server that exposes AgentSmith's
schedule management REST API as tools for Claude Code and Gemini CLI.

Protocol: JSON-RPC 2.0 over stdio (one JSON object per line).
Dependencies: Python 3 stdlib only (json, os, sys, urllib.request).
"""

import json
import os
import sys
import urllib.request
import urllib.error

BASE_URL = os.environ.get("AGENTSMITH_URL", "http://127.0.0.1:3000")

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
        "description": "List all scheduled tasks in AgentSmith.",
        "inputSchema": {
            "type": "object",
            "properties": {},
            "required": [],
        },
    },
    {
        "name": "create_schedule",
        "description": "Create a new scheduled task. Requires a cron expression, tool name, and prompt.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "cron": {
                    "type": "string",
                    "description": 'Cron expression (e.g. "0 8 * * *") or preset (@daily, @hourly, @weekly).',
                },
                "tool": {
                    "type": "string",
                    "description": "CLI tool to run (claude, gemini, goose, zeroclaw).",
                },
                "prompt": {
                    "type": "string",
                    "description": "The prompt/task to send to the tool.",
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
        "description": "Get details of a specific schedule by ID.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID.",
                },
            },
            "required": ["id"],
        },
    },
    {
        "name": "delete_schedule",
        "description": "Delete a schedule by ID.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID.",
                },
            },
            "required": ["id"],
        },
    },
    {
        "name": "pause_schedule",
        "description": "Pause a schedule so it stops running until resumed.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID.",
                },
            },
            "required": ["id"],
        },
    },
    {
        "name": "resume_schedule",
        "description": "Resume a paused schedule.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID.",
                },
            },
            "required": ["id"],
        },
    },
    {
        "name": "trigger_schedule",
        "description": "Trigger a schedule to run immediately, regardless of its cron timing.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": {
                    "type": "integer",
                    "description": "Schedule ID.",
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
            },
        }

    if method == "notifications/initialized":
        return None  # Notification — no response

    if method == "tools/list":
        return {
            "jsonrpc": "2.0",
            "id": msg_id,
            "result": {"tools": TOOLS},
        }

    if method == "tools/call":
        tool_name = params.get("name", "")
        arguments = params.get("arguments", {})

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
            return {
                "jsonrpc": "2.0",
                "id": msg_id,
                "result": {
                    "content": [{"type": "text", "text": f"Error: {e}"}],
                    "isError": True,
                },
            }

    # Unknown method
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
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue

        response = handle_request(msg)
        if response is not None:
            sys.stdout.write(json.dumps(response) + "\n")
            sys.stdout.flush()

if __name__ == "__main__":
    main()
