#!/usr/bin/env bash
set -euo pipefail

# AgentSmith Remote Worker uninstaller
#
# Stops the systemd service, removes the binary, and (optionally) cleans up
# config and data. Mirrors the install.sh layout — same defaults, same flags.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/kevinbayes/agentsmith-worker/main/uninstall.sh | bash
#   curl -fsSL ... | bash -s -- --system
#   curl -fsSL ... | bash -s -- --purge
#
# Flags:
#   --system           Remove a system-wide install (under /usr/local/bin,
#                      systemd system unit, `agentsmith` service user).
#                      Requires root (re-runs under sudo if needed).
#   --purge            Also delete config dir, data dir, and (system mode)
#                      the dedicated service user. Without this, config and
#                      data are left in place so reinstall preserves state.
#   --keep-config      Even with --purge, leave the config directory alone.
#   --install-dir DIR  Override the binary install directory.
#   --config-dir DIR   Override the config directory.
#   --yes              Don't prompt before removing files.

REPO="kevinbayes/agentsmith-worker"
BINARY_NAME="agentsmith-remote-worker"
SERVICE_NAME="agentsmith-worker"
INSTALL_DIR=""
CONFIG_DIR=""
SYSTEM_MODE=false
PURGE=false
KEEP_CONFIG=false
ASSUME_YES=false

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
        --purge)
            PURGE=true
            shift
            ;;
        --keep-config)
            KEEP_CONFIG=true
            shift
            ;;
        --yes|-y)
            ASSUME_YES=true
            shift
            ;;
        --help|-h)
            sed -n '4,24p' "$0"
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
    info "System uninstall requires root; re-running under sudo"
    exec sudo -E bash "$0" "$@"
fi

# Resolve install/config dirs based on mode (must match install.sh)
if [[ "$SYSTEM_MODE" == "true" ]]; then
    INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
    CONFIG_DIR="${CONFIG_DIR:-/etc/agentsmith}"
    DATA_DIR="/var/lib/agentsmith"
    SERVICE_USER="agentsmith"
    SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
    SYSTEMCTL=(systemctl)
else
    INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
    CONFIG_DIR="${CONFIG_DIR:-${HOME}/.config/agentsmith}"
    DATA_DIR="${HOME}/.local/share/agentsmith"
    SERVICE_FILE="${HOME}/.config/systemd/user/${SERVICE_NAME}.service"
    SYSTEMCTL=(systemctl --user)
fi

confirm() {
    [[ "$ASSUME_YES" == "true" ]] && return 0
    local prompt="$1"
    local reply
    printf "%s [y/N] " "$prompt"
    read -r reply </dev/tty || return 1
    [[ "$reply" =~ ^[Yy]$ ]]
}

stop_service() {
    if [[ ! -f "$SERVICE_FILE" ]] && ! "${SYSTEMCTL[@]}" list-unit-files "${SERVICE_NAME}.service" &>/dev/null; then
        info "No ${SERVICE_NAME} systemd unit found — skipping service teardown"
        return
    fi

    if "${SYSTEMCTL[@]}" is-active --quiet "${SERVICE_NAME}.service" 2>/dev/null; then
        info "Stopping ${SERVICE_NAME}"
        "${SYSTEMCTL[@]}" stop "${SERVICE_NAME}.service" || warn "Failed to stop service cleanly"
    fi

    if "${SYSTEMCTL[@]}" is-enabled --quiet "${SERVICE_NAME}.service" 2>/dev/null; then
        info "Disabling ${SERVICE_NAME}"
        "${SYSTEMCTL[@]}" disable "${SERVICE_NAME}.service" || warn "Failed to disable service"
    fi

    if [[ -f "$SERVICE_FILE" ]]; then
        info "Removing ${SERVICE_FILE}"
        rm -f "$SERVICE_FILE"
    fi

    info "Reloading systemd"
    "${SYSTEMCTL[@]}" daemon-reload || true
    "${SYSTEMCTL[@]}" reset-failed "${SERVICE_NAME}.service" 2>/dev/null || true
}

remove_binary() {
    local binary_path="${INSTALL_DIR}/${BINARY_NAME}"
    if [[ -f "$binary_path" ]]; then
        info "Removing ${binary_path}"
        rm -f "$binary_path"
    else
        info "Binary not present at ${binary_path}"
    fi
}

remove_config_and_data() {
    if [[ "$PURGE" != "true" ]]; then
        echo ""
        info "Config and data left in place (pass --purge to remove):"
        echo "    ${CONFIG_DIR}"
        echo "    ${DATA_DIR}"
        return
    fi

    if [[ "$KEEP_CONFIG" != "true" ]]; then
        if [[ -d "$CONFIG_DIR" ]]; then
            if confirm "Delete config dir ${CONFIG_DIR}?"; then
                info "Removing ${CONFIG_DIR}"
                rm -rf "$CONFIG_DIR"
            else
                warn "Keeping ${CONFIG_DIR}"
            fi
        fi
    else
        info "Keeping config dir ${CONFIG_DIR} (--keep-config)"
    fi

    if [[ -d "$DATA_DIR" ]]; then
        if confirm "Delete data dir ${DATA_DIR}?"; then
            info "Removing ${DATA_DIR}"
            rm -rf "$DATA_DIR"
        else
            warn "Keeping ${DATA_DIR}"
        fi
    fi
}

remove_service_user() {
    [[ "$SYSTEM_MODE" == "true" ]] || return 0
    [[ "$PURGE" == "true" ]] || return 0
    id "$SERVICE_USER" &>/dev/null || return 0

    if confirm "Delete system user '${SERVICE_USER}'?"; then
        info "Removing user ${SERVICE_USER}"
        # userdel may complain if home dir is already gone — don't fail the run.
        userdel "$SERVICE_USER" 2>/dev/null || warn "userdel reported an issue (user may already be partially removed)"
    else
        warn "Keeping system user ${SERVICE_USER}"
    fi
}

print_summary() {
    echo ""
    success "Uninstall complete."
    echo ""
    echo "  Mode:   $([[ "$SYSTEM_MODE" == "true" ]] && echo system || echo user)"
    echo "  Binary: removed from ${INSTALL_DIR}"

    if [[ "$PURGE" == "true" ]]; then
        echo "  Config: ${CONFIG_DIR} $([[ -d "$CONFIG_DIR" ]] && echo "(kept)" || echo "(removed)")"
        echo "  Data:   ${DATA_DIR} $([[ -d "$DATA_DIR" ]] && echo "(kept)" || echo "(removed)")"
    else
        echo "  Config: ${CONFIG_DIR} (kept — pass --purge to remove)"
        echo "  Data:   ${DATA_DIR} (kept — pass --purge to remove)"
    fi
    echo ""
}

main() {
    local os
    case "$(uname -s)" in
        Linux)  os="linux" ;;
        Darwin) os="macos" ;;
        *)      os="other" ;;
    esac

    # Service teardown is Linux + systemd only
    if [[ "$os" == "linux" ]] && command -v systemctl &>/dev/null; then
        stop_service
    else
        info "systemctl not available — skipping service teardown"
    fi

    remove_binary
    remove_service_user
    remove_config_and_data
    print_summary
}

main "$@"
