#!/bin/sh

cd demo
go build
cd ..

docker compose -f 'docker-compose.dev.yml' down
docker compose -f 'docker-compose.dev.yml' up -d --build

clear && RUST_BACKTRACE=1 cargo nextest run test_deploy -- --no-capture

sleep 20

echo "=================="
echo "DEPLOY 2ND"
echo "=================="

RUST_BACKTRACE=1 cargo nextest run test_deploy -- --no-capture
