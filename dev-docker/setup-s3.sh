#!/bin/sh
# Wait for SeaweedFS S3 to become ready and ensure required buckets exist.
#
# Usage:
#   ./setup-s3.sh

set -e

echo "Waiting for SeaweedFS S3 to start..."
attempt=1
until curl -s -o /dev/null -w "%{http_code}" http://localhost:9000/ | grep -q '^[2-4][0-9][0-9]$'; do
  if [ "$attempt" -ge 30 ]; then
    echo "Error: SeaweedFS S3 failed to start within 30 seconds" >&2
    exit 1
  fi
  attempt=$((attempt + 1))
  sleep 1
done

echo "SeaweedFS S3 is ready. Ensuring bucket 'crane1' exists..."

# Create bucket crane1 if it does not already exist
if ! docker exec s3 sh -c 'echo "s3.bucket.list" | weed shell' 2>/dev/null | grep -q '\bcrane1\b'; then
  docker exec s3 sh -c 'echo "s3.bucket.create -name crane1" | weed shell'
fi

echo "SeaweedFS bucket 'crane1' is ready."
