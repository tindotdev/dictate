#!/bin/bash
# Install say daemon as a systemd user service
#
# This script:
# 1. Checks for bun in PATH
# 2. Builds the daemon if needed
# 3. Installs systemd service with correct paths
# 4. Optionally creates lazy.nvim symlink for Neovim integration
#
# Usage:
#   ./install-service.sh              # Install with lazy.nvim symlink (default)
#   ./install-service.sh --no-lazy    # Skip lazy.nvim symlink

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
LAZY_SYMLINK=true

# Parse arguments
for arg in "$@"; do
  case $arg in
    --no-lazy)
      LAZY_SYMLINK=false
      ;;
    -h|--help)
      echo "Usage: $0 [--no-lazy]"
      echo ""
      echo "Options:"
      echo "  --no-lazy    Skip creating lazy.nvim symlink"
      echo ""
      echo "By default, creates symlink at ~/.local/share/nvim/lazy/say"
      echo "for lazy.nvim plugin manager integration."
      exit 0
      ;;
  esac
done

echo "=== Say Daemon Installer ==="
echo ""

# -----------------------------------------------------------------------------
# Check for bun
# -----------------------------------------------------------------------------
echo "Checking for bun..."

BUN_PATH=$(which bun 2>/dev/null || true)
if [[ -z "$BUN_PATH" ]]; then
  echo "Error: bun not found in PATH"
  echo ""
  echo "Install bun: https://bun.sh/docs/installation"
  echo "  curl -fsSL https://bun.sh/install | bash"
  exit 1
fi

BUN_DIR=$(dirname "$BUN_PATH")
echo "  Found: $BUN_PATH"

# -----------------------------------------------------------------------------
# Build daemon if needed
# -----------------------------------------------------------------------------
DAEMON_PATH="$PROJECT_ROOT/daemon/dist/main.js"

if [[ ! -f "$DAEMON_PATH" ]]; then
  echo ""
  echo "Building daemon..."
  (cd "$PROJECT_ROOT/daemon" && bun run build)
fi

if [[ ! -f "$DAEMON_PATH" ]]; then
  echo "Error: Failed to build daemon. Check for errors above."
  exit 1
fi
echo "  Daemon: $DAEMON_PATH"

# -----------------------------------------------------------------------------
# Create directories
# -----------------------------------------------------------------------------
echo ""
echo "Creating directories..."
mkdir -p ~/.config/systemd/user
mkdir -p ~/.config/say

# -----------------------------------------------------------------------------
# Install systemd service
# -----------------------------------------------------------------------------
echo "Installing systemd service..."

# Copy service file
cp "$PROJECT_ROOT/systemd/say.service" ~/.config/systemd/user/

# Update ExecStart path to use actual daemon location
ESCAPED_PATH=$(printf '%s\n' "$DAEMON_PATH" | sed 's/[\/&]/\\&/g')
sed -i "s|%h/.local/share/nvim/lazy/say/daemon/dist/main.js|$ESCAPED_PATH|g" \
  ~/.config/systemd/user/say.service

# Update PATH to include bun's directory
# The service file has a default PATH, we prepend bun's location
sed -i "s|Environment=PATH=|Environment=PATH=$BUN_DIR:|g" \
  ~/.config/systemd/user/say.service

echo "  Installed: ~/.config/systemd/user/say.service"

# -----------------------------------------------------------------------------
# Create env file
# -----------------------------------------------------------------------------
if [[ ! -f ~/.config/say/env ]]; then
  echo ""
  echo "Setting up API key..."

  if [[ -n "$OPENAI_API_KEY" ]]; then
    echo "OPENAI_API_KEY=$OPENAI_API_KEY" > ~/.config/say/env
    echo "  Created ~/.config/say/env (using existing OPENAI_API_KEY)"
  else
    read -p "  Enter OPENAI_API_KEY (or press Enter to skip): " api_key
    if [[ -z "$api_key" ]]; then
      echo "  Warning: No API key provided."
      echo "  Set OPENAI_API_KEY in ~/.config/say/env before starting the service."
      touch ~/.config/say/env
    else
      echo "OPENAI_API_KEY=$api_key" > ~/.config/say/env
      echo "  Created ~/.config/say/env"
    fi
  fi
  chmod 600 ~/.config/say/env
else
  echo "  ~/.config/say/env already exists, skipping"
fi

# -----------------------------------------------------------------------------
# Create lazy.nvim symlink (optional)
# -----------------------------------------------------------------------------
if [[ "$LAZY_SYMLINK" == "true" ]]; then
  echo ""
  echo "Creating lazy.nvim symlink..."

  LAZY_DIR="$HOME/.local/share/nvim/lazy"
  LAZY_LINK="$LAZY_DIR/say"

  mkdir -p "$LAZY_DIR"

  if [[ -L "$LAZY_LINK" ]]; then
    # Remove existing symlink
    rm "$LAZY_LINK"
  elif [[ -e "$LAZY_LINK" ]]; then
    echo "  Warning: $LAZY_LINK exists and is not a symlink"
    echo "  Skipping symlink creation"
    LAZY_SYMLINK=false
  fi

  if [[ "$LAZY_SYMLINK" == "true" ]]; then
    ln -s "$PROJECT_ROOT" "$LAZY_LINK"
    echo "  Created: $LAZY_LINK -> $PROJECT_ROOT"
  fi
fi

# -----------------------------------------------------------------------------
# Reload and start systemd
# -----------------------------------------------------------------------------
echo ""
echo "Starting service..."

# Stop old socket-based setup if running
systemctl --user stop say.socket 2>/dev/null || true
systemctl --user disable say.socket 2>/dev/null || true

# Reload and start
systemctl --user daemon-reload
systemctl --user enable say.service
systemctl --user restart say.service

# Check status
sleep 1
if systemctl --user is-active --quiet say.service; then
  echo "  Service is running"
else
  echo "  Warning: Service may have failed to start"
  echo "  Check: journalctl --user -u say.service"
fi

# -----------------------------------------------------------------------------
# Done
# -----------------------------------------------------------------------------
echo ""
echo "=== Installation Complete ==="
echo ""
echo "Commands:"
echo "  systemctl --user status say.service   # Check status"
echo "  systemctl --user restart say.service  # Restart"
echo "  journalctl --user -u say.service -f   # View logs"
echo ""
echo "Socket: /run/user/$(id -u)/say/say.sock"
echo ""

# -----------------------------------------------------------------------------
# Troubleshooting hints
# -----------------------------------------------------------------------------
if ! systemctl --user is-active --quiet say.service; then
  echo "=== Troubleshooting ==="
  echo ""
  echo "If the service fails to start, check:"
  echo ""
  echo "1. Bun path - verify bun is executable:"
  echo "   which bun && bun --version"
  echo ""
  echo "2. API key - ensure it's set:"
  echo "   cat ~/.config/say/env"
  echo ""
  echo "3. Daemon build - ensure dist exists:"
  echo "   ls -la $PROJECT_ROOT/daemon/dist/main.js"
  echo ""
  echo "4. Service logs:"
  echo "   journalctl --user -u say.service --no-pager -n 50"
  echo ""
fi
