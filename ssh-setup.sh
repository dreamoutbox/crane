#!/usr/bin/env bash
# setup.sh — one-time setup for crane VPS simulation
# Run once before `docker compose up`

set -euo pipefail

KEYS_DIR="$(dirname "$0")/keys"
mkdir -p "$KEYS_DIR"

# Use existing SSH key or generate a dedicated sim key
if [[ -f "$HOME/.ssh/id_rsa.pub" ]]; then
  cp "$HOME/.ssh/id_rsa.pub" "$KEYS_DIR/authorized_keys"
  echo "✓ Copied ~/.ssh/id_rsa.pub → keys/authorized_keys"
elif [[ -f "$HOME/.ssh/id_ed25519.pub" ]]; then
  cp "$HOME/.ssh/id_ed25519.pub" "$KEYS_DIR/authorized_keys"
  echo "✓ Copied ~/.ssh/id_ed25519.pub → keys/authorized_keys"
else
  echo "No existing SSH key found. Generating crane-sim key..."
  ssh-keygen -t ed25519 -C "crane-sim" -f "$KEYS_DIR/crane_sim_key" -N ""
  cp "$KEYS_DIR/crane_sim_key.pub" "$KEYS_DIR/authorized_keys"
  echo "✓ Generated keys/crane_sim_key (add to your SSH agent: ssh-add keys/crane_sim_key)"
fi

# Add sim hosts to SSH config to avoid StrictHostKeyChecking prompts
SSH_CONFIG="$HOME/.ssh/config"
MARKER="# crane-sim"

if ! grep -q "$MARKER" "$SSH_CONFIG" 2>/dev/null; then
  cat >> "$SSH_CONFIG" <<EOF

$MARKER

Host *
  StrictHostKeyChecking no
  LogLevel ERROR

Host vps1
  HostName localhost
  Port 2221
  User admin
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null

Host vps2
  HostName localhost
  Port 2222
  User admin
  StrictHostKeyChecking no
  UserKnownHostsFile /dev/null
EOF
  echo "✓ Added crane-sim hosts to ~/.ssh/config"
else
  echo "✓ SSH config already contains crane-sim entries"
fi
