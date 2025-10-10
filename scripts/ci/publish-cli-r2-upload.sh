#!/usr/bin/env bash
set -euo pipefail

echo "Publishing CLI binaries to Cloudflare R2..."

PREFIX=${CLOUDFLARE_R2_PREFIX:-cli/nautilus-cli}
BUCKET=${CLOUDFLARE_R2_BUCKET_NAME:?CLOUDFLARE_R2_BUCKET_NAME not set}
R2_URL=${CLOUDFLARE_R2_URL:?CLOUDFLARE_R2_URL not set}

BRANCH_NAME="${GITHUB_REF_NAME:-}"

get_base_version() {
  # Extract workspace.package.version from Cargo.toml
  awk '
    $0 ~ /^\[workspace\.package\]/ { in_block=1; next }
    in_block && $0 ~ /^\[/ { in_block=0 }
    in_block && $0 ~ /^version *=/ { gsub(/"/, "", $3); print $3; exit }
  ' Cargo.toml
}

BASE_VERSION="$(get_base_version)"
if [[ -z "$BASE_VERSION" ]]; then
  echo "Failed to read base version from Cargo.toml" >&2
  exit 1
fi

SUFFIX=""
if [[ "$BRANCH_NAME" == "develop" ]]; then
  SUFFIX=".dev$(date +%Y%m%d)+${GITHUB_RUN_NUMBER:-0}"
elif [[ "$BRANCH_NAME" == "nightly" ]]; then
  SUFFIX="a$(date +%Y%m%d)"
fi
VERSION="${BASE_VERSION}${SUFFIX}"

echo "Base version: $BASE_VERSION"
echo "Computed version: $VERSION"

ART_DIR="dist/cli"
if [[ ! -d "$ART_DIR" ]]; then
  echo "Artifacts directory not found: $ART_DIR" >&2
  exit 1
fi

mkdir -p "$ART_DIR/versioned" "$ART_DIR/latest"

# Normalize artifacts into latest names and prepare versioned copies
mapfile -t FILES < <(find "$ART_DIR" -maxdepth 2 -type f \( -name 'nautilus-*.tar.gz' -o -name 'nautilus-*.zip' \) | sort)
if [[ ${#FILES[@]} -eq 0 ]]; then
  echo "No CLI archives found under $ART_DIR" >&2
  exit 1
fi

echo "Found ${#FILES[@]} artifacts:"
printf '  %s\n' "${FILES[@]}"

# Copy to latest/ (rename to flat form) and compute checksums
LATEST_LIST=()
for path in "${FILES[@]}"; do
  fname="$(basename "$path")" # nautilus-<target>.<ext>
  # Ensure expected pattern
  if [[ ! "$fname" =~ ^nautilus-(.+)\.(tar\.gz|zip)$ ]]; then
    echo "Skipping unexpected file name: $fname" >&2
    continue
  fi
  cp -f "$path" "$ART_DIR/latest/$fname"
  LATEST_LIST+=("$ART_DIR/latest/$fname")
done

# checksums for latest
(
  cd "$ART_DIR/latest"
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum nautilus-* > checksums.txt
  else
    for f in nautilus-*; do shasum -a 256 "$f"; done | awk '{print $1"  "$2}' > checksums.txt
  fi
)

# Create manifest.json for latest
MANIFEST="$ART_DIR/latest/manifest.json"
BASE_HTTP="https://packages.nautechsystems.io/${PREFIX}/latest"
{
  echo '{'
  echo '  "name": "nautilus",'
  echo "  \"version\": \"$VERSION\","
  echo '  "targets": {'
  first=1
  while read -r line; do
    hash=$(echo "$line" | awk '{print $1}')
    file=$(echo "$line" | awk '{print $2}')
    # strip leading ./ if present
    file=${file#./}
    # extract target from 'nautilus-<target>.<ext>'
    target=$(echo "$file" | sed -E 's/^nautilus-(.+)\.(tar\.gz|zip)$/\1/')
    url="${BASE_HTTP}/${file}#sha256=${hash}"
    fmt="$(echo "$file" | sed -nE 's/^.*\.(tar\.gz|zip)$/\1/p')"
    if [[ $first -eq 0 ]]; then echo ','; fi
    echo "    \"$target\": { \"url\": \"$url\", \"sha256\": \"$hash\", \"format\": \"$fmt\" }"
    first=0
  done < "$ART_DIR/latest/checksums.txt"
  echo ''
  echo '  }'
  echo '}'
} > "$MANIFEST"

# Prepare versioned filenames and upload both versioned and latest
LATEST_CACHE_CONTROL="no-cache, max-age=60, must-revalidate"

for latest_file in "${LATEST_LIST[@]}"; do
  b=$(basename "$latest_file")
  ext="${b##*.}"
  if [[ "$ext" == "gz" ]]; then ext="tar.gz"; fi
  target="$(echo "$b" | sed -E 's/^nautilus-(.+)\.(tar\.gz|zip)$/\1/')"
  versioned_fname="nautilus-cli-${VERSION}-${target}.${ext}"
  cp -f "$latest_file" "$ART_DIR/versioned/$versioned_fname"
  echo "Uploading versioned: $versioned_fname"
  set +e
  success=false
  for i in {1..5}; do
    aws s3 cp "$ART_DIR/versioned/$versioned_fname" "s3://${BUCKET}/${PREFIX}/${VERSION}/${versioned_fname}" \
      --endpoint-url="$R2_URL" --content-type application/octet-stream \
      --cli-connect-timeout 10 --cli-read-timeout 60
    status=$?
    if [ $status -eq 0 ]; then
      success=true
      break
    else
      echo "Upload versioned failed (exit=$status), retry ($i/5)"
      sleep $((2 ** i))
    fi
  done
  set -e
  if [ "$success" = false ]; then
    echo "Failed to upload versioned artifact after retries"
    exit 1
  fi
  echo "Uploading latest: $b"
  set +e
  success=false
  for i in {1..5}; do
    aws s3 cp "$latest_file" "s3://${BUCKET}/${PREFIX}/latest/${b}" \
      --endpoint-url="$R2_URL" --content-type application/octet-stream --cache-control "$LATEST_CACHE_CONTROL" \
      --cli-connect-timeout 10 --cli-read-timeout 60
    status=$?
    if [ $status -eq 0 ]; then
      success=true
      break
    else
      echo "Upload latest failed (exit=$status), retry ($i/5)"
      sleep $((2 ** i))
    fi
  done
  set -e
  if [ "$success" = false ]; then
    echo "Failed to upload latest artifact after retries"
    exit 1
  fi
done

echo "Uploading latest checksums and manifest"
set +e
success=false
for i in {1..5}; do
  aws s3 cp "$ART_DIR/latest/checksums.txt" "s3://${BUCKET}/${PREFIX}/latest/checksums.txt" \
    --endpoint-url="$R2_URL" --content-type text/plain --cache-control "$LATEST_CACHE_CONTROL" \
    --cli-connect-timeout 10 --cli-read-timeout 60
  status=$?
  if [ $status -eq 0 ]; then
    success=true
    break
  else
    echo "Upload checksums failed (exit=$status), retry ($i/5)"
    sleep $((2 ** i))
  fi
done
set -e
if [ "$success" = false ]; then
  echo "Failed to upload checksums.txt after retries"
  exit 1
fi

set +e
success=false
for i in {1..5}; do
  aws s3 cp "$MANIFEST" "s3://${BUCKET}/${PREFIX}/latest/manifest.json" \
    --endpoint-url="$R2_URL" --content-type application/json --cache-control "$LATEST_CACHE_CONTROL" \
    --cli-connect-timeout 10 --cli-read-timeout 60
  status=$?
  if [ $status -eq 0 ]; then
    success=true
    break
  else
    echo "Upload manifest failed (exit=$status), retry ($i/5)"
    sleep $((2 ** i))
  fi
done
set -e
if [ "$success" = false ]; then
  echo "Failed to upload manifest.json after retries"
  exit 1
fi

echo "Publish complete"
