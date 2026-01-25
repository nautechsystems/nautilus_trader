#!/bin/bash
# Pulls a Docker image with retry and exponential backoff.
# Usage: docker-pull-retry.sh <image> [max_attempts]
#
# ECR Public has rate limits (1/sec unauthenticated, 10/sec authenticated),
# so retries are necessary when running concurrent CI jobs.

set -euo pipefail

IMAGE="${1:?Usage: docker-pull-retry.sh <image> [max_attempts]}"
MAX_ATTEMPTS="${2:-5}"

for i in $(seq 1 "$MAX_ATTEMPTS"); do
  if docker pull "$IMAGE"; then
    exit 0
  fi
  if [ "$i" -eq "$MAX_ATTEMPTS" ]; then
    echo "ERROR: Failed to pull $IMAGE after $MAX_ATTEMPTS attempts"
    exit 1
  fi
  WAIT=$((i * 30))
  echo "Pull attempt $i failed, waiting ${WAIT}s before retry..."
  sleep "$WAIT"
done
