#!/bin/sh

set -e

KEYS_DIR="../dev-ssh-keys"
chmod 600 "$KEYS_DIR/id_ed25519"
if [ ! -f "$KEYS_DIR/authorized_keys" ]; then
  cp "$KEYS_DIR/id_ed25519.pub" "$KEYS_DIR/authorized_keys"
fi

# Fallback if docker credsStore (e.g. desktop.exe in WSL) cannot be executed
if [ -f "$HOME/.docker/config.json" ] && grep -q '"desktop.exe"' "$HOME/.docker/config.json" 2>/dev/null; then
  if ! /usr/bin/docker-credential-desktop.exe version >/dev/null 2>&1; then
    TEMP_DOCKER_DIR="$(mktemp -d)"
    echo '{}' > "$TEMP_DOCKER_DIR/config.json"
    export DOCKER_CONFIG="$TEMP_DOCKER_DIR"
    trap 'rm -rf "$TEMP_DOCKER_DIR"' EXIT
  fi
fi

docker build --platform linux/amd64 -f 'Dockerfile.vps' -t 'dreamoutbox/crane-dev-vps:latest' .

docker compose -f docker-compose.dev.yml down
docker compose -f docker-compose.dev.yml up -d --build

for node in vps1 vps2 vps3; do
  docker exec "$node" sh -c "cp /opt/authorized_keys /home/crane/.ssh/authorized_keys && chown crane:crane /home/crane/.ssh/authorized_keys && chmod 600 /home/crane/.ssh/authorized_keys && mkdir -p /root/.ssh && cp /opt/authorized_keys /root/.ssh/authorized_keys && chmod 700 /root/.ssh && chmod 600 /root/.ssh/authorized_keys"
done

echo "Checking SSH connectivity to vps1-3..."
for port in 2221 2222 2223; do
  attempt=1
  until ssh -i "$KEYS_DIR/id_ed25519" -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=2 -p "$port" crane@localhost true 2>/dev/null; do
    if [ "$attempt" -ge 15 ]; then
      echo "Failed to connect to vps on port $port after 15 attempts" >&2
      exit 1
    fi
    
    attempt=$((attempt + 1))
    sleep 1
  done
done
echo "Connection verification succeeded for all VPS instances"
