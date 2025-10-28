#!/usr/bin/env bash
set -euo pipefail

echo "Generating package index..."

bucket_path="s3://${CLOUDFLARE_R2_BUCKET_NAME}/${CLOUDFLARE_R2_PREFIX:-simple/nautilus-trader}/"
index_file="index.html"

# Create a temporary directory for downloads
TEMP_DIR=$(mktemp -d)
trap 'rm -rf "$TEMP_DIR"' EXIT

# Download existing index.html if it exists
if aws s3 ls "${bucket_path}${index_file}" --endpoint-url="${CLOUDFLARE_R2_URL}" > /dev/null 2>&1; then
  echo "Existing index.html found, downloading..."
  aws s3 cp "${bucket_path}${index_file}" . --endpoint-url="${CLOUDFLARE_R2_URL}"
else
  echo "No existing index.html found, creating a new one..."
  echo '<!DOCTYPE html>' > "$index_file"
  echo '<html><head><title>NautilusTrader Packages</title></head>' >> "$index_file"
  echo '<body><h1>Packages for nautilus_trader</h1></body></html>' >> "$index_file"
fi

# Extract existing hashes from index.html
declare -A existing_hashes=()
if [[ -f "$index_file" ]]; then
  echo "Extracting existing hashes from index.html..."
  while IFS= read -r line; do
    if [[ $line =~ href=\"([^\"#]+)#sha256=([a-f0-9]{64}) ]]; then
      file="${BASH_REMATCH[1]}"
      hash="${BASH_REMATCH[2]}"
      existing_hashes["$file"]="$hash"
      echo "Found existing hash for $file"
    fi
  done < "$index_file"
fi

# Create new index.html
echo '<!DOCTYPE html>' > "${index_file}.new"
echo '<html><head><title>NautilusTrader Packages</title></head>' >> "${index_file}.new"
echo '<body><h1>Packages for nautilus_trader</h1>' >> "${index_file}.new"

# Map to store final hashes we'll use
declare -A final_hashes=()

# First, calculate hashes for all new/updated wheels
# These will override any existing hashes for the same filename
for file in dist/nautilus_trader-*.whl; do
  if [[ -f "$file" ]]; then
    filename=$(basename "$file")
    hash=$(sha256sum "$file" | awk '{print $1}')
    final_hashes["$filename"]="$hash"
    echo "Calculated hash for new/updated wheel $filename: $hash"
  fi
done

# Get list of all wheel files in bucket
existing_files=$(aws s3 ls "${bucket_path}" --endpoint-url="${CLOUDFLARE_R2_URL}" | grep 'nautilus_trader-.*\.whl$' | awk '{print $4}')

# For existing files, use hash from index if we don't have a new one
for file in $existing_files; do
  if [[ -z "${final_hashes[$file]:-}" ]]; then # Only if we don't have a new hash
    if [[ -n "${existing_hashes[$file]:-}" ]]; then
      final_hashes["$file"]="${existing_hashes[$file]}"
      echo "Using existing hash for $file: ${existing_hashes[$file]}"
    else
      # Only download and calculate if we have no hash at all
      echo "No existing hash found, downloading wheel to compute hash for $file..."
      tmpfile="$TEMP_DIR/$file"
      if aws s3 cp "${bucket_path}${file}" "$tmpfile" \
        --endpoint-url="${CLOUDFLARE_R2_URL}"; then
        hash=$(sha256sum "$tmpfile" | awk '{print $1}')
        final_hashes["$file"]="$hash"
        echo "Calculated hash for missing file $file: $hash"
      else
        echo "Warning: Could not download $file for hashing, skipping..."
      fi
    fi
  fi
done

# Sort files for consistent ordering
readarray -t sorted_files < <(printf '%s\n' "${!final_hashes[@]}" | sort)

# Generate index entries using sorted list
for file in "${sorted_files[@]}"; do
  hash="${final_hashes[$file]}"
  escaped_file=$(echo "$file" | sed 's/&/\&amp;/g; s/</\&lt;/g; s/>/\&gt;/g; s/"/\&quot;/g; s/'"'"'/\&#39;/g')
  echo "<a href=\"$escaped_file#sha256=$hash\">$escaped_file</a><br>" >> "${index_file}.new"
done

echo '</body></html>' >> "${index_file}.new"
mv "${index_file}.new" "$index_file"
echo "Index generation complete"
