#!/bin/sh
# Bootstrap development environment by setting up Docker containers and SeaweedFS S3 storage.
#
# Usage:
#   ./dev-docker/bootstrap.sh
#   or from within dev-docker:
#   ./bootstrap.sh

set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

echo "Setting up Docker containers..."
./setup-docker.sh

echo "Setting up SeaweedFS S3..."
./setup-s3.sh

echo "Bootstrap completed successfully."
