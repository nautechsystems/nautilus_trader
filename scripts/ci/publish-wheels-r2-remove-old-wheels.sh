#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 || ("$1" != "plan" && "$1" != "apply") ]]; then
  echo "Usage: $0 <plan|apply>" >&2
  exit 1
fi

operation=$1
delete_manifest="${PUBLISH_WHEELS_DELETE_MANIFEST:?PUBLISH_WHEELS_DELETE_MANIFEST is required}"
version="${PUBLISH_WHEEL_VERSION:?PUBLISH_WHEEL_VERSION is required}"
matrix="${PUBLISH_WHEEL_MATRIX:?PUBLISH_WHEEL_MATRIX is required}"
bucket="${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME is required}"
prefix="${CLOUDFLARE_R2_PREFIX:?CLOUDFLARE_R2_PREFIX is required}"
endpoint="${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL is required}"
bucket_path="s3://${bucket}/${prefix}/"

apply_deletions() {
  local filename
  local success
  local attempt

  if [[ ! -s "$delete_manifest" ]]; then
    echo "No old wheels selected for deletion"
    return
  fi

  while IFS= read -r filename; do
    if ! [[ "$filename" =~ ^nautilus_trader-[A-Za-z0-9_.+]+-[A-Za-z0-9_.-]+\.whl$ ]]; then
      echo "Error: Refusing unsafe wheel deletion target ${filename}" >&2
      exit 1
    fi

    success=false
    attempt=1
    while [[ "$attempt" -le 5 ]]; do
      if aws s3 rm "${bucket_path}${filename}" --endpoint-url="$endpoint" \
        --cli-connect-timeout 10 --cli-read-timeout 60; then
        success=true
        break
      fi

      if [[ "$attempt" -lt 5 ]]; then
        echo "Delete failed for ${filename}, retrying (${attempt}/5)"
        sleep $((2 ** attempt))
      fi
      attempt=$((attempt + 1))
    done

    if [[ "$success" != "true" ]]; then
      echo "Error: Failed to delete ${filename} after 5 attempts" >&2
      exit 1
    fi
    echo "Deleted old wheel ${filename}"
  done < "$delete_manifest"
}

if [[ "$operation" == "apply" ]]; then
  apply_deletions
  exit 0
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
listing="${work_dir}/listing"
files="${work_dir}/files"
planned="${work_dir}/planned"

aws s3 ls "$bucket_path" --endpoint-url="$endpoint" \
  --cli-connect-timeout 10 --cli-read-timeout 60 > "$listing"
awk 'NF >= 4 { print $4 }' "$listing" | sort -u > "$files"
: > "$planned"

if [[ "$matrix" == "development" ]]; then
  while IFS= read -r filename; do
    if [[ "$filename" =~ ^nautilus_trader-([^-]+\.dev[0-9]{8}\+[0-9]+)-.*\.whl$ ]] &&
      [[ "${BASH_REMATCH[1]}" != "$version" ]]; then
      echo "$filename" >> "$planned"
    fi
  done < "$files"
elif [[ "$matrix" == "nightly" ]]; then
  lookback="${NIGHTLY_LOOKBACK:-30}"
  if ! [[ "$lookback" =~ ^[1-9][0-9]*$ ]]; then
    echo "Error: NIGHTLY_LOOKBACK must be a positive integer" >&2
    exit 1
  fi

  candidates="${work_dir}/nightly"
  : > "$candidates"
  while IFS= read -r filename; do
    if [[ "$filename" =~ ^nautilus_trader-.*(a|\.dev)([0-9]{8})-cp[0-9]+-cp[0-9]+-([A-Za-z0-9_.]+)\.whl$ ]]; then
      printf '%s\t%s\t%s\n' "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}" "$filename" >> "$candidates"
    fi
  done < "$files"

  if [[ -s "$candidates" ]]; then
    cut -f2 "$candidates" | sort -u | while IFS= read -r platform; do
      keep="${work_dir}/keep-${platform}"
      awk -F '\t' -v platform="$platform" '$2 == platform { print $1 }' "$candidates" |
        sort -n -u | tail -n "$lookback" > "$keep"
      while IFS=$'\t' read -r date candidate_platform filename; do
        if [[ "$candidate_platform" == "$platform" ]] && ! grep -Fxq "$date" "$keep"; then
          echo "$filename" >> "$planned"
        fi
      done < "$candidates"
    done
  fi
elif [[ "$matrix" != "stable" ]]; then
  echo "Error: Unknown wheel matrix ${matrix}" >&2
  exit 1
fi

sort -u "$planned" > "$delete_manifest"
echo "Planned $(wc -l < "$delete_manifest" | tr -d ' ') old wheel deletions"
