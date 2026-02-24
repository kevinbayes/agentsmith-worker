# Reporter API Contract

The Reporter sends periodic state reports (heartbeats) from the worker daemon to a control-center server. This document defines the HTTP interface contract, authentication, payload schema, and sample payloads.

---

## Endpoint

**Method:** `POST`

**URL:** Configured via `reporter.endpoint` in TOML or `AGENTSMITH_REPORTER_ENDPOINT` env var.

Example: `https://control.example.com/api/v1/workers/state`

---

## Authentication

The reporter uses the **OpenID Connect client credentials grant** to obtain a bearer token from a Keycloak (or compatible OIDC) token endpoint, then sends that token with each state report.

### Token Acquisition

The worker POSTs to the configured `token_url` with `grant_type=client_credentials`:

```
POST /realms/myrealm/protocol/openid-connect/token HTTP/1.1
Host: keycloak.example.com
Content-Type: application/x-www-form-urlencoded

grant_type=client_credentials&client_id=worker-01&client_secret=s3cret
```

**Sample token response:**

```json
{
  "access_token": "eyJhbGciOiJSUzI1NiIsInR5cCI6IkpXVCJ9...",
  "expires_in": 300,
  "token_type": "Bearer"
}
```

### Token Caching

- The access token is cached in memory and reused across report cycles.
- A new token is fetched when the cached token is within **30 seconds** of expiry, or when no token has been fetched yet.
- If the token fetch fails, the report cycle is skipped and a warning is logged.

### State Report Authorization

Each state report is sent with the cached bearer token:

```
Authorization: Bearer <access_token>
```

### Configuration

| Config field             | Env var                              | Role                      |
|--------------------------|--------------------------------------|---------------------------|
| `reporter.token_url`    | `AGENTSMITH_REPORTER_TOKEN_URL`      | OIDC token endpoint URL   |
| `reporter.client_id`    | `AGENTSMITH_REPORTER_CLIENT_ID`      | OIDC client ID            |
| `reporter.client_secret`| `AGENTSMITH_REPORTER_CLIENT_SECRET`  | OIDC client secret        |

---

## Request

| Property       | Value                |
|----------------|----------------------|
| Content-Type   | `application/json`   |
| Authorization  | `Bearer <access_token>` |
| Timeout        | 10 seconds           |
| Delivery       | Fire-and-forget (non-blocking; failures are logged but not retried) |

---

## Report Interval

Configured via `reporter.interval_secs` (default: **30 seconds**). The first report fires immediately on startup.

---

## Payload Schema

```
StateReport
  worker: WorkerIdentity
  agents: AgentReport[]
  sessions: SessionReport[]
  timestamp: string (ISO 8601, UTC)
```

### WorkerIdentity

| Field         | Type   | Description                                                        |
|---------------|--------|--------------------------------------------------------------------|
| `hostname`    | string | OS hostname (`sysinfo::System::host_name()`, fallback `"unknown"`) |
| `worker_name` | string | From `reporter.worker_name` config, defaults to `hostname`         |
| `version`     | string | Crate version from `Cargo.toml` (e.g. `"0.1.0"`)                  |

### AgentReport

One entry per monitored tool. Currently: Claude Code, Gemini CLI, Goose, ZeroClaw, OpenClaw.

| Field               | Type            | Description                                                 |
|----------------------|-----------------|-------------------------------------------------------------|
| `name`              | string          | Human-readable tool name (e.g. `"Claude Code"`)             |
| `binary`            | string          | Binary name or path used for `which` lookup (e.g. `"claude"`) |
| `installed`         | boolean         | `true` if the binary is found on `$PATH`                    |
| `version`           | string \| null  | First non-empty line of `<binary> --version` output, or null |
| `path`              | string \| null  | Absolute path to the binary if installed, or null            |
| `running_instances` | ProcessReport[] | OS processes matching this tool (may be empty)               |

### ProcessReport

| Field         | Type   | Description                                           |
|---------------|--------|-------------------------------------------------------|
| `pid`         | u32    | OS process ID                                         |
| `name`        | string | Process name as reported by the OS                    |
| `cmd_summary` | string | Command line (truncated to 120 chars with `...` suffix) |

### SessionReport

One entry per active agentsmith session.

| Field    | Type   | Description                                                    |
|----------|--------|----------------------------------------------------------------|
| `id`     | u64    | Session ID (monotonically increasing, starts at 1)             |
| `agent`  | string | Tool display name: `"Claude"`, `"Gemini"`, `"Goose"`, `"ZeroClaw"` |
| `status` | string | One of: `"running"`, `"idle"`, `"awaiting input"`, `"stopped"` |

---

## Sample Payloads

### Worker with no sessions and partial tool installations

```json
{
  "worker": {
    "hostname": "dev-machine",
    "worker_name": "dev-machine",
    "version": "0.1.0"
  },
  "agents": [
    {
      "name": "Claude Code",
      "binary": "claude",
      "installed": true,
      "version": "Claude Code v1.0.25",
      "path": "/home/user/.npm/bin/claude",
      "running_instances": []
    },
    {
      "name": "Gemini CLI",
      "binary": "gemini",
      "installed": true,
      "version": "Gemini CLI v0.1.0",
      "path": "/usr/local/bin/gemini",
      "running_instances": []
    },
    {
      "name": "Goose",
      "binary": "goose",
      "installed": false,
      "version": null,
      "path": null,
      "running_instances": []
    },
    {
      "name": "ZeroClaw",
      "binary": "zeroclaw",
      "installed": false,
      "version": null,
      "path": null,
      "running_instances": []
    },
    {
      "name": "OpenClaw",
      "binary": "openclaw",
      "installed": false,
      "version": null,
      "path": null,
      "running_instances": []
    }
  ],
  "sessions": [],
  "timestamp": "2026-02-21T14:30:00.123456Z"
}
```

### Worker with active sessions and running processes

```json
{
  "worker": {
    "hostname": "build-server-1",
    "worker_name": "primary-worker",
    "version": "0.1.0"
  },
  "agents": [
    {
      "name": "Claude Code",
      "binary": "claude",
      "installed": true,
      "version": "Claude Code v1.0.25",
      "path": "/home/user/.npm/bin/claude",
      "running_instances": [
        {
          "pid": 48210,
          "name": "claude",
          "cmd_summary": "claude --dangerously-skip-permissions"
        }
      ]
    },
    {
      "name": "Gemini CLI",
      "binary": "gemini",
      "installed": true,
      "version": "Gemini CLI v0.1.0",
      "path": "/usr/local/bin/gemini",
      "running_instances": []
    },
    {
      "name": "Goose",
      "binary": "goose",
      "installed": true,
      "version": "goose 1.0.16",
      "path": "/home/user/.local/bin/goose",
      "running_instances": [
        {
          "pid": 49001,
          "name": "goose",
          "cmd_summary": "goose run -n agentsmith-2 -r -t fix the login bug"
        }
      ]
    },
    {
      "name": "ZeroClaw",
      "binary": "zeroclaw",
      "installed": true,
      "version": "zeroclaw 0.3.1",
      "path": "/usr/local/bin/zeroclaw",
      "running_instances": [
        {
          "pid": 49500,
          "name": "zeroclaw",
          "cmd_summary": "zeroclaw agent"
        }
      ]
    },
    {
      "name": "OpenClaw",
      "binary": "openclaw",
      "installed": true,
      "version": "1.2.3",
      "path": "/usr/local/bin/openclaw",
      "running_instances": [
        {
          "pid": 50100,
          "name": "node",
          "cmd_summary": "/usr/bin/node /usr/local/lib/node_modules/openclaw/dist/index.js agent --model gpt-4o"
        }
      ]
    }
  ],
  "sessions": [
    {
      "id": 1,
      "agent": "Claude",
      "status": "running"
    },
    {
      "id": 2,
      "agent": "Goose",
      "status": "idle"
    },
    {
      "id": 3,
      "agent": "ZeroClaw",
      "status": "awaiting input"
    }
  ],
  "timestamp": "2026-02-21T14:35:22.987654Z"
}
```

### Minimal worker (custom name, no tools installed)

```json
{
  "worker": {
    "hostname": "unknown",
    "worker_name": "edge-worker-03",
    "version": "0.1.0"
  },
  "agents": [
    {
      "name": "Claude Code",
      "binary": "claude",
      "installed": false,
      "version": null,
      "path": null,
      "running_instances": []
    },
    {
      "name": "Gemini CLI",
      "binary": "gemini",
      "installed": false,
      "version": null,
      "path": null,
      "running_instances": []
    },
    {
      "name": "Goose",
      "binary": "goose",
      "installed": false,
      "version": null,
      "path": null,
      "running_instances": []
    },
    {
      "name": "ZeroClaw",
      "binary": "zeroclaw",
      "installed": false,
      "version": null,
      "path": null,
      "running_instances": []
    },
    {
      "name": "OpenClaw",
      "binary": "openclaw",
      "installed": false,
      "version": null,
      "path": null,
      "running_instances": []
    }
  ],
  "sessions": [],
  "timestamp": "2026-02-21T00:00:01.000000Z"
}
```

---

## Configuration Reference

TOML section:

```toml
[reporter]
enabled = true
endpoint = "https://control.example.com/api/v1/workers/state"
token_url = "https://keycloak.example.com/realms/myrealm/protocol/openid-connect/token"
client_id = "worker-01"
client_secret = "s3cret"
worker_name = "primary-worker"   # optional, defaults to hostname
interval_secs = 30               # optional, default 30
```

Env var overrides:

| Env var                              | Overrides               |
|--------------------------------------|-------------------------|
| `AGENTSMITH_REPORTER_ENABLED`        | `reporter.enabled`      |
| `AGENTSMITH_REPORTER_ENDPOINT`       | `reporter.endpoint`     |
| `AGENTSMITH_REPORTER_TOKEN_URL`      | `reporter.token_url`    |
| `AGENTSMITH_REPORTER_CLIENT_ID`      | `reporter.client_id`    |
| `AGENTSMITH_REPORTER_CLIENT_SECRET`  | `reporter.client_secret`|
| `AGENTSMITH_REPORTER_WORKER_NAME`    | `reporter.worker_name`  |
| `AGENTSMITH_REPORTER_INTERVAL_SECS`  | `reporter.interval_secs`|

---

## Server Implementation Notes

- The server should accept `POST` with `Content-Type: application/json` and validate the `Authorization: Bearer <token>` header against the same Keycloak realm.
- Respond with `2xx` on success. Any non-2xx status is logged as a warning by the worker but not retried.
- The `timestamp` field uses the worker's clock (UTC). Servers may also record their own receive time.
- The `agents` array is always present and always contains all monitored tools, even if none are installed.
- The `sessions` array is empty when no sessions are active.
- Reports arrive at roughly the configured interval but are not guaranteed to be exactly periodic (the worker checks elapsed time on each router poll cycle, ~50ms).
