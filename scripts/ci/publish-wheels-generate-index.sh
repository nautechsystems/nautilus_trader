#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

manifest="${PUBLISH_WHEELS_MANIFEST:?PUBLISH_WHEELS_MANIFEST is required}"
delete_manifest="${PUBLISH_WHEELS_DELETE_MANIFEST:?PUBLISH_WHEELS_DELETE_MANIFEST is required}"
index_file="${PUBLISH_WHEELS_INDEX_FILE:?PUBLISH_WHEELS_INDEX_FILE is required}"
bucket="${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME is required}"
prefix="${CLOUDFLARE_R2_PREFIX:?CLOUDFLARE_R2_PREFIX is required}"
endpoint="${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL is required}"
bucket_path="s3://${bucket}/${prefix}/"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
listing="${work_dir}/listing"
remote_files="${work_dir}/remote-files"
existing_index="${work_dir}/index.html"
hashes="${work_dir}/hashes"

aws s3 ls "$bucket_path" --endpoint-url="$endpoint" \
  --cli-connect-timeout 10 --cli-read-timeout 60 > "$listing"
awk 'NF >= 4 { print $4 }' "$listing" | sort -u > "$remote_files"

if grep -Fxq "index.html" "$remote_files"; then
  aws s3 cp "${bucket_path}index.html" "$existing_index" \
    --endpoint-url="$endpoint" --no-progress \
    --cli-connect-timeout 10 --cli-read-timeout 60
else
  : > "$existing_index"
fi

existing_hash() {
  local filename=$1

  awk -v filename="$filename" '
    BEGIN {
      prefix = "<a href=\"" filename "#sha256="
      suffix = "\">" filename "</a><br>"
    }
    index($0, prefix) == 1 && substr($0, length($0) - length(suffix) + 1) == suffix {
      hash = substr($0, length(prefix) + 1, length($0) - length(prefix) - length(suffix))
      if (hash ~ /^[a-f0-9]{64}$/) {
        print hash
      }
    }
  ' "$existing_index"
}

: > "$hashes"
while IFS= read -r filename; do
  if ! [[ "$filename" =~ ^nautilus_trader-[A-Za-z0-9_.+]+-[A-Za-z0-9_.-]+\.whl$ ]]; then
    continue
  fi
  if grep -Fxq "$filename" "$delete_manifest"; then
    continue
  fi

  hash="$(awk -F '\t' -v filename="$filename" '$3 == filename { print $1 }' "$manifest")"
  if [[ -z "$hash" ]]; then
    hash="$(existing_hash "$filename")"
    if [[ "$(printf '%s\n' "$hash" | sed '/^$/d' | wc -l | tr -d ' ')" -gt 1 ]]; then
      echo "Error: Existing index has duplicate links for ${filename}" >&2
      exit 1
    fi
  fi
  if [[ -z "$hash" ]]; then
    wheel="${work_dir}/${filename}"
    aws s3 cp "${bucket_path}${filename}" "$wheel" \
      --endpoint-url="$endpoint" --no-progress \
      --cli-connect-timeout 10 --cli-read-timeout 60
    hash="$(bash "${script_dir}/publish-wheels-sha256.bash" "$wheel")"
  fi

  printf '%s\t%s\n' "$filename" "$hash" >> "$hashes"
done < "$remote_files"

sort -t $'\t' -k1,1 -o "$hashes" "$hashes"
{
  echo '<!DOCTYPE html>'
  echo '<html><head><title>NautilusTrader Packages</title></head>'
  echo '<body><h1>Packages for nautilus_trader</h1>'
  while IFS=$'\t' read -r filename hash; do
    printf '<a href="%s#sha256=%s">%s</a><br>\n' "$filename" "$hash" "$filename"
  done < "$hashes"
  echo '</body></html>'
} > "$index_file"

echo "Generated exact package index with $(wc -l < "$hashes" | tr -d ' ') wheels"
