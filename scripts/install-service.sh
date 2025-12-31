#!/bin/bash
# Install say daemon as a systemd user service
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "Installing say daemon systemd service..."

# Create directories
mkdir -p ~/.config/systemd/user
mkdir -p ~/.config/say

# Copy service files
cp "$PROJECT_ROOT/systemd/say.socket" ~/.config/systemd/user/
cp "$PROJECT_ROOT/systemd/say.service" ~/.config/systemd/user/

# Update ExecStart path in service file to use actual location
DAEMON_PATH="$PROJECT_ROOT/daemon/dist/main.js"
if [[ ! -f "$DAEMON_PATH" ]]; then
  echo "Error: Daemon not built. Run 'bun run build' in daemon/ first."
  exit 1
fi

# Use sed to update the path (escape slashes)
ESCAPED_PATH=$(printf '%s\n' "$DAEMON_PATH" | sed 's/[\/&]/\\&/g')
sed -i "s|%h/.local/share/nvim/lazy/say/daemon/dist/main.js|$ESCAPED_PATH|g" ~/.config/systemd/user/say.service

# Create env file if not exists
if [[ ! -f ~/.config/say/env ]]; then
  if [[ -n "$OPENAI_API_KEY" ]]; then
    echo "OPENAI_API_KEY=$OPENAI_API_KEY" > ~/.config/say/env
    echo "Created ~/.config/say/env with existing OPENAI_API_KEY"
  else
    read -p "Enter OPENAI_API_KEY: " api_key
    if [[ -z "$api_key" ]]; then
      echo "Warning: No API key provided. Set OPENAI_API_KEY in ~/.config/say/env"
      touch ~/.config/say/env
    else
      echo "OPENAI_API_KEY=$api_key" > ~/.config/say/env
    fi
  fi
  chmod 600 ~/.config/say/env
else
  echo "~/.config/say/env already exists, skipping"
fi

# Reload and enable
systemctl --user daemon-reload
systemctl --user enable --now say.socket

echo ""
echo "Done! Socket is listening. Service starts on first connection."
echo ""
echo "Commands:"
echo "  systemctl --user status say.socket   # Check socket status"
echo "  systemctl --user status say.service  # Check service status"
echo "  journalctl --user -u say.service     # View logs"
echo ""
