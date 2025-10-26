#!/usr/bin/env bash
set -euo pipefail

echo "Fetching artifacts for the current run"

response=$(curl -sS --retry 5 --retry-delay 2 --retry-all-errors --connect-timeout 5 --max-time 60 \
  -H "Authorization: Bearer ${GITHUB_TOKEN}" \
  -H "Accept: application/vnd.github+json" \
  "https://api.github.com/repos/${GITHUB_REPOSITORY}/actions/runs/${GITHUB_RUN_ID}/artifacts")

# Extract artifact IDs
ids=$(echo "$response" | jq -r '.artifacts[].id // empty')
if [[ -z "$ids" ]]; then
  echo "No artifact IDs found for the current run"
  exit 0
fi

echo "Artifact IDs to delete: $ids"

# Delete artifacts
for id in $ids; do
  echo "Deleting artifact ID $id"
  attempts=0
  while true; do
    attempts=$((attempts + 1))
    status=$(curl -sS --connect-timeout 5 --max-time 60 -o /dev/null -w "%{http_code}" -X DELETE \
      -H "Authorization: Bearer ${GITHUB_TOKEN}" \
      -H "Accept: application/vnd.github+json" \
      "https://api.github.com/repos/${GITHUB_REPOSITORY}/actions/artifacts/$id")

    if [ "$status" -eq 204 ]; then
      echo "Successfully deleted artifact ID $id"
      break
    fi

    # 404 means already gone – treat as success
    if [ "$status" -eq 404 ]; then
      echo "Artifact ID $id already deleted (404)"
      break
    fi

    # Retry on 5xx up to 5 attempts; fail on persistent 4xx
    if [ "$status" -ge 500 ] && [ $attempts -lt 5 ]; then
      echo "Delete failed for $id (HTTP $status), retrying ($attempts/5)..."
      sleep $((2 ** attempts))
      continue
    fi

    echo "Failed to delete artifact ID $id (HTTP $status)"
    exit 1
  done
done

echo "Artifact deletion process completed"
