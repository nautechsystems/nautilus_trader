#!/usr/bin/env bash
# Verify that registry-published release artifacts match the release manifests
#
# Usage:
#   verify-published-registries.bash [release-asset-dir]
#
# The directory must contain dist-manifest.json from publish-release-checksums.sh
# This script writes crates-manifest.json into the same directory
set -euo pipefail

asset_dir="${1:-release-assets}"
dist_manifest="${asset_dir}/dist-manifest.json"

if [[ ! -f "$dist_manifest" ]]; then
  echo "::error::dist manifest not found: $dist_manifest"
  exit 1
fi
if ! command -v jq > /dev/null; then
  echo "::error::jq not found."
  exit 1
fi
if ! command -v curl > /dev/null; then
  echo "::error::curl not found."
  exit 1
fi
if ! command -v cargo > /dev/null; then
  echo "::error::cargo not found."
  exit 1
fi
if ! command -v uv > /dev/null; then
  echo "::error::uv not found."
  exit 1
fi

if [[ -n "${PACKAGE_VERSION:-}" ]]; then
  package_version="$PACKAGE_VERSION"
elif [[ -n "${TAG_NAME:-}" ]]; then
  package_version="${TAG_NAME#v}"
else
  package_version="$(bash scripts/package-version.sh)"
fi

github_repository="${GITHUB_REPOSITORY:-nautechsystems/nautilus_trader}"
github_sha="${GITHUB_SHA:-}"
if [[ -z "$github_sha" ]]; then
  echo "::error::GITHUB_SHA is required for trusted-publishing verification."
  exit 1
fi

pypi_project="${PYPI_PROJECT:-nautilus_trader}"
pypi_publisher_repository="${PYPI_PUBLISHER_REPOSITORY:-$github_repository}"
pypi_publisher_workflow="${PYPI_PUBLISHER_WORKFLOW:-build.yml}"
pypi_publisher_environment="${PYPI_PUBLISHER_ENVIRONMENT:-release}"
pypi_attestations_version="${PYPI_ATTESTATIONS_VERSION:-$(bash scripts/tool-version.sh pypi-attestations)}"

cargo_registry_api_url="${CARGO_REGISTRY_API_URL:-https://crates.io/api/v1}"
cargo_sparse_index_url="${CARGO_SPARSE_INDEX_URL:-https://index.crates.io}"
crates_static_url="${CRATES_STATIC_URL:-https://static.crates.io/crates}"
curl_retries="${CURL_RETRIES:-5}"
curl_connect_timeout="${CURL_CONNECT_TIMEOUT:-20}"
curl_max_time="${CURL_MAX_TIME:-300}"
cargo_publish_user_agent="${CARGO_PUBLISH_USER_AGENT:-nautilus-trader-release-verifier}"
registry_propagation_timeout_seconds="${REGISTRY_PROPAGATION_TIMEOUT_SECONDS:-600}"
registry_propagation_poll_seconds="${REGISTRY_PROPAGATION_POLL_SECONDS:-15}"

if ! [[ "$registry_propagation_timeout_seconds" =~ ^[0-9]+$ ]] ||
  [[ "$registry_propagation_timeout_seconds" -lt 1 ]]; then
  echo "::error::REGISTRY_PROPAGATION_TIMEOUT_SECONDS must be a positive integer."
  exit 1
fi
if ! [[ "$registry_propagation_poll_seconds" =~ ^[0-9]+$ ]] ||
  [[ "$registry_propagation_poll_seconds" -lt 1 ]]; then
  echo "::error::REGISTRY_PROPAGATION_POLL_SECONDS must be a positive integer."
  exit 1
fi

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

curl_to_file() {
  local url=$1
  local output=$2

  curl --proto '=https' --tlsv1.2 --silent --show-error --fail --location \
    --retry "$curl_retries" \
    --retry-all-errors \
    --connect-timeout "$curl_connect_timeout" \
    --max-time "$curl_max_time" \
    --header "User-Agent: ${cargo_publish_user_agent}" \
    --output "$output" \
    "$url"
}

sha256_file() {
  sha256sum "$1" | awk '{print $1}'
}

verify_pypi() {
  local project_json="${work_dir}/pypi-project.json"
  local expected_tsv="${work_dir}/pypi-expected.tsv"
  local remote_tsv="${work_dir}/pypi-remote.tsv"
  local expected_names="${work_dir}/pypi-expected.names"
  local remote_names="${work_dir}/pypi-remote.names"

  echo "Verifying PyPI release ${pypi_project} ${package_version}"

  jq -r '
    .artifacts[]
    | select(.name | test("\\.(whl|tar\\.gz)$"))
    | [.name, .sha256]
    | @tsv
  ' "$dist_manifest" | sort > "$expected_tsv"

  if [[ ! -s "$expected_tsv" ]]; then
    echo "::error::No Python artifacts found in $dist_manifest"
    exit 1
  fi

  wait_for_registry_state \
    "PyPI release ${pypi_project} ${package_version}" \
    check_pypi_release_files \
    "$expected_tsv" \
    "$project_json" \
    "$remote_tsv" \
    "$expected_names" \
    "$remote_names"

  while IFS=$'\t' read -r filename expected_sha256; do
    [[ -z "$filename" ]] && continue

    local remote_line remote_sha256 remote_url downloaded_file provenance_file
    remote_line="$(awk -F '\t' -v name="$filename" '$1 == name { print; exit }' "$remote_tsv")"
    remote_sha256="$(printf '%s\n' "$remote_line" | cut -f2)"
    remote_url="$(printf '%s\n' "$remote_line" | cut -f3)"

    if [[ "$remote_sha256" != "$expected_sha256" ]]; then
      echo "::error::PyPI SHA256 mismatch for $filename"
      echo "expected: $expected_sha256"
      echo "remote:   $remote_sha256"
      exit 1
    fi

    downloaded_file="${work_dir}/${filename}"
    wait_for_registry_state \
      "PyPI file ${filename}" \
      check_pypi_file \
      "$filename" \
      "$remote_url" \
      "$downloaded_file" \
      "$expected_sha256"

    provenance_file="${work_dir}/${filename}.provenance.json"
    wait_for_registry_state \
      "PyPI provenance for ${filename}" \
      check_pypi_provenance \
      "$filename" \
      "$provenance_file"

    uv run --no-project --no-build --with "pypi-attestations==${pypi_attestations_version}" -- \
      pypi-attestations verify pypi \
      --repository "https://github.com/${pypi_publisher_repository}" \
      "$remote_url"
  done < "$expected_tsv"
}

check_pypi_release_files() {
  local expected_tsv=$1
  local project_json=$2
  local remote_tsv=$3
  local expected_names=$4
  local remote_names=$5

  if ! curl_to_file "https://pypi.org/pypi/${pypi_project}/json" "$project_json"; then
    return 1
  fi

  jq -r --arg version "$package_version" '
    .releases[$version] // []
    | .[]
    | [.filename, .digests.sha256, .url]
    | @tsv
  ' "$project_json" | sort > "$remote_tsv"

  cut -f1 "$expected_tsv" | sort > "$expected_names"
  cut -f1 "$remote_tsv" | sort > "$remote_names"

  local missing extra
  missing="$(comm -23 "$expected_names" "$remote_names" || true)"
  extra="$(comm -13 "$expected_names" "$remote_names" || true)"
  if [[ -n "$missing" ]]; then
    return 1
  fi
  if [[ -n "$extra" ]]; then
    echo "::error::PyPI release contains unexpected files:"
    printf '%s\n' "$extra"
    return 2
  fi

  local filename expected_sha256 remote_line remote_sha256
  while IFS=$'\t' read -r filename expected_sha256; do
    [[ -z "$filename" ]] && continue

    remote_line="$(awk -F '\t' -v name="$filename" '$1 == name { print; exit }' "$remote_tsv")"
    remote_sha256="$(printf '%s\n' "$remote_line" | cut -f2)"
    if [[ "$remote_sha256" != "$expected_sha256" ]]; then
      echo "::error::PyPI SHA256 mismatch for $filename"
      echo "expected: $expected_sha256"
      echo "remote:   $remote_sha256"
      return 2
    fi
  done < "$expected_tsv"
}

check_pypi_file() {
  local filename=$1
  local remote_url=$2
  local downloaded_file=$3
  local expected_sha256=$4

  if ! curl_to_file "$remote_url" "$downloaded_file"; then
    return 1
  fi

  local downloaded_sha256
  downloaded_sha256="$(sha256_file "$downloaded_file")"
  if [[ "$downloaded_sha256" != "$expected_sha256" ]]; then
    echo "::error::Downloaded PyPI file SHA256 mismatch for $filename"
    echo "expected: $expected_sha256"
    echo "actual:   $downloaded_sha256"
    return 2
  fi
}

check_pypi_provenance() {
  local filename=$1
  local provenance_file=$2

  if ! curl_to_file \
    "https://pypi.org/integrity/${pypi_project}/${package_version}/${filename}/provenance" \
    "$provenance_file"; then
    return 1
  fi

  if ! jq -e \
    --arg repository "$pypi_publisher_repository" \
    --arg workflow "$pypi_publisher_workflow" \
    --arg environment "$pypi_publisher_environment" \
    '
      [
        .attestation_bundles[].publisher?
        | select(
            .kind == "GitHub"
            and .repository == $repository
            and .workflow == $workflow
            and .environment == $environment
          )
      ]
      | length > 0
    ' "$provenance_file" > /dev/null; then
    echo "::error::PyPI provenance for ${filename} has no matching publisher identity."
    echo "expected repository: ${pypi_publisher_repository}"
    echo "expected workflow:   ${pypi_publisher_workflow}"
    echo "expected environment: ${pypi_publisher_environment}"
    return 2
  fi
}

sparse_index_path() {
  local crate_name=${1,,}
  local crate_name_length=${#crate_name}

  case "$crate_name_length" in
    1)
      printf '1/%s' "$crate_name"
      ;;
    2)
      printf '2/%s' "$crate_name"
      ;;
    3)
      printf '3/%s/%s' "${crate_name:0:1}" "$crate_name"
      ;;
    *)
      printf '%s/%s/%s' "${crate_name:0:2}" "${crate_name:2:2}" "$crate_name"
      ;;
  esac
}

verify_crates() {
  local metadata_file="${work_dir}/cargo-metadata.json"
  local plan_file="${work_dir}/cargo-publish-plan.tsv"
  local manifest_items="${work_dir}/crates-manifest-items.jsonl"

  echo "Verifying crates.io workspace crates for ${github_repository}@${github_sha}"

  cargo metadata --no-deps --format-version=1 > "$metadata_file"
  jq -r '
    def crates_io_publishable:
      .publish == null or (.publish | index("crates-io"));

    .packages[]
    | select(.source == null and crates_io_publishable)
    | [.name, .version]
    | @tsv
  ' "$metadata_file" | sort > "$plan_file"

  if [[ ! -s "$plan_file" ]]; then
    echo "::error::No publishable workspace crates found."
    exit 1
  fi

  : > "$manifest_items"

  while IFS=$'\t' read -r crate_name crate_version; do
    [[ -z "$crate_name" ]] && continue

    local versions_json version_json checksum trustpub provider repository sha
    local published_by publish_status current_release_commit static_url downloaded_file index_file

    echo "Verifying ${crate_name} ${crate_version}"

    versions_json="${work_dir}/${crate_name}-versions.json"
    version_json_file="${work_dir}/${crate_name}-${crate_version}-version.json"
    wait_for_registry_state \
      "crates.io API version ${crate_name} ${crate_version}" \
      check_crates_io_version \
      "$crate_name" \
      "$crate_version" \
      "$versions_json" \
      "$version_json_file"

    version_json="$(< "$version_json_file")"

    checksum="$(jq -r '.checksum // empty' <<< "$version_json")"
    if [[ -z "$checksum" ]]; then
      echo "::error::Missing crates.io checksum for ${crate_name} ${crate_version}"
      exit 1
    fi

    trustpub="$(jq -c '.trustpub_data' <<< "$version_json")"
    provider="$(jq -r '.trustpub_data.provider // empty' <<< "$version_json")"
    repository="$(jq -r '.trustpub_data.repository // empty' <<< "$version_json")"
    sha="$(jq -r '.trustpub_data.sha // empty' <<< "$version_json")"
    published_by="$(jq -c '.published_by' <<< "$version_json")"

    if [[ "$published_by" != "null" ]]; then
      echo "::error::Expected trusted publishing for ${crate_name}, got user publisher:"
      echo "$published_by"
      exit 1
    fi
    if [[ "$provider" != "github" || "$repository" != "$github_repository" || -z "$sha" ]]; then
      echo "::error::Unexpected trusted-publishing identity for ${crate_name} ${crate_version}"
      echo "expected: github ${github_repository}"
      echo "remote:   ${trustpub}"
      exit 1
    fi

    if [[ "$sha" == "$github_sha" ]]; then
      publish_status="current_release"
      current_release_commit=true
    else
      publish_status="previously_published"
      current_release_commit=false
      echo "::notice::${crate_name} ${crate_version} was already published from ${repository}@${sha}."
    fi

    static_url="${crates_static_url}/${crate_name}/${crate_name}-${crate_version}.crate"
    downloaded_file="${work_dir}/${crate_name}-${crate_version}.crate"
    wait_for_registry_state \
      "static crate ${crate_name} ${crate_version}" \
      check_static_crate_file \
      "$crate_name" \
      "$crate_version" \
      "$static_url" \
      "$downloaded_file" \
      "$checksum"

    index_file="${work_dir}/${crate_name}-index.jsonl"
    wait_for_registry_state \
      "sparse index entry ${crate_name} ${crate_version}" \
      check_sparse_index_entry \
      "$crate_name" \
      "$crate_version" \
      "$index_file" \
      "$checksum"

    jq -nc \
      --arg name "$crate_name" \
      --arg version "$crate_version" \
      --arg checksum "$checksum" \
      --arg static_url "$static_url" \
      --arg publish_status "$publish_status" \
      --argjson trustpub "$trustpub" \
      --argjson current_release_commit "$current_release_commit" \
      '{
        name: $name,
        version: $version,
        sha256: $checksum,
        url: $static_url,
        trusted_publishing: $trustpub,
        release_status: $publish_status,
        current_release_commit: $current_release_commit
      }' >> "$manifest_items"
  done < "$plan_file"

  jq -n \
    --arg generated_at "$(date -u '+%Y-%m-%dT%H:%M:%SZ')" \
    --arg repository "$github_repository" \
    --arg sha "$github_sha" \
    --slurpfile crates "$manifest_items" \
    '{
      schema_version: 1,
      generated_at: $generated_at,
      repository: $repository,
      commit_sha: $sha,
      crates: $crates
    }' > "${asset_dir}/crates-manifest.json"
}

check_crates_io_version() {
  local crate_name=$1
  local crate_version=$2
  local versions_json=$3
  local version_json_file=$4

  if ! curl_to_file "${cargo_registry_api_url}/crates/${crate_name}/versions" "$versions_json"; then
    return 1
  fi

  local version_json checksum trustpub provider repository sha published_by
  version_json="$(jq -c --arg version "$crate_version" '
    .versions[]
    | select(.num == $version)
  ' "$versions_json")"
  if [[ -z "$version_json" ]]; then
    return 1
  fi

  checksum="$(jq -r '.checksum // empty' <<< "$version_json")"
  if [[ -z "$checksum" ]]; then
    return 1
  fi

  trustpub="$(jq -c '.trustpub_data' <<< "$version_json")"
  provider="$(jq -r '.trustpub_data.provider // empty' <<< "$version_json")"
  repository="$(jq -r '.trustpub_data.repository // empty' <<< "$version_json")"
  sha="$(jq -r '.trustpub_data.sha // empty' <<< "$version_json")"
  published_by="$(jq -c '.published_by' <<< "$version_json")"

  if [[ "$published_by" != "null" ]]; then
    echo "::error::Expected trusted publishing for ${crate_name}, got user publisher:"
    echo "$published_by"
    return 2
  fi
  if [[ -z "$provider" || -z "$repository" || -z "$sha" ]]; then
    return 1
  fi
  if [[ "$provider" != "github" || "$repository" != "$github_repository" ]]; then
    echo "::error::Unexpected trusted-publishing identity for ${crate_name} ${crate_version}"
    echo "expected: github ${github_repository}"
    echo "remote:   ${trustpub}"
    return 2
  fi
  printf '%s\n' "$version_json" > "$version_json_file"
}

check_static_crate_file() {
  local crate_name=$1
  local crate_version=$2
  local static_url=$3
  local downloaded_file=$4
  local checksum=$5

  if ! curl_to_file "$static_url" "$downloaded_file"; then
    return 1
  fi

  local downloaded_sha256
  downloaded_sha256="$(sha256_file "$downloaded_file")"
  if [[ "$downloaded_sha256" != "$checksum" ]]; then
    echo "::error::Downloaded crate SHA256 mismatch for ${crate_name} ${crate_version}"
    echo "expected: $checksum"
    echo "actual:   $downloaded_sha256"
    return 2
  fi
}

check_sparse_index_entry() {
  local crate_name=$1
  local crate_version=$2
  local index_file=$3
  local checksum=$4

  if ! curl_to_file "${cargo_sparse_index_url%/}/$(sparse_index_path "$crate_name")" "$index_file"; then
    return 1
  fi

  local index_checksum
  index_checksum="$(jq -r --arg version "$crate_version" '
    select(.vers == $version)
    | .cksum
  ' "$index_file")"
  if [[ -z "$index_checksum" ]]; then
    return 1
  fi
  if [[ "$index_checksum" != "$checksum" ]]; then
    echo "::error::Sparse index checksum mismatch for ${crate_name} ${crate_version}"
    echo "api:   $checksum"
    echo "index: $index_checksum"
    return 2
  fi
}

wait_for_registry_state() {
  local description=$1
  shift

  local start now elapsed remaining status
  start="$(date +%s)"
  while true; do
    set +e
    "$@"
    status=$?
    set -e

    if [[ "$status" -eq 0 ]]; then
      return 0
    fi
    if [[ "$status" -eq 2 ]]; then
      return 2
    fi

    now="$(date +%s)"
    elapsed=$((now - start))
    if [[ "$elapsed" -ge "$registry_propagation_timeout_seconds" ]]; then
      echo "::error::Timed out waiting for ${description} to propagate."
      return 1
    fi

    remaining=$((registry_propagation_timeout_seconds - elapsed))
    echo "Waiting for ${description} to propagate (${remaining}s remaining)."
    sleep "$registry_propagation_poll_seconds"
  done
}

verify_pypi
verify_crates

echo "Published registry verification succeeded."
