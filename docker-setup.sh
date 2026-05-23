#!/bin/sh

set -e

docker build -f 'Dockerfile.vps' -t 'dreamoutbox/crane-dev-vps:latest' .

docker compose -f docker-compose.dev.yml up -d --build
