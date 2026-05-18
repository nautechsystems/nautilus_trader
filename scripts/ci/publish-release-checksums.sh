#!/usr/bin/env bash
# Publish SHA256 checksums for wheel and sdist assets on a GitHub release.
#
# Usage:
#   publish-release-checksums.sh [asset-dir]
#
# Required env:
#   TAG_NAME          - release tag to update
#   GITHUB_TOKEN      - for gh CLI auth
#   GITHUB_REPOSITORY - owner/repo (set by GitHub Actions)
#
# Optional env:
#   ATTESTATION_IDENTITY - expected OIDC identity for GitHub artifact attestations
#   ATTESTATION_ISSUER   - expected OIDC issuer for GitHub artifact attestations
set -euo pipefail

if [[ -z "${TAG_NAME:-}" ]]; then
  echo "::error::TAG_NAME not set."
  exit 1
fi
if [[ -z "${GITHUB_REPOSITORY:-}" ]]; then
  echo "::error::GITHUB_REPOSITORY not set."
  exit 1
fi
if [[ -z "${GITHUB_TOKEN:-}" && -z "${GH_TOKEN:-}" ]]; then
  echo "::error::GITHUB_TOKEN or GH_TOKEN not set."
  exit 1
fi
if ! command -v jq > /dev/null; then
  echo "::error::jq not found."
  exit 1
fi

retry_gh() {
  local description=$1
  shift

  local status=0
  for i in {1..5}; do
    "$@" && return 0
    status=$?
    echo "${description} failed (exit=${status}), retry (${i}/5)"
    sleep $((2 ** i))
  done

  echo "::error::${description} failed after retries."
  return "$status"
}

asset_dir="${1:-release-assets}"
case "$asset_dir" in
  "" | "/" | ".")
    echo "::error::Refusing unsafe asset directory: ${asset_dir:-<empty>}."
    exit 1
    ;;
esac

github_server_url="${GITHUB_SERVER_URL:-https://github.com}"
attestation_issuer="${ATTESTATION_ISSUER:-https://token.actions.githubusercontent.com}"
if [[ -n "${ATTESTATION_IDENTITY:-}" ]]; then
  attestation_identity="$ATTESTATION_IDENTITY"
else
  attestation_identity="${github_server_url}/${GITHUB_REPOSITORY}/.github/workflows/build.yml@refs/heads/master"
fi

rm -rf "$asset_dir"
mkdir -p "$asset_dir"
retry_gh "gh release download" gh release download "$TAG_NAME" \
  --repo "$GITHUB_REPOSITORY" \
  --dir "$asset_dir" \
  --pattern '*.whl' \
  --pattern '*.tar.gz'

(
  cd "$asset_dir"
  find . -maxdepth 1 \( -name '*.whl' -o -name '*.tar.gz' \) \
    -type f -printf '%P\0' |
    sort -z |
    xargs -0 -r sha256sum -- > SHA256SUMS
)

if [[ ! -s "$asset_dir/SHA256SUMS" ]]; then
  echo "::error::No wheel or sdist assets found for release $TAG_NAME."
  exit 1
fi

manifest_items=$(mktemp)
while read -r checksum artifact; do
  printf '%s  %s\n' "$checksum" "$artifact" > "$asset_dir/${artifact}.sha256"
  size=$(stat -c '%s' "$asset_dir/$artifact")
  jq -nc \
    --arg name "$artifact" \
    --arg sha256 "$checksum" \
    --argjson size "$size" \
    '{name: $name, sha256: $sha256, size: $size}' >> "$manifest_items"
done < "$asset_dir/SHA256SUMS"

jq -n \
  --arg tag "$TAG_NAME" \
  --arg generated_at "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
  --slurpfile artifacts "$manifest_items" \
  '{schema_version: 1, tag: $tag, generated_at: $generated_at, artifacts: $artifacts}' \
  > "$asset_dir/dist-manifest.json"

upload_assets=("$asset_dir/SHA256SUMS" "$asset_dir/dist-manifest.json")
while IFS= read -r -d '' checksum_file; do
  upload_assets+=("$checksum_file")
done < <(find "$asset_dir" -maxdepth 1 -type f -name '*.sha256' -print0 | sort -z)

retry_gh "gh release upload" gh release upload "$TAG_NAME" "${upload_assets[@]}" \
  --clobber \
  --repo "$GITHUB_REPOSITORY"

body_file=$(mktemp)
retry_gh "gh release view" gh release view "$TAG_NAME" \
  --repo "$GITHUB_REPOSITORY" \
  --json body \
  --jq '.body // ""' > "$body_file"

clean_body_file=$(mktemp)

# shellcheck disable=SC2016
awk '
  { marker = $0; sub(/\r$/, "", marker) }
  marker == "<!-- release-checksums:start -->" { skip = 1; next }
  marker == "<!-- release-checksums:end -->" { skip = 0; next }
  !skip { print }
' "$body_file" > "$clean_body_file"
mv "$clean_body_file" "$body_file"

{
  printf '\n<!-- release-checksums:start -->\n'
  printf '## Artifact checksums\n\n'
  printf "SHA256 checksums are attached as \`SHA256SUMS\`, per-asset \`.sha256\` files, "
  printf "and \`dist-manifest.json\`.\n\n"
  printf '| Artifact | SHA256 |\n'
  printf '| --- | --- |\n'
  asset_url_base="${github_server_url}/${GITHUB_REPOSITORY}/releases/download/${TAG_NAME}"
  while read -r checksum artifact; do
    printf '| [%s](%s/%s) | %s |\n' "$artifact" "$asset_url_base" "$artifact" "$checksum"
  done < "$asset_dir/SHA256SUMS"
  printf '\n### Verify provenance\n\n'
  printf 'After downloading an artifact, verify its GitHub artifact attestation:\n\n'
  printf '```bash\n'
  printf 'gh attestation verify <artifact> \\\n'
  printf '  --repo %s \\\n' "$GITHUB_REPOSITORY"
  printf '  --cert-identity "%s" \\\n' "$attestation_identity"
  printf '  --cert-oidc-issuer %s\n' "$attestation_issuer"
  printf '```\n\n'
  printf '<!-- release-checksums:end -->\n'
} >> "$body_file"

retry_gh "gh release edit" gh release edit "$TAG_NAME" \
  --repo "$GITHUB_REPOSITORY" \
  --notes-file "$body_file"
