#!/bin/sh

set -e

# Ensure SSH keys and authorized_keys exist before mounting into containers
if [ ! -d "keys" ] && [ -d "../keys" ]; then
  ln -s ../keys keys
fi

mkdir -p keys

if [ -d "keys/authorized_keys" ]; then
  rm -rf "keys/authorized_keys"
fi

if [ ! -f "keys/authorized_keys" ]; then
  if [ -f "keys/id_ed25519.pub" ]; then
    cp "keys/id_ed25519.pub" "keys/authorized_keys"
  elif [ -f "../keys/id_ed25519.pub" ]; then
    cp "../keys/id_ed25519.pub" "keys/authorized_keys"
  else
    echo "Generating SSH simulation keys..."
    ssh-keygen -t ed25519 -C "crane-sim" -f "keys/id_ed25519" -N ""
    cp "keys/id_ed25519.pub" "keys/authorized_keys"
  fi
fi

docker build --platform linux/amd64 -f 'Dockerfile.rocky.vps' -t 'dreamoutbox/crane-dev-vps-rocky:latest' .

docker compose -f docker-compose.dev.yml down
docker compose -f docker-compose.rocky.dev.yml down
docker compose -f docker-compose.rocky.dev.yml up -d --build

for node in vps1 vps2 vps3; do
  docker exec "$node" sh -c "cp /opt/authorized_keys /home/crane/.ssh/authorized_keys && chown crane:crane /home/crane/.ssh/authorized_keys && chmod 600 /home/crane/.ssh/authorized_keys && mkdir -p /root/.ssh && cp /opt/authorized_keys /root/.ssh/authorized_keys && chmod 700 /root/.ssh && chmod 600 /root/.ssh/authorized_keys"
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
echo "Connection verification succeeded for all VPS instances"
