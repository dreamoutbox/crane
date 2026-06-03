#!/usr/bin/env bash
# setup.sh — one-time setup for crane VPS simulation
# Run once before `docker compose up`

set -euo pipefail

# Keys dir
KEYS_DIR="$(dirname "$0")/keys"
mkdir -p "$KEYS_DIR"

# Add vps hosts to SSH config to avoid StrictHostKeyChecking prompts
SSH_CONFIG="$HOME/.ssh/config"
MARKER="# crane-vps"

# Use existing SSH key or generate a dedicated sim key
if [[ -f "$KEYS_DIR/id_ed25519.pub" ]]; then
  cp "$KEYS_DIR/id_ed25519.pub" "$KEYS_DIR/authorized_keys"
  echo "✓ Copied keys/id_ed25519.pub → keys/authorized_keys"
else
  echo "No existing SSH key found. Generating crane-sim key..."
  ssh-keygen -t ed25519 -C "crane-sim" -f "$KEYS_DIR/id_ed25519" -N ""
  cp "$KEYS_DIR/id_ed25519.pub" "$KEYS_DIR/authorized_keys"
  echo "✓ Generated keys/id_ed25519"
fi

if ! grep -q "$MARKER" "$SSH_CONFIG" 2>/dev/null; then
  cat >> "$SSH_CONFIG" <<EOF

$MARKER

Host *
  StrictHostKeyChecking no
  LogLevel ERROR

Host vps1
  HostName localhost
  Port 2221
  User crane
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null

Host vps2
  HostName localhost
  Port 2222
  User crane
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null

Host vps3
  HostName localhost
  Port 2223
  User crane
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
EOF
  echo "✓ Added crane-vps hosts to $SSH_CONFIG"
else
  echo "✓ SSH config already contains crane-vps entries"
fi
