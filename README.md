# AgentSmith Remote Worker

A daemon that bridges messaging platforms (Signal, Slack) with AI CLI tools (Claude Code, Gemini CLI, Goose), allowing you to give tasks remotely and interact with AI sessions via chat messages.

## How It Works

```
You (Signal/Slack) --> AgentSmith Daemon --> Claude Code / Gemini CLI / Goose
                   <-- AI responses    <--
```

Send a message from your phone or Slack workspace, and the daemon routes it to an AI session running on your machine. Responses stream back to the same conversation thread.

## Quick Install

```bash
curl -fsSL https://raw.githubusercontent.com/kevinbayes/agentsmith-worker/main/install.sh | bash
```

Or with a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/kevinbayes/agentsmith-worker/main/install.sh | bash -s -- v0.1.0
```

## Prerequisites

Before running the daemon, install the AI tools you want to use:

- **Claude Code**: https://docs.anthropic.com/en/docs/claude-code/overview
- **Gemini CLI**: https://github.com/google-gemini/gemini-cli
- **Goose**: https://github.com/block/goose

The daemon calls these as external commands (`claude`, `gemini`, and `goose` by default).

## Building from Source

### Linux (Ubuntu/Debian)

```bash
# Install system dependencies
sudo apt-get update
sudo apt-get install -y build-essential pkg-config libsqlite3-dev cmake clang libssl-dev

# Install Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone and build
git clone https://github.com/kevinbayes/agentsmith-worker.git
cd agentsmith-worker
cargo build --release

# The binary is at target/release/agentsmith-remote-worker
sudo cp target/release/agentsmith-remote-worker /usr/local/bin/
```

### Linux (Fedora/RHEL)

```bash
# Install system dependencies
sudo dnf install -y gcc pkg-config sqlite-devel cmake clang openssl-devel

# Install Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone and build
git clone https://github.com/kevinbayes/agentsmith-worker.git
cd agentsmith-worker
cargo build --release

sudo cp target/release/agentsmith-remote-worker /usr/local/bin/
```

### Linux (Arch)

```bash
# Install system dependencies
sudo pacman -S base-devel pkg-config sqlite cmake clang openssl

# Install Rust (if not installed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Clone and build
git clone https://github.com/kevinbayes/agentsmith-worker.git
cd agentsmith-worker
cargo build --release

sudo cp target/release/agentsmith-remote-worker /usr/local/bin/
```

### Windows

```powershell
# Prerequisites:
# 1. Install Visual Studio Build Tools with "Desktop development with C++" workload
#    https://visualstudio.microsoft.com/visual-cpp-build-tools/
# 2. Install Rust: https://rustup.rs
# 3. Install CMake: https://cmake.org/download/ (add to PATH)

# Install vcpkg and sqlite3
git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat
C:\vcpkg\vcpkg install sqlite3:x64-windows

# Set environment variables (add to your profile for persistence)
$env:VCPKG_ROOT = "C:\vcpkg"
$env:SQLITE3_LIB_DIR = "C:\vcpkg\installed\x64-windows\lib"

# Clone and build
git clone https://github.com/kevinbayes/agentsmith-worker.git
cd agentsmith-worker
cargo build --release

# The binary is at target\release\agentsmith-remote-worker.exe
# Copy it somewhere on your PATH, e.g.:
Copy-Item target\release\agentsmith-remote-worker.exe C:\Windows\System32\
```

## Running with Cargo

If you've cloned the repository, you can run the daemon directly with `cargo` without installing:

```bash
# Run in debug mode
cargo run -- --config config.toml

# Run in release mode (recommended for performance)
cargo run --release -- --config config.toml

# Link Signal account
cargo run -- --link-signal

# With environment variable overrides
AGENTSMITH_DAEMON_LOG_LEVEL=debug cargo run --release -- --config config.toml
```

## Configuration

Create a `config.toml` file (default location: current directory):

```toml
[daemon]
working_dir = "/home/user/projects"   # Directory where AI sessions run
log_level = "info"                     # trace, debug, info, warn, error
data_dir = "/home/user/.agentsmith"    # Daemon data storage

[signal]
enabled = true
device_name = "agentsmith-worker"
# db_path defaults to <data_dir>/signal-db
# authorized_user = "+1234567890"      # Restrict to one phone number

[slack]
enabled = false
app_token = "xapp-..."                 # Slack app-level token (Socket Mode)
bot_token = "xoxb-..."                 # Slack bot token
dm_only = true                         # Only respond to direct messages

[session_defaults]
default_tool = "claude"                # "claude", "gemini", or "goose"
max_sessions = 5                       # Maximum concurrent AI sessions
output_flush_interval_ms = 500         # How often to flush output chunks
max_chunk_size = 3000                  # Max characters per message chunk

[interaction_agent]
enabled = true                         # LLM agent that detects when the CLI needs user input
quiet_timeout_ms = 3000                # ms of quiet output before checking if input is needed

[interaction_agent.llm]
provider = "anthropic"                 # anthropic, openai, gemini, cerebras, groq, grok, sambanova
# api_key = "sk-ant-..."              # Or set ANTHROPIC_API_KEY env var (provider-specific)
# model = "claude-haiku-4-5-20251001" # Optional, uses provider default
max_tokens = 256                       # Max tokens for classification responses

[claude]
binary = "claude"                      # Path to Claude Code binary
# extra_args = ["--model", "opus"]     # Additional CLI arguments
prompt_mode = true                     # Use -p (print) mode instead of interactive PTY
skip_permissions = false               # Pass --dangerously-skip-permissions

[gemini]
binary = "gemini"                      # Path to Gemini CLI binary
# extra_args = []                      # Additional CLI arguments
prompt_mode = true                     # Use non-interactive mode with -r for session resume
skip_permissions = false               # Pass --approval-mode=yolo

[goose]
binary = "goose"                       # Path to Goose binary
# extra_args = []                      # Additional CLI arguments
```

### Interaction Agent

The interaction agent is an LLM-powered layer that monitors CLI output and automatically detects when the AI tool (Claude Code, Gemini CLI) is waiting for user input -- such as permission prompts, yes/no confirmations, or clarification questions. When detected, it forwards the question to your chat and translates your response back into the appropriate terminal input.

**This is enabled by default** and requires an API key for the configured provider. The default provider is Anthropic. Set the API key via environment variable:

```bash
export ANTHROPIC_API_KEY="sk-ant-..."
```

Or in the config file:

```toml
[interaction_agent]
enabled = true

[interaction_agent.llm]
provider = "anthropic"
api_key = "sk-ant-..."
```

**Supported providers:** `anthropic`, `openai`, `gemini`, `cerebras`, `groq`, `grok`, `sambanova`

Each provider reads its API key from a provider-specific environment variable (e.g. `OPENAI_API_KEY`, `GEMINI_API_KEY`, `GROQ_API_KEY`) or from the `api_key` config field. To switch providers:

```toml
[interaction_agent.llm]
provider = "groq"
# model = "llama-3.3-70b-versatile"  # Optional, uses provider default
```

```bash
export GROQ_API_KEY="gsk_..."
```

Without the interaction agent, permission prompts and other interactive questions from the CLI tools will not be forwarded to your chat -- you would need to handle them manually.

### Environment Variable Overrides

Every config value can be overridden with environment variables using the pattern `AGENTSMITH_<SECTION>_<KEY>`:

```bash
export AGENTSMITH_SIGNAL_ENABLED=true
export AGENTSMITH_SIGNAL_AUTHORIZED_USER="+1234567890"
export AGENTSMITH_SLACK_ENABLED=true
export AGENTSMITH_SLACK_APP_TOKEN="xapp-..."
export AGENTSMITH_SLACK_BOT_TOKEN="xoxb-..."
export AGENTSMITH_DAEMON_WORKING_DIR="/home/user/projects"
export AGENTSMITH_DAEMON_LOG_LEVEL="debug"
export ANTHROPIC_API_KEY="sk-ant-..."
```

## Setup

### Signal Setup

This walks through setting up Signal from scratch so you can send a message from your phone and get AI responses back.

**Prerequisites:**
- Signal installed on your phone
- At least one AI CLI tool installed (`claude` or `gemini` on your PATH)

#### 1. Create a Signal config file

Create a `config.toml` with Signal enabled:

```toml
[daemon]
working_dir = "/home/user/projects"   # Directory where AI sessions run
log_level = "info"
data_dir = "/home/user/.agentsmith"   # Where Signal DB and daemon data are stored

[signal]
enabled = true
device_name = "agentsmith-worker"
# authorized_user = "your-uuid"      # Optional: restrict to your Signal account UUID

[session_defaults]
default_tool = "claude"               # "claude", "gemini", or "goose"

[interaction_agent]
enabled = true                        # Detects when the CLI needs input and forwards to chat

[interaction_agent.llm]
provider = "anthropic"                # Or openai, gemini, groq, etc.
# api_key = "sk-ant-..."             # Or set ANTHROPIC_API_KEY env var

[claude]
binary = "claude"
```

Replace `/home/user` with your actual home directory. The `data_dir` will be created automatically. Make sure `ANTHROPIC_API_KEY` is set in your environment so the interaction agent can detect and forward CLI prompts to your chat.

#### 2. Link your Signal account

This is a one-time step that registers the daemon as a linked device on your Signal account:

```bash
# Using the installed binary
agentsmith-remote-worker --link-signal --config config.toml

# Or with cargo from the repo
cargo run --release -- --link-signal --config config.toml
```

A QR code will appear in your terminal. On your phone, open **Signal > Settings > Linked Devices > Link New Device** and scan it. Once linked you should see "Device linked successfully" in the terminal.

#### 3. Start the daemon

```bash
# Using the installed binary
agentsmith-remote-worker --config config.toml

# Or with cargo from the repo
cargo run --release -- --config config.toml
```

You should see output like:

```
AgentSmith Remote Worker starting...
Starting Signal adapter...
AgentSmith Remote Worker is running. Press Ctrl+C to stop.
```

#### 4. Send your first message

Open Signal on your phone and send a message **to yourself** (your own "Note to Self" conversation). The daemon receives messages sent to the linked account.

```
You:    /new claude
Agent:  Created Claude session #1 (now active).

You:    Hello, what can you do?
Agent:  [Claude Code responds...]
```

Any text you send that doesn't start with `/` goes directly to the active AI session. Use `/help` to see all available commands.

#### 5. (Optional) Restrict access to your account

By default the daemon responds to messages from anyone who messages your Signal account. To restrict it to only your messages, set `authorized_user` to your Signal account UUID:

```toml
[signal]
enabled = true
authorized_user = "a1b2c3d4-e5f6-7890-abcd-ef1234567890"
```

You can find your UUID in Signal's settings or from the daemon logs -- it logs the sender UUID for each incoming message when running at `debug` log level.

### Slack Setup

This walks through setting up Slack from scratch so you can send a direct message to a Slack bot and get AI responses back.

**Prerequisites:**
- A Slack workspace where you have permission to create apps (or ask your workspace admin)
- At least one AI CLI tool installed (`claude` or `gemini` on your PATH)

#### 1. Create a Slack App

Go to https://api.slack.com/apps and click **Create New App**. Choose **From scratch**, give it a name (e.g. "AgentSmith Worker"), and select your workspace.

#### 2. Enable Socket Mode

Socket Mode lets the daemon receive events over a WebSocket instead of requiring a public HTTP endpoint.

1. In the left sidebar, go to **Socket Mode**
2. Toggle **Enable Socket Mode** to On
3. You will be prompted to create an **App-Level Token** — name it something like `agentsmith-socket` and add the `connections:write` scope
4. Click **Generate** — copy the token (starts with `xapp-`). You will need this as `app_token` in your config

#### 3. Configure Bot Token Scopes

1. In the left sidebar, go to **OAuth & Permissions**
2. Under **Bot Token Scopes**, add these scopes:
   - `chat:write` — Send messages as the bot
   - `im:history` — Read DM message history
   - `im:read` — View basic DM info
   - `im:write` — Start DMs with people
   - `channels:history` — (Optional) Only needed if you want the bot to respond in channels, not just DMs
3. Scroll up and click **Install to Workspace** (or **Reinstall** if already installed)
4. Copy the **Bot User OAuth Token** (starts with `xoxb-`). You will need this as `bot_token` in your config

#### 4. Enable the Messages Tab

This allows users to send direct messages to the bot.

1. In the left sidebar, go to **App Home**
2. Scroll down to **Show Tabs**
3. Toggle **Messages Tab** to On
4. Check the box **Allow users to send Slash commands and messages from the messages tab**

Without this step you will see "Sending messages to this app has been turned off" when trying to DM the bot.

#### 5. Subscribe to Events

1. In the left sidebar, go to **Event Subscriptions**
2. Toggle **Enable Events** to On
3. Under **Subscribe to bot events**, add:
   - `message.im` — Messages sent to the bot in DMs
   - `message.channels` — (Optional) Messages in channels the bot is in
4. Click **Save Changes**

#### 6. Create the config file

Create a `config.toml` with Slack enabled:

```toml
[daemon]
working_dir = "/home/user/projects"   # Directory where AI sessions run
log_level = "info"
data_dir = "/home/user/.agentsmith"   # Where daemon data is stored

[slack]
enabled = true
app_token = "xapp-1-A0000000000-0000000000000-abc123..."   # App-Level Token from step 2
bot_token = "xoxb-0000000000000-0000000000000-abc123..."   # Bot User OAuth Token from step 3
dm_only = true                         # Only respond to direct messages (recommended)

[session_defaults]
default_tool = "claude"               # "claude", "gemini", or "goose"

[interaction_agent]
enabled = true                        # Detects when the CLI needs input and forwards to chat

[interaction_agent.llm]
provider = "anthropic"                # Or openai, gemini, groq, etc.
# api_key = "sk-ant-..."             # Or set ANTHROPIC_API_KEY env var

[claude]
binary = "claude"
```

Replace the token placeholders with your actual tokens. You can also use environment variables instead of putting tokens in the file:

```bash
export AGENTSMITH_SLACK_APP_TOKEN="xapp-1-..."
export AGENTSMITH_SLACK_BOT_TOKEN="xoxb-..."
export ANTHROPIC_API_KEY="sk-ant-..."   # Or OPENAI_API_KEY, GROQ_API_KEY, etc.
```

#### 7. Start the daemon

```bash
# Using the installed binary
agentsmith-remote-worker --config config.toml

# Or with cargo from the repo
cargo run --release -- --config config.toml
```

You should see output like:

```
AgentSmith Remote Worker starting...
Starting Slack adapter...
AgentSmith Remote Worker is running. Press Ctrl+C to stop.
```

#### 8. Send your first message

Open Slack and start a direct message with your bot (search for the app name you chose in step 1). Send a message:

```
You:    /new claude
Agent:  Created Claude session #1 (now active).

You:    Hello, what can you do?
Agent:  [Claude Code responds...]
```

Any text you send that doesn't start with `/` goes directly to the active AI session. Use `/help` to see all available commands.

#### 9. (Optional) Allow channel messages

By default `dm_only = true` restricts the bot to direct messages only. If you want the bot to respond in channels:

1. Set `dm_only = false` in your config
2. Invite the bot to a channel with `/invite @AgentSmith Worker`
3. Make sure you added the `channels:history` scope and `message.channels` event subscription in the steps above

## Usage

Once the daemon is running, send messages from Signal or Slack:

### Chat Commands

| Command | Description |
|---|---|
| `/new claude` | Start a new Claude Code session |
| `/new gemini` | Start a new Gemini CLI session |
| `/new goose` | Start a new Goose session |
| `/list` | List all active sessions |
| `/switch <id>` | Switch to a different session |
| `/stop <id>` | Stop a specific session |
| `/stop all` | Stop all sessions |
| `/status` | Show daemon status |
| `/help` | Show available commands |

### Example Conversation

```
You:    /new claude
Agent:  Created Claude session #1 (now active).

You:    Create a hello world web server in Python
Agent:  [Claude Code response with Python code...]

You:    /new gemini
Agent:  Created Gemini session #2 (now active).

You:    Explain how async/await works in Rust
Agent:  [Gemini response...]

You:    /switch 1
Agent:  Switched to session #1 (Claude).

You:    /list
Agent:  Sessions:
        #1 Claude (idle) [active]
        #2 Gemini (running)

You:    /stop all
Agent:  All sessions stopped.
```

Any text that doesn't start with `/` is sent directly to the active AI session.

## Running as a Service

### Linux (systemd)

Create `/etc/systemd/system/agentsmith-worker.service`:

```ini
[Unit]
Description=AgentSmith Remote Worker
After=network.target

[Service]
Type=simple
User=your-user
ExecStart=/usr/local/bin/agentsmith-remote-worker --config /home/your-user/.agentsmith/config.toml
Restart=on-failure
RestartSec=10
Environment=AGENTSMITH_DAEMON_LOG_LEVEL=info

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable agentsmith-worker
sudo systemctl start agentsmith-worker
sudo journalctl -u agentsmith-worker -f   # View logs
```

### Linux (Snap)

```bash
sudo snap install agentsmith-worker --devmode
```

## Architecture

```
Signal ──┐                              ┌── Claude Code (-p mode)
         ├── incoming_tx ── Router ──┤── Gemini CLI (PTY mode)
Slack  ──┘                              └── Goose (prompt mode)
         ◄── outgoing_tx ◄─ output ◄─┘
```

- **Router**: Central message dispatcher, owns the `SessionManager`
- **SessionManager**: Creates, tracks, and routes input/output for AI sessions
- **Claude sessions**: Run in `-p` (print) mode with `--continue` for stateful conversations
- **Gemini sessions**: Run in non-interactive prompt mode with `-r "latest"` for session resume (or interactive PTY mode)
- **Goose sessions**: Run in prompt mode with `goose run -n <name> -t` and `-r` for session resume
- **Output buffer**: Aggregates and chunks AI output before sending back to chat

## License

MIT
