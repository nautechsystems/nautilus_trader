#!/usr/bin/env bash
# Publish SHA256 checksums for wheel and sdist assets on a GitHub release.
#
# Usage:
#   publish-release-checksums.sh [--generate-only|--publish-existing] [asset-dir]
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
if ! command -v gh > /dev/null; then
  echo "::error::gh not found."
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

sha256_file() {
  local path=$1

  if command -v sha256sum > /dev/null; then
    sha256sum "$path" | awk '{ print $1 }'
  elif command -v shasum > /dev/null; then
    shasum -a 256 "$path" | awk '{ print $1 }'
  else
    echo "::error::sha256sum or shasum not found."
    exit 1
  fi
}

file_size() {
  local path=$1
  local size

  if size="$(stat -c '%s' "$path" 2> /dev/null)"; then
    printf '%s\n' "$size"
  elif size="$(stat -f '%z' "$path" 2> /dev/null)"; then
    printf '%s\n' "$size"
  else
    echo "::error::Failed to stat file size for $path."
    exit 1
  fi
}

release_artifacts() {
  local artifact

  for artifact in "$asset_dir"/*.whl "$asset_dir"/*.tar.gz; do
    [[ -f "$artifact" ]] || continue
    printf '%s\n' "${artifact##*/}"
  done | sort
}

checksum_files() {
  local checksum_file

  for checksum_file in "$asset_dir"/*.sha256; do
    [[ -f "$checksum_file" ]] || continue
    printf '%s\n' "$checksum_file"
  done | sort
}

mode="publish"
case "${1:-}" in
  "")
    ;;
  --generate-only)
    mode="generate"
    shift
    ;;
  --publish-existing)
    mode="publish_existing"
    shift
    ;;
  *)
    if [[ "${1:-}" == -* ]]; then
      echo "::error::Usage: publish-release-checksums.sh [--generate-only|--publish-existing] [asset-dir]"
      exit 1
    fi
    ;;
esac

if [[ "$#" -gt 1 ]]; then
  echo "::error::Usage: publish-release-checksums.sh [--generate-only|--publish-existing] [asset-dir]"
  exit 1
fi

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

generate_checksum_assets() {
  rm -rf "$asset_dir"
  mkdir -p "$asset_dir"
  retry_gh "gh release download" gh release download "$TAG_NAME" \
    --repo "$GITHUB_REPOSITORY" \
    --dir "$asset_dir" \
    --pattern '*.whl' \
    --pattern '*.tar.gz'

  : > "$asset_dir/SHA256SUMS"
  while IFS= read -r artifact; do
    printf '%s  %s\n' "$(sha256_file "$asset_dir/$artifact")" "$artifact" \
      >> "$asset_dir/SHA256SUMS"
  done < <(release_artifacts)

  if [[ ! -s "$asset_dir/SHA256SUMS" ]]; then
    echo "::error::No wheel or sdist assets found for release $TAG_NAME."
    exit 1
  fi

  local manifest_items size
  manifest_items="$(mktemp)"
  while read -r checksum artifact; do
    printf '%s  %s\n' "$checksum" "$artifact" > "$asset_dir/${artifact}.sha256"
    size="$(file_size "$asset_dir/$artifact")"
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

  echo "Generated release checksum assets in $asset_dir."
}

publish_checksum_assets() {
  if [[ ! -s "$asset_dir/SHA256SUMS" ]]; then
    echo "::error::SHA256SUMS not found in $asset_dir."
    exit 1
  fi
  if [[ ! -s "$asset_dir/dist-manifest.json" ]]; then
    echo "::error::dist-manifest.json not found in $asset_dir."
    exit 1
  fi
  if ! jq -e '.artifacts | length > 0' "$asset_dir/dist-manifest.json" > /dev/null; then
    echo "::error::dist-manifest.json contains no artifacts."
    exit 1
  fi

  local missing_checksums
  missing_checksums=$(mktemp)
  jq -r '.artifacts[].name' "$asset_dir/dist-manifest.json" |
    while IFS= read -r artifact; do
      [[ -z "$artifact" ]] && continue

      if [[ ! -s "$asset_dir/${artifact}.sha256" ]]; then
        printf '%s\n' "${artifact}.sha256" >> "$missing_checksums"
      fi
    done

  if [[ -s "$missing_checksums" ]]; then
    echo "::error::Missing per-asset checksum files in $asset_dir:"
    cat "$missing_checksums"
    exit 1
  fi
  rm -f "$missing_checksums"

  local -a upload_assets
  upload_assets=("$asset_dir/SHA256SUMS" "$asset_dir/dist-manifest.json")
  while IFS= read -r checksum_file; do
    upload_assets+=("$checksum_file")
  done < <(checksum_files)

  retry_gh "gh release upload" gh release upload "$TAG_NAME" "${upload_assets[@]}" \
    --clobber \
    --repo "$GITHUB_REPOSITORY"

  local body_file clean_body_file asset_url_base
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
}

case "$mode" in
  generate)
    generate_checksum_assets
    ;;
  publish_existing)
    publish_checksum_assets
    ;;
  publish)
    generate_checksum_assets
    publish_checksum_assets
    ;;
esac
