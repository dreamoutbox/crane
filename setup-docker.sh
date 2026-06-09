#!/bin/sh

set -e

docker build -f 'Dockerfile.vps' -t 'dreamoutbox/crane-dev-vps:latest' .

docker compose -f docker-compose.dev.yml up -d --build

for node in vps1 vps2 vps3; do
  docker exec "$node" sh -c "cp /opt/authorized_keys /home/crane/.ssh/authorized_keys && chown crane:crane /home/crane/.ssh/authorized_keys && chmod 600 /home/crane/.ssh/authorized_keys"
done

echo "Checking SSH connectivity to vps1-3..."
for port in 2221 2222 2223; do
  attempt=1
  until ssh -i keys/id_ed25519 -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=2 -p "$port" crane@localhost true 2>/dev/null; do
    if [ "$attempt" -ge 15 ]; then
      echo "Failed to connect to vps on port $port after 15 attempts" >&2
      exit 1
    fi
    
    attempt=$((attempt + 1))
    sleep 1
  done
done
echo "✓ Connection verification succeeded for all VPS instances"
