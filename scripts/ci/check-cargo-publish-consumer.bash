#!/usr/bin/env bash
# Check the packaged HyperSync and DeFi dependency graph without workspace patches
set -euo pipefail

consumer_work_dir=""

main() {
  local script_dir
  local repo_root
  local target_dir
  local package_target_dir
  local extracted_dir
  local publish_plan_file
  local metadata_file
  local blockchain_path
  local cli_path
  local blockchain_version
  local cli_version

  if [[ "$#" -ne 1 ]]; then
    echo "Usage: $0 PUBLISH_PLAN" >&2
    exit 1
  fi
  publish_plan_file=$1

  script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
  repo_root="$(cd -- "${script_dir}/../.." && pwd)"
  target_dir="${CARGO_TARGET_DIR:-${repo_root}/target}"

  require_command cargo
  require_command jq
  require_command tar

  if [[ ! -s "$publish_plan_file" ]]; then
    echo "::error::Cargo publish plan is missing or empty: ${publish_plan_file}" >&2
    exit 1
  fi

  mkdir -p "$target_dir"
  target_dir="$(cd -- "$target_dir" && pwd)"
  consumer_work_dir="$(mktemp -d "${target_dir}/cargo-publish-consumer.XXXXXX")"
  trap cleanup EXIT

  package_target_dir="${consumer_work_dir}/package-target"
  CARGO_TARGET_DIR="$package_target_dir" cargo package \
    --workspace \
    --locked \
    --no-verify

  extracted_dir="${consumer_work_dir}/packages"
  extract_packages "$publish_plan_file" "$package_target_dir" "$extracted_dir"

  blockchain_version="$(package_version "$publish_plan_file" nautilus-blockchain)"
  cli_version="$(package_version "$publish_plan_file" nautilus-cli)"
  blockchain_path="$(jq -Rn \
    --arg path "${extracted_dir}/nautilus-blockchain-${blockchain_version}" '$path')"
  cli_path="$(jq -Rn --arg path "${extracted_dir}/nautilus-cli-${cli_version}" '$path')"
  write_consumer "$publish_plan_file" "$extracted_dir" "$blockchain_path" "$cli_path"

  metadata_file="${consumer_work_dir}/metadata.json"
  CARGO_TARGET_DIR="$target_dir" cargo metadata \
    --format-version=1 \
    --manifest-path "${consumer_work_dir}/Cargo.toml" > "$metadata_file"

  reject_local_package "$metadata_file" arrow
  reject_local_package "$metadata_file" parquet
  require_registry_package "$metadata_file" arrow 57
  require_registry_package "$metadata_file" arrow 59
  require_registry_package "$metadata_file" parquet 57
  require_registry_package "$metadata_file" parquet 59
  require_registry_package "$metadata_file" thrift 0

  CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" CARGO_TARGET_DIR="$target_dir" \
    cargo check --locked --manifest-path "${consumer_work_dir}/Cargo.toml"

  echo "Cargo published consumer graph is valid"
}

require_command() {
  local command_name=$1

  if ! command -v "$command_name" > /dev/null; then
    echo "::error::${command_name} not found" >&2
    exit 1
  fi
}

extract_packages() {
  local publish_plan_file=$1
  local package_target_dir=$2
  local extracted_dir=$3
  local crate_name
  local crate_version
  local archive

  mkdir -p "$extracted_dir"
  while IFS=$'\t' read -r crate_name crate_version; do
    archive="${package_target_dir}/package/${crate_name}-${crate_version}.crate"
    if [[ ! -f "$archive" ]]; then
      echo "::error::Packaged crate is missing: ${archive}" >&2
      exit 1
    fi
    tar -xzf "$archive" -C "$extracted_dir"
  done < "$publish_plan_file"
}

package_version() {
  local publish_plan_file=$1
  local package_name=$2
  local version

  version="$(awk -F '\t' -v name="$package_name" '$1 == name { print $2; exit }' \
    "$publish_plan_file")"
  if [[ -z "$version" ]]; then
    echo "::error::${package_name} is missing from the Cargo publish plan" >&2
    exit 1
  fi
  printf '%s\n' "$version"
}

write_consumer() {
  local publish_plan_file=$1
  local extracted_dir=$2
  local blockchain_path=$3
  local cli_path=$4
  local crate_name
  local crate_version
  local package_path

  mkdir -p "${consumer_work_dir}/src"
  cat > "${consumer_work_dir}/Cargo.toml" << EOF
[package]
name = "nautilus-cargo-publish-consumer"
version = "0.0.0"
edition = "2024"
publish = false

[workspace]

[dependencies]
nautilus-blockchain = { path = ${blockchain_path}, features = ["hypersync", "turmoil"] }
nautilus-cli = { path = ${cli_path}, features = ["defi"] }
EOF

  printf '\n[patch.crates-io]\n' >> "${consumer_work_dir}/Cargo.toml"
  while IFS=$'\t' read -r crate_name crate_version; do
    package_path="$(jq -Rn \
      --arg path "${extracted_dir}/${crate_name}-${crate_version}" '$path')"
    printf '"%s" = { path = %s }\n' "$crate_name" "$package_path" \
      >> "${consumer_work_dir}/Cargo.toml"
  done < "$publish_plan_file"

  printf 'fn main() {}\n' > "${consumer_work_dir}/src/main.rs"
}

require_registry_package() {
  local metadata_file=$1
  local package_name=$2
  local version_prefix=$3

  if ! jq -e \
    --arg name "$package_name" \
    --arg prefix "${version_prefix}." \
    '.packages | any(
      .name == $name
      and (.version | startswith($prefix))
      and .source == "registry+https://github.com/rust-lang/crates.io-index"
    )' "$metadata_file" > /dev/null; then
    echo "::error::${package_name} ${version_prefix} from crates.io is missing" >&2
    exit 1
  fi
}

reject_local_package() {
  local metadata_file=$1
  local package_name=$2

  if jq -e \
    --arg name "$package_name" \
    '.packages | any(.name == $name and .source == null)' \
    "$metadata_file" > /dev/null; then
    echo "::error::Published consumer graph resolved local ${package_name}" >&2
    exit 1
  fi
}

cleanup() {
  if [[ -z "$consumer_work_dir" ]]; then
    return
  fi

  case "$consumer_work_dir" in
    */cargo-publish-consumer.*)
      rm -rf -- "$consumer_work_dir"
      ;;
    *)
      echo "::error::Refusing to remove unexpected path: ${consumer_work_dir}" >&2
      return 1
      ;;
  esac
}

main "$@"
