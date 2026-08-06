#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

manifest="${PUBLISH_WHEELS_MANIFEST:?PUBLISH_WHEELS_MANIFEST is required}"
version="${PUBLISH_WHEEL_VERSION:?PUBLISH_WHEEL_VERSION is required}"
matrix="${PUBLISH_WHEEL_MATRIX:?PUBLISH_WHEEL_MATRIX is required}"
bucket="${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME is required}"
prefix="${CLOUDFLARE_R2_PREFIX:?CLOUDFLARE_R2_PREFIX is required}"
endpoint="${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL is required}"
skip_file="${PUBLISH_WHEELS_SKIP_FILE:?PUBLISH_WHEELS_SKIP_FILE is required}"
bucket_path="s3://${bucket}/${prefix}/"

if [[ ! -s "$manifest" ]]; then
  echo "Error: Wheel manifest is empty or missing" >&2
  exit 1
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
listing="${work_dir}/listing"
remote_files="${work_dir}/remote-files"

aws s3 ls "$bucket_path" --endpoint-url="$endpoint" \
  --cli-connect-timeout 10 --cli-read-timeout 60 > "$listing"
awk 'NF >= 4 { print $4 }' "$listing" | sort -u > "$remote_files"

manifest_hash() {
  awk -F '\t' -v filename="$1" '$3 == filename { print $1 }' "$manifest"
}

verify_remote() {
  local filename=$1
  local expected_hash=$2
  local destination="${work_dir}/${filename}"
  local actual_hash

  aws s3 cp "${bucket_path}${filename}" "$destination" \
    --endpoint-url="$endpoint" --no-progress \
    --cli-connect-timeout 10 --cli-read-timeout 60
  actual_hash="$(bash "${script_dir}/publish-wheels-sha256.bash" "$destination")"
  if [[ "$actual_hash" != "$expected_hash" ]]; then
    echo "Error: Existing R2 object ${filename} has SHA-256 ${actual_hash}, expected ${expected_hash}" >&2
    exit 1
  fi
}

if [[ "$matrix" == "development" ]]; then
  if ! [[ "$version" =~ \.dev([0-9]{8})\+([0-9]+)$ ]]; then
    echo "Error: Invalid development version ${version}" >&2
    exit 1
  fi
  current_date="${BASH_REMATCH[1]}"
  current_run="${BASH_REMATCH[2]}"

  while IFS= read -r filename; do
    if [[ "$filename" =~ ^nautilus_trader-[^-]+\.dev([0-9]{8})\+([0-9]+)-.*\.whl$ ]]; then
      remote_date="${BASH_REMATCH[1]}"
      remote_run="${BASH_REMATCH[2]}"
      if [[ "$remote_date" > "$current_date" ]] ||
        [[ "$remote_date" == "$current_date" && "$remote_run" -gt "$current_run" ]]; then
        echo "More recent development wheels (${remote_date}+${remote_run}) already exist; skipping upload of ${version}"
        : > "$skip_file"
        exit 0
      fi
    fi
  done < "$remote_files"
fi

current_prefix="nautilus_trader-${version}-"
while IFS= read -r filename; do
  if [[ "$filename" == "${current_prefix}"*.whl ]]; then
    expected_hash="$(manifest_hash "$filename")"
    if [[ -z "$expected_hash" ]]; then
      echo "Error: R2 contains unexpected wheel for current version: ${filename}" >&2
      exit 1
    fi
    verify_remote "$filename" "$expected_hash"
  fi
done < "$remote_files"

while IFS=$'\t' read -r expected_hash _ filename; do
  if grep -Fxq "$filename" "$remote_files"; then
    echo "R2 object already matches: ${filename}"
    continue
  fi

  source="dist/${filename}"
  destination="${bucket_path}${filename}"
  success=false
  attempt=1
  while [[ "$attempt" -le 5 ]]; do
    if aws s3 cp "$source" "$destination" \
      --endpoint-url="$endpoint" \
      --content-type "application/zip" \
      --no-progress; then
      success=true
      break
    fi

    if [[ "$attempt" -lt 5 ]]; then
      echo "Upload failed for ${filename}, retrying (${attempt}/5)"
      sleep $((2 ** attempt))
    fi
    attempt=$((attempt + 1))
  done

  if [[ "$success" != "true" ]]; then
    echo "Error: Failed to upload ${filename} after 5 attempts" >&2
    exit 1
  fi

  verify_remote "$filename" "$expected_hash"
  echo "Uploaded and verified ${filename}"
done < "$manifest"
