#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

manifest="${PUBLISH_WHEELS_MANIFEST:?PUBLISH_WHEELS_MANIFEST is required}"
index_file="${PUBLISH_WHEELS_INDEX_FILE:?PUBLISH_WHEELS_INDEX_FILE is required}"
version="${PUBLISH_WHEEL_VERSION:?PUBLISH_WHEEL_VERSION is required}"
index_url="${PUBLISH_WHEELS_INDEX_URL:?PUBLISH_WHEELS_INDEX_URL is required}"
bucket="${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME is required}"
prefix="${CLOUDFLARE_R2_PREFIX:?CLOUDFLARE_R2_PREFIX is required}"
endpoint="${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL is required}"
bucket_path="s3://${bucket}/${prefix}/"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

while IFS=$'\t' read -r expected_hash expected_size filename; do
  object="${work_dir}/${filename}"
  aws s3 cp "${bucket_path}${filename}" "$object" \
    --endpoint-url="$endpoint" --no-progress \
    --cli-connect-timeout 10 --cli-read-timeout 60
  actual_hash="$(bash "${script_dir}/publish-wheels-sha256.bash" "$object")"
  actual_size="$(wc -c < "$object" | tr -d ' ')"
  if [[ "$actual_hash" != "$expected_hash" || "$actual_size" != "$expected_size" ]]; then
    echo "Error: Uploaded object verification failed for ${filename}" >&2
    exit 1
  fi
done < "$manifest"

origin_index="${work_dir}/origin-index.html"
aws s3 cp "${bucket_path}index.html" "$origin_index" \
  --endpoint-url="$endpoint" --no-progress \
  --cli-connect-timeout 10 --cli-read-timeout 60
if ! cmp -s "$index_file" "$origin_index"; then
  echo "Error: R2 index does not match the generated index" >&2
  exit 1
fi

while IFS=$'\t' read -r hash _ filename; do
  link="<a href=\"${filename}#sha256=${hash}\">${filename}</a><br>"
  if [[ "$(grep -Fxc "$link" "$origin_index")" -ne 1 ]]; then
    echo "Error: R2 index does not contain one exact link for ${filename}" >&2
    exit 1
  fi
done < "$manifest"

public_index="${work_dir}/public-index.html"
public_matches=false
attempt=1
while [[ "$attempt" -le 5 ]]; do
  if curl -fsSLo "$public_index" "${index_url%/}/nautilus-trader/" &&
    cmp -s "$index_file" "$public_index"; then
    public_matches=true
    break
  fi

  if [[ "$attempt" -lt 5 ]]; then
    echo "Public index is not current, retrying (${attempt}/5)"
    sleep $((2 ** attempt))
  fi
  attempt=$((attempt + 1))
done

if [[ "$public_matches" != "true" ]]; then
  echo "Error: Public index did not match the generated index" >&2
  exit 1
fi

while IFS=$'\t' read -r hash _ filename; do
  link="<a href=\"${filename}#sha256=${hash}\">${filename}</a><br>"
  if [[ "$(grep -Fxc "$link" "$public_index")" -ne 1 ]]; then
    echo "Error: Public index does not contain one exact link for ${filename}" >&2
    exit 1
  fi
done < "$manifest"

venv="${work_dir}/install"
uv venv --python 3.13 "$venv"
UV_NO_CACHE=1 uv pip install \
  --python "${venv}/bin/python" \
  --index-url "$index_url" \
  "nautilus-trader==${version}"

echo "Verified wheel objects, index links, public index, and installation"
