#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <manifest-path>" >&2
  exit 1
fi

manifest=$1
version="${PUBLISH_WHEEL_VERSION:?PUBLISH_WHEEL_VERSION is required}"
matrix="${PUBLISH_WHEEL_MATRIX:?PUBLISH_WHEEL_MATRIX is required}"

case "$matrix" in
  development)
    if ! [[ "$version" =~ ^[0-9A-Za-z.]+\.dev[0-9]{8}\+[0-9]+$ ]]; then
      echo "Error: Development version must end in .devYYYYMMDD+run, was ${version}" >&2
      exit 1
    fi
    ;;
  nightly)
    if ! [[ "$version" =~ (a[0-9]{8}|\.dev[0-9]{8})$ ]]; then
      echo "Error: Nightly version must end in aYYYYMMDD or .devYYYYMMDD, was ${version}" >&2
      exit 1
    fi
    ;;
  stable)
    bash "${script_dir}/release-version-policy.bash" "$version" > /dev/null
    ;;
  *)
    echo "Error: Unknown wheel matrix ${matrix}" >&2
    exit 1
    ;;
esac

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT
actual="${work_dir}/actual"
expected="${work_dir}/expected"
files="${work_dir}/files"
manifest_new="${work_dir}/manifest"

find dist -type f -name '*.whl' -print | sort > "$files"
if [[ ! -s "$files" ]]; then
  echo "Error: No wheel artifacts found in dist" >&2
  exit 1
fi

: > "$actual"
: > "$manifest_new"
while IFS= read -r path; do
  if [[ "$(dirname "$path")" != "dist" ]]; then
    echo "Error: Wheel artifact must be directly under dist, was ${path}" >&2
    exit 1
  fi

  filename="$(basename "$path")"
  if ! [[ "$filename" =~ ^nautilus_trader-([^-]+)-(cp[0-9]+)-([^-]+)-([A-Za-z0-9_.]+)\.whl$ ]]; then
    echo "Error: Unexpected wheel artifact ${filename}" >&2
    exit 1
  fi

  artifact_version="${BASH_REMATCH[1]}"
  python_tag="${BASH_REMATCH[2]}"
  abi_tag="${BASH_REMATCH[3]}"
  platform_tag="${BASH_REMATCH[4]}"

  if [[ "$artifact_version" != "$version" ]]; then
    echo "Error: Wheel ${filename} has version ${artifact_version}, expected ${version}" >&2
    exit 1
  fi
  if [[ "$abi_tag" != "$python_tag" ]]; then
    echo "Error: Wheel ${filename} has mismatched Python and ABI tags" >&2
    exit 1
  fi

  case "$python_tag" in
    cp312 | cp313 | cp314) ;;
    *)
      echo "Error: Wheel ${filename} has unsupported Python tag ${python_tag}" >&2
      exit 1
      ;;
  esac

  case "$platform_tag" in
    manylinux*_x86_64)
      platform="linux_x86_64"
      ;;
    manylinux*_aarch64)
      platform="linux_aarch64"
      ;;
    macosx*_arm64)
      platform="macos_arm64"
      ;;
    win_amd64)
      platform="windows_x86_64"
      ;;
    *)
      echo "Error: Wheel ${filename} has unsupported platform tag ${platform_tag}" >&2
      exit 1
      ;;
  esac

  printf '%s\t%s\n' "$python_tag" "$platform" >> "$actual"
  hash="$(bash "${script_dir}/publish-wheels-sha256.bash" "$path")"
  size="$(wc -c < "$path" | tr -d ' ')"
  printf '%s\t%s\t%s\n' "$hash" "$size" "$filename" >> "$manifest_new"
done < "$files"

: > "$expected"
for python_tag in cp312 cp313 cp314; do
  {
    printf '%s\t%s\n' "$python_tag" "linux_x86_64"
    if [[ "$matrix" == "nightly" || "$matrix" == "stable" ]]; then
      printf '%s\t%s\n' "$python_tag" "linux_aarch64"
      printf '%s\t%s\n' "$python_tag" "macos_arm64"
      printf '%s\t%s\n' "$python_tag" "windows_x86_64"
    fi
  } >> "$expected"
done

sort -o "$actual" "$actual"
sort -o "$expected" "$expected"
if ! diff -u "$expected" "$actual"; then
  echo "Error: Wheel artifacts do not match the ${matrix} matrix" >&2
  exit 1
fi

sort -t $'\t' -k3,3 -o "$manifest_new" "$manifest_new"
mv "$manifest_new" "$manifest"
echo "Validated wheel version ${version} and ${matrix} matrix"
