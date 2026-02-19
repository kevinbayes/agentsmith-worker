#!/usr/bin/env bash
set -euo pipefail

# AgentSmith Remote Worker installer
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/kevinbayes/agentsmith-worker/main/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/kevinbayes/agentsmith-worker/main/install.sh | bash -s -- v0.1.0
#   curl -fsSL https://raw.githubusercontent.com/kevinbayes/agentsmith-worker/main/install.sh | bash -s -- --install-dir /usr/local/bin

REPO="kevinbayes/agentsmith-worker"
BINARY_NAME="agentsmith-remote-worker"
INSTALL_DIR="${HOME}/.local/bin"
VERSION=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case "$1" in
        --install-dir)
            INSTALL_DIR="$2"
            shift 2
            ;;
        v*)
            VERSION="$1"
            shift
            ;;
        *)
            shift
            ;;
    esac
done

info() {
    printf "\033[1;34m==>\033[0m %s\n" "$1"
}

error() {
    printf "\033[1;31mError:\033[0m %s\n" "$1" >&2
    exit 1
}

success() {
    printf "\033[1;32m==>\033[0m %s\n" "$1"
}

# Detect OS
detect_os() {
    local os
    os="$(uname -s)"
    case "$os" in
        Linux)  echo "linux" ;;
        Darwin) echo "macos" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *)      error "Unsupported operating system: $os" ;;
    esac
}

# Detect architecture
detect_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        x86_64|amd64)   echo "x86_64" ;;
        aarch64|arm64)   echo "aarch64" ;;
        *)               error "Unsupported architecture: $arch" ;;
    esac
}

# Get the target triple
get_target() {
    local os="$1"
    local arch="$2"

    case "${os}-${arch}" in
        linux-x86_64)   echo "x86_64-unknown-linux-gnu" ;;
        linux-aarch64)  echo "aarch64-unknown-linux-gnu" ;;
        windows-x86_64) echo "x86_64-pc-windows-msvc" ;;
        *)              error "No prebuilt binary available for ${os} ${arch}" ;;
    esac
}

# Get latest release version from GitHub
get_latest_version() {
    local latest
    latest="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
        | grep '"tag_name"' \
        | head -1 \
        | sed -E 's/.*"tag_name":\s*"([^"]+)".*/\1/')" || true

    if [[ -z "$latest" ]]; then
        error "Failed to fetch latest release version. Specify a version manually: install.sh v0.1.0"
    fi
    echo "$latest"
}

# Check for required commands
check_deps() {
    for cmd in curl tar; do
        if ! command -v "$cmd" &>/dev/null; then
            error "Required command not found: $cmd"
        fi
    done
}

main() {
    check_deps

    local os arch target
    os="$(detect_os)"
    arch="$(detect_arch)"
    target="$(get_target "$os" "$arch")"

    if [[ -z "$VERSION" ]]; then
        info "Fetching latest release version..."
        VERSION="$(get_latest_version)"
    fi

    info "Installing ${BINARY_NAME} ${VERSION} for ${target}"

    # Determine archive extension and download URL
    local ext="tar.gz"
    if [[ "$os" == "windows" ]]; then
        ext="zip"
    fi

    local archive_name="${BINARY_NAME}-${VERSION}-${target}.${ext}"
    local download_url="https://github.com/${REPO}/releases/download/${VERSION}/${archive_name}"

    # Create temp directory
    local tmpdir
    tmpdir="$(mktemp -d)"
    trap 'rm -rf "$tmpdir"' EXIT

    info "Downloading ${download_url}..."
    if ! curl -fsSL -o "${tmpdir}/${archive_name}" "$download_url"; then
        error "Download failed. Check that version '${VERSION}' exists at https://github.com/${REPO}/releases"
    fi

    # Extract
    info "Extracting..."
    if [[ "$ext" == "tar.gz" ]]; then
        tar -xzf "${tmpdir}/${archive_name}" -C "$tmpdir"
    else
        unzip -q "${tmpdir}/${archive_name}" -d "$tmpdir"
    fi

    # Find the binary
    local binary_path
    if [[ "$os" == "windows" ]]; then
        binary_path="$(find "$tmpdir" -name "${BINARY_NAME}.exe" -type f | head -1)"
    else
        binary_path="$(find "$tmpdir" -name "${BINARY_NAME}" -type f | head -1)"
    fi

    if [[ -z "$binary_path" ]]; then
        error "Binary not found in archive"
    fi

    # Install
    mkdir -p "$INSTALL_DIR"
    cp "$binary_path" "${INSTALL_DIR}/${BINARY_NAME}"
    chmod +x "${INSTALL_DIR}/${BINARY_NAME}"

    success "Installed ${BINARY_NAME} to ${INSTALL_DIR}/${BINARY_NAME}"

    # Check if install dir is in PATH
    if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
        echo ""
        echo "  Add ${INSTALL_DIR} to your PATH:"
        echo ""
        echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
        echo ""
        echo "  Add this line to your ~/.bashrc or ~/.zshrc to make it permanent."
        echo ""
    fi

    # Verify installation
    if command -v "$BINARY_NAME" &>/dev/null; then
        success "Run '${BINARY_NAME} --help' to get started"
    else
        echo ""
        echo "  After updating your PATH, run:"
        echo "    ${BINARY_NAME} --help"
        echo ""
    fi
}

main "$@"
