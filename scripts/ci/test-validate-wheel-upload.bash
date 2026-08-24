#!/usr/bin/env bash
set -euo pipefail

repo_root=$(git rev-parse --show-toplevel)
script="$repo_root/scripts/ci/validate-wheel-upload.bash"
case_root=$(mktemp -d)
trap 'rm -rf "$case_root"' EXIT

make_case() {
  name=$1
  wheel=${2:-}
  case_dir="$case_root/$name"
  mkdir -p "$case_dir/dist" "$case_dir/python"
  printf '[project]\nversion = "2.0.0rc4"\n' > "$case_dir/python/pyproject.toml"
  if [ -n "$wheel" ]; then
    touch "$case_dir/dist/$wheel"
  fi
}

run_case() {
  name=$1
  bash "$script" "$case_root/$name/dist" "$case_root/$name/python/pyproject.toml"
}

expect_failure() {
  name=$1
  expected=$2
  stderr="$case_root/$name/stderr"
  if run_case "$name" > /dev/null 2> "$stderr"; then
    echo "Expected $name to fail" >&2
    exit 1
  fi
  grep -Fq "$expected" "$stderr"
}

make_case linux-x86 nautilus_trader-2.0.0rc4-cp312-cp312-manylinux_2_34_x86_64.whl
make_case linux-arm nautilus_trader-2.0.0rc4-cp313-cp313-manylinux_2_35_aarch64.whl
make_case macos nautilus_trader-2.0.0rc4-cp314-cp314-macosx_11_0_arm64.whl
make_case windows nautilus_trader-2.0.0rc4-cp313-cp313-win_amd64.whl

for name in linux-x86 linux-arm macos windows; do
  expected=$(find "$case_root/$name/dist" -type f -exec basename {} \;)
  actual=$(run_case "$name")
  if [ "$actual" != "$expected" ]; then
    echo "Expected wheel $expected, found $actual" >&2
    exit 1
  fi
done

make_case missing
expect_failure missing "Expected one nautilus_trader wheel"

make_case malformed nautilus_trader-z.whl
expect_failure malformed "Invalid wheel filename"

make_case version nautilus_trader-2.0.0rc3-cp313-cp313-manylinux_2_34_x86_64.whl
expect_failure version "does not match package version"

make_case python-tag nautilus_trader-2.0.0rc4-cp311-cp311-manylinux_2_34_x86_64.whl
expect_failure python-tag "unsupported Python or ABI tags"

make_case abi nautilus_trader-2.0.0rc4-cp313-cp312-manylinux_2_34_x86_64.whl
expect_failure abi "unsupported Python or ABI tags"

make_case platform nautilus_trader-2.0.0rc4-cp313-cp313-linux_x86_64.whl
expect_failure platform "unsupported platform tag"

make_case duplicate nautilus_trader-2.0.0rc4-cp312-cp312-manylinux_2_34_x86_64.whl
touch "$case_root/duplicate/dist/nautilus_trader-2.0.0rc4-cp313-cp313-manylinux_2_34_x86_64.whl"
expect_failure duplicate "found 2"

make_case directory nautilus_trader-2.0.0rc4-cp313-cp313-manylinux_2_34_x86_64.whl
rm "$case_root/directory/dist/nautilus_trader-2.0.0rc4-cp313-cp313-manylinux_2_34_x86_64.whl"
mkdir "$case_root/directory/dist/nautilus_trader-2.0.0rc4-cp313-cp313-manylinux_2_34_x86_64.whl"
expect_failure directory "not a file"

echo "Wheel upload validation tests passed"
