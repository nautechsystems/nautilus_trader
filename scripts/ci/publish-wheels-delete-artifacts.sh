#!/usr/bin/env bash
set -euo pipefail

echo "Fetching artifacts for the current run"

response=$(curl -s -H "Authorization: Bearer ${GITHUB_TOKEN}" \
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
  response=$(curl -s -o /dev/null -w "%{http_code}" -X DELETE \
    -H "Authorization: Bearer ${GITHUB_TOKEN}" \
    -H "Accept: application/vnd.github+json" \
    "https://api.github.com/repos/${GITHUB_REPOSITORY}/actions/artifacts/$id")

  if [ "$response" -ne 204 ]; then
    echo "Warning: Failed to delete artifact ID $id (HTTP $response)"
  else
    echo "Successfully deleted artifact ID $id"
  fi
done

echo "Artifact deletion process completed"
