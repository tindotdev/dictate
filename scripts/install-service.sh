#!/bin/bash
# Install dictate daemon as a systemd user service
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
      echo "By default, creates symlink at ~/.local/share/nvim/lazy/dictate"
      echo "for lazy.nvim plugin manager integration."
      exit 0
      ;;
  esac
done

echo "=== Dictate Daemon Installer ==="
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
mkdir -p ~/.config/dictate

# -----------------------------------------------------------------------------
# Install systemd service
# -----------------------------------------------------------------------------
echo "Installing systemd service..."

# Copy service file
cp "$PROJECT_ROOT/systemd/dictate.service" ~/.config/systemd/user/

# Update ExecStart path to use actual daemon location
ESCAPED_PATH=$(printf '%s\n' "$DAEMON_PATH" | sed 's/[\/&]/\\&/g')
sed -i "s|%h/.local/share/nvim/lazy/dictate/daemon/dist/main.js|$ESCAPED_PATH|g" \
  ~/.config/systemd/user/dictate.service

# Update PATH to include bun's directory
# The service file has a default PATH, we prepend bun's location
sed -i "s|Environment=PATH=|Environment=PATH=$BUN_DIR:|g" \
  ~/.config/systemd/user/dictate.service

echo "  Installed: ~/.config/systemd/user/dictate.service"

# -----------------------------------------------------------------------------
# Create env file
# -----------------------------------------------------------------------------
if [[ ! -f ~/.config/dictate/env ]]; then
  echo ""
  echo "Setting up API key..."

  if [[ -n "$OPENAI_API_KEY" ]]; then
    echo "OPENAI_API_KEY=$OPENAI_API_KEY" > ~/.config/dictate/env
    echo "  Created ~/.config/dictate/env (using existing OPENAI_API_KEY)"
  else
    read -p "  Enter OPENAI_API_KEY (or press Enter to skip): " api_key
    if [[ -z "$api_key" ]]; then
      echo "  Warning: No API key provided."
      echo "  Set OPENAI_API_KEY in ~/.config/dictate/env before starting the service."
      touch ~/.config/dictate/env
    else
      echo "OPENAI_API_KEY=$api_key" > ~/.config/dictate/env
      echo "  Created ~/.config/dictate/env"
    fi
  fi
  chmod 600 ~/.config/dictate/env
else
  echo "  ~/.config/dictate/env already exists, skipping"
fi

# -----------------------------------------------------------------------------
# Create lazy.nvim symlink (optional)
# -----------------------------------------------------------------------------
if [[ "$LAZY_SYMLINK" == "true" ]]; then
  echo ""
  echo "Creating lazy.nvim symlink..."

  LAZY_DIR="$HOME/.local/share/nvim/lazy"
  LAZY_LINK="$LAZY_DIR/dictate"

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

# Stop old say-based setup if running (migration from old naming)
systemctl --user stop say.service 2>/dev/null || true
systemctl --user disable say.service 2>/dev/null || true
systemctl --user stop say.socket 2>/dev/null || true
systemctl --user disable say.socket 2>/dev/null || true

# Stop old dictate socket-based setup if running
systemctl --user stop dictate.socket 2>/dev/null || true
systemctl --user disable dictate.socket 2>/dev/null || true

# Reload and start
systemctl --user daemon-reload
systemctl --user enable dictate.service
systemctl --user restart dictate.service

# Check status
sleep 1
if systemctl --user is-active --quiet dictate.service; then
  echo "  Service is running"
else
  echo "  Warning: Service may have failed to start"
  echo "  Check: journalctl --user -u dictate.service"
fi

# -----------------------------------------------------------------------------
# Done
# -----------------------------------------------------------------------------
echo ""
echo "=== Installation Complete ==="
echo ""
echo "Commands:"
echo "  systemctl --user status dictate.service   # Check status"
echo "  systemctl --user restart dictate.service  # Restart"
echo "  journalctl --user -u dictate.service -f   # View logs"
echo ""
echo "Socket: /run/user/$(id -u)/dictate/dictate.sock"
echo ""

# -----------------------------------------------------------------------------
# Troubleshooting hints
# -----------------------------------------------------------------------------
if ! systemctl --user is-active --quiet dictate.service; then
  echo "=== Troubleshooting ==="
  echo ""
  echo "If the service fails to start, check:"
  echo ""
  echo "1. Bun path - verify bun is executable:"
  echo "   which bun && bun --version"
  echo ""
  echo "2. API key - ensure it's set:"
  echo "   cat ~/.config/dictate/env"
  echo ""
  echo "3. Daemon build - ensure dist exists:"
  echo "   ls -la $PROJECT_ROOT/daemon/dist/main.js"
  echo ""
  echo "4. Service logs:"
  echo "   journalctl --user -u dictate.service --no-pager -n 50"
  echo ""
fi
