#!/bin/sh

set -e

# Wait for MinIO to start
echo "Waiting for MinIO to start..."
attempt=1
until curl -s -f http://localhost:9000/minio/health/ready >/dev/null 2>&1; do
  if [ "$attempt" -ge 30 ]; then
    echo "Error: MinIO failed to start within 30 seconds" >&2
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 1
done

echo "MinIO is ready. Setting up alias and bucket..."

# Configure mc inside the s3 container
docker exec s3 mc alias set local http://localhost:9000 miniominio miniominio

# Create bucket crane1 if it does not exist
docker exec s3 mc mb --ignore-existing local/crane1

echo "✓ MinIO bucket 'crane1' successfully created or already exists."
