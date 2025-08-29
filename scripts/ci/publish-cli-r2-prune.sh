#!/usr/bin/env bash
set -euo pipefail

echo "Pruning old CLI versions in Cloudflare R2..."

PREFIX=${CLOUDFLARE_R2_PREFIX:-cli/nautilus-cli}
BUCKET=${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME not set}
R2_URL=${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL not set}
BRANCH_NAME="${GITHUB_REF_NAME:-}"

KEEP_DEVELOP=1
KEEP_NIGHTLY=30

# Collect version directories (exclude 'latest')
mapfile -t dirs < <(aws s3 ls "s3://${BUCKET}/${PREFIX}/" --endpoint-url="$R2_URL" | awk '/PRE/ {print $2}' | sed 's:/$::' | grep -v '^latest$' || true)

if [[ ${#dirs[@]} -eq 0 ]]; then
  echo "No version directories found; nothing to prune."
  exit 0
fi

# Sort lexicographically; dev/nightly names include date and generally increase
IFS=$'\n' sorted=($(sort <<<"${dirs[*]}"))
unset IFS

keep=$KEEP_DEVELOP
if [[ "$BRANCH_NAME" == "nightly" ]]; then
  keep=$KEEP_NIGHTLY
fi

to_delete=()
if (( ${#sorted[@]} > keep )); then
  count=$((${#sorted[@]} - keep))
  for ((i=0; i<count; i++)); do
    to_delete+=("${sorted[$i]}")
  done
fi

if [[ ${#to_delete[@]} -eq 0 ]]; then
  echo "Nothing to prune; keeping last $keep versions."
  exit 0
fi

echo "Deleting ${#to_delete[@]} old version directories:"
printf '  %s\n' "${to_delete[@]}"

for d in "${to_delete[@]}"; do
  echo "Removing s3://${BUCKET}/${PREFIX}/${d}/"
  aws s3 rm "s3://${BUCKET}/${PREFIX}/${d}/" --recursive --endpoint-url="$R2_URL" || echo "Warning: failed to delete $d"
done

echo "Prune complete"

