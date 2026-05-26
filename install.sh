#!/usr/bin/env bash
set -euo pipefail

# AgentSmith Remote Worker installer
#
# Downloads the prebuilt binary from GitHub Releases and (on Linux) installs
# it as a systemd service. Defaults to per-user installation; pass --system
# for a host-wide install.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/kevinbayes/agentsmith-worker/main/install.sh | bash
#   curl -fsSL ... | bash -s -- v0.1.0
#   curl -fsSL ... | bash -s -- --system
#   curl -fsSL ... | bash -s -- --no-service       # install binary only
#
# Flags:
#   --system           Install system-wide under /usr/local/bin and run as a
#                      dedicated `agentsmith` system user via systemd system
#                      unit. Requires root (re-runs under sudo if needed).
#   --no-service       Skip systemd service setup; install the binary only.
#   --no-start         Install and enable the service but don't start it now.
#   --install-dir DIR  Override the binary install directory.
#   --config-dir DIR   Override the config directory.
#   v0.1.0             Pin to a specific release tag.

REPO="kevinbayes/agentsmith-worker"
BINARY_NAME="agentsmith-remote-worker"
SERVICE_NAME="agentsmith-worker"
VERSION=""
INSTALL_DIR=""
CONFIG_DIR=""
SYSTEM_MODE=false
SETUP_SERVICE=true
START_SERVICE=true

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --install-dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        --config-dir)
            CONFIG_DIR="$2"
            shift 2
            ;;
        --system)
            SYSTEM_MODE=true
            shift
            ;;
        --no-service)
            SETUP_SERVICE=false
            shift
            ;;
        --no-start)
            START_SERVICE=false
            shift
            ;;
        v*)
            VERSION="$1"
            shift
            ;;
        --help|-h)
            sed -n '4,21p' "$0"
            exit 0
            ;;
        *)
            shift
            ;;
    esac
done

info()    { printf "\033[1;34m==>\033[0m %s\n" "$1"; }
warn()    { printf "\033[1;33m==>\033[0m %s\n" "$1" >&2; }
error()   { printf "\033[1;31mError:\033[0m %s\n" "$1" >&2; exit 1; }
success() { printf "\033[1;32m==>\033[0m %s\n" "$1"; }

# Re-exec under sudo for system mode
if [[ "$SYSTEM_MODE" == "true" && "$(id -u)" != "0" ]]; then
    info "System install requires root; re-running under sudo"
    exec sudo -E bash "$0" "$@"
fi

# Resolve install/config dirs based on mode
if [[ "$SYSTEM_MODE" == "true" ]]; then
    INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
    CONFIG_DIR="${CONFIG_DIR:-/etc/agentsmith}"
    DATA_DIR="/var/lib/agentsmith"
    WORK_DIR="/var/lib/agentsmith/projects"
    SERVICE_USER="agentsmith"
    SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
    SYSTEMCTL=(systemctl)
else
    INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
    CONFIG_DIR="${CONFIG_DIR:-${HOME}/.config/agentsmith}"
    DATA_DIR="${HOME}/.local/share/agentsmith"
    WORK_DIR="${HOME}/agentsmith/projects"
    SERVICE_FILE="${HOME}/.config/systemd/user/${SERVICE_NAME}.service"
    SYSTEMCTL=(systemctl --user)
fi

detect_os() {
    case "$(uname -s)" in
        Linux)  echo "linux" ;;
        Darwin) echo "macos" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)      error "Unsupported operating system: $(uname -s)" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64|amd64)  echo "x86_64" ;;
        aarch64|arm64) echo "aarch64" ;;
        *)             error "Unsupported architecture: $(uname -m)" ;;
    esac
}

get_target() {
    case "$1-$2" in
        linux-x86_64)   echo "x86_64-unknown-linux-gnu" ;;
        linux-aarch64)  echo "aarch64-unknown-linux-gnu" ;;
        windows-x86_64) echo "x86_64-pc-windows-msvc" ;;
        *)              error "No prebuilt binary available for $1 $2" ;;
    esac
}

get_latest_version() {
    local latest
    latest="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
        | grep '"tag_name"' \
        | head -1 \
        | sed -E 's/.*"tag_name":\s*"([^"]+)".*/\1/')" || true
    if [[ -z "$latest" ]]; then
        error "Failed to fetch latest release version. Specify a version: install.sh v0.1.0"
    fi
    echo "$latest"
}

check_deps() {
    for cmd in curl tar; do
        command -v "$cmd" &>/dev/null || error "Required command not found: $cmd"
    done
}

# Create the dedicated service user (system mode only). No-op if already present.
ensure_service_user() {
    if id "$SERVICE_USER" &>/dev/null; then
        return
    fi
    info "Creating system user '${SERVICE_USER}'"
    useradd --system --home-dir "$DATA_DIR" --shell /usr/sbin/nologin \
            --comment "AgentSmith Remote Worker" "$SERVICE_USER"
}

# Drop a starter config file if one doesn't exist yet. The daemon will run
# with all adapters disabled until the user edits it — safe default.
seed_config() {
    local config_file="${CONFIG_DIR}/config.toml"
    if [[ -f "$config_file" ]]; then
        info "Config already present at ${config_file} — leaving it alone"
        return
    fi
    info "Writing starter config to ${config_file}"
    mkdir -p "$CONFIG_DIR"
    cat > "$config_file" <<EOF
# AgentSmith Remote Worker — starter config.
# Enable adapters and fill in credentials as needed.

[daemon]
working_dir = "${WORK_DIR}"
log_level = "info"
data_dir = "${DATA_DIR}"

[web]
enabled = true
host = "127.0.0.1"
port = 3000

[agent_management]
enabled = true
# bin_dir defaults to ~/.local/bin (user mode) or honour whatever you set here.

# --- Messaging adapters (disabled by default) ---
[signal]
enabled = false

[slack]
enabled = false
# app_token = "xapp-..."
# bot_token = "xoxb-..."

[telegram]
enabled = false
# bot_token = "..."

# --- LLM for agent mode (optional) ---
[interaction_agent]
enabled = true
[interaction_agent.llm]
provider = "anthropic"
# api_key = "sk-ant-..."     # Or set ANTHROPIC_API_KEY
EOF

    if [[ "$SYSTEM_MODE" == "true" ]]; then
        chown -R "${SERVICE_USER}:${SERVICE_USER}" "$CONFIG_DIR"
        chmod 640 "$config_file"
    fi
}

# Pre-create runtime dirs so the daemon never has to bootstrap them.
ensure_runtime_dirs() {
    mkdir -p "$DATA_DIR" "$WORK_DIR"
    if [[ "$SYSTEM_MODE" == "true" ]]; then
        chown -R "${SERVICE_USER}:${SERVICE_USER}" "$DATA_DIR" "$WORK_DIR"
    fi
}

write_user_unit() {
    mkdir -p "$(dirname "$SERVICE_FILE")"
    cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=AgentSmith Remote Worker
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=${INSTALL_DIR}/${BINARY_NAME} --config ${CONFIG_DIR}/config.toml
Restart=on-failure
RestartSec=5
WorkingDirectory=${WORK_DIR}
Environment=RUST_LOG=info

[Install]
WantedBy=default.target
EOF
}

write_system_unit() {
    cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=AgentSmith Remote Worker
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
ExecStart=${INSTALL_DIR}/${BINARY_NAME} --config ${CONFIG_DIR}/config.toml
Restart=on-failure
RestartSec=5
WorkingDirectory=${WORK_DIR}
Environment=RUST_LOG=info
# Sandboxing — relax these if the daemon needs broader filesystem access
ProtectSystem=full
ProtectHome=read-only
ReadWritePaths=${DATA_DIR} ${WORK_DIR} ${CONFIG_DIR}
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF
}

install_service() {
    info "Writing systemd unit to ${SERVICE_FILE}"
    if [[ "$SYSTEM_MODE" == "true" ]]; then
        write_system_unit
    else
        write_user_unit
    fi

    info "Reloading systemd"
    "${SYSTEMCTL[@]}" daemon-reload

    info "Enabling ${SERVICE_NAME}"
    "${SYSTEMCTL[@]}" enable "${SERVICE_NAME}.service"

    if [[ "$START_SERVICE" == "true" ]]; then
        info "Starting ${SERVICE_NAME}"
        "${SYSTEMCTL[@]}" restart "${SERVICE_NAME}.service"
    else
        warn "Skipping service start (--no-start). Run: ${SYSTEMCTL[*]} start ${SERVICE_NAME}"
    fi
}

print_summary() {
    echo ""
    success "Installation complete."
    echo ""
    echo "  Binary:  ${INSTALL_DIR}/${BINARY_NAME}"
    echo "  Config:  ${CONFIG_DIR}/config.toml"
    echo "  Data:    ${DATA_DIR}"
    echo ""

    if [[ "$SETUP_SERVICE" != "true" ]]; then
        if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
            echo "  Add to your PATH:"
            echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
            echo ""
        fi
        return
    fi

    echo "  Service control:"
    echo "    ${SYSTEMCTL[*]} status   ${SERVICE_NAME}"
    echo "    ${SYSTEMCTL[*]} restart  ${SERVICE_NAME}"
    if [[ "$SYSTEM_MODE" == "true" ]]; then
        echo "    journalctl -fu ${SERVICE_NAME}"
    else
        echo "    journalctl --user -fu ${SERVICE_NAME}"
        echo ""
        # Linger state lives at /var/lib/systemd/linger/<user>. systemd creates
        # this file when `loginctl enable-linger` is run. `loginctl show-user`
        # only emits Linger=yes for users with active sessions, so the file
        # check is the reliable signal.
        if [[ ! -f "/var/lib/systemd/linger/${USER}" ]]; then
            warn "User-mode services stop when you log out."
            echo "  To keep the daemon running across logouts/reboots:"
            echo "    sudo loginctl enable-linger \$USER"
        fi
    fi
    echo ""
    echo "  Edit ${CONFIG_DIR}/config.toml to enable adapters, then restart the service."
}

main() {
    check_deps

    local os arch target
    os="$(detect_os)"
    arch="$(detect_arch)"
    target="$(get_target "$os" "$arch")"

    # Service setup is Linux + systemd only
    if [[ "$os" != "linux" && "$SETUP_SERVICE" == "true" ]]; then
        warn "Service setup is only supported on Linux. Installing binary only."
        SETUP_SERVICE=false
    fi
    if [[ "$SETUP_SERVICE" == "true" ]] && ! command -v systemctl &>/dev/null; then
        warn "systemctl not found. Installing binary only."
        SETUP_SERVICE=false
    fi

    if [[ -z "$VERSION" ]]; then
        info "Fetching latest release version"
        VERSION="$(get_latest_version)"
    fi

    info "Installing ${BINARY_NAME} ${VERSION} for ${target}"
    info "Mode: $([[ "$SYSTEM_MODE" == "true" ]] && echo system || echo user)"

    local ext="tar.gz"
    [[ "$os" == "windows" ]] && ext="zip"

    local archive_name="${BINARY_NAME}-${VERSION}-${target}.${ext}"
    local download_url="https://github.com/${REPO}/releases/download/${VERSION}/${archive_name}"

    # NOTE: tmpdir must be a global (not `local`) because the EXIT trap fires
    # after main() returns, by which point a local would be out of scope and
    # `set -u` would treat $tmpdir as unbound.
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "${tmpdir:-}"' EXIT

    info "Downloading ${download_url}"
    if ! curl -fsSL -o "${tmpdir}/${archive_name}" "$download_url"; then
        error "Download failed. Check that '${VERSION}' exists at https://github.com/${REPO}/releases"
    fi

    info "Extracting archive"
    if [[ "$ext" == "tar.gz" ]]; then
        tar -xzf "${tmpdir}/${archive_name}" -C "$tmpdir"
    else
        unzip -q "${tmpdir}/${archive_name}" -d "$tmpdir"
    fi

    local binary_path
    if [[ "$os" == "windows" ]]; then
        binary_path="$(find "$tmpdir" -name "${BINARY_NAME}.exe" -type f | head -1)"
    else
        binary_path="$(find "$tmpdir" -name "${BINARY_NAME}" -type f | head -1)"
    fi
    [[ -n "$binary_path" ]] || error "Binary not found in archive"

    info "Installing binary to ${INSTALL_DIR}"
    mkdir -p "$INSTALL_DIR"
    install -m 0755 "$binary_path" "${INSTALL_DIR}/${BINARY_NAME}"

    if [[ "$SETUP_SERVICE" == "true" ]]; then
        if [[ "$SYSTEM_MODE" == "true" ]]; then
            ensure_service_user
        fi
        ensure_runtime_dirs
        seed_config
        install_service
    fi

    print_summary
}

main "$@"
