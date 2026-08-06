#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHECK_SCRIPT="${SCRIPT_DIR}/check-github-action-shas.sh"

test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT

action_file="${test_root}/action.yml"
output="${test_root}/output"
fake_bin="${test_root}/bin"
mkdir -p "$fake_bin"

tag_object_sha="1111111111111111111111111111111111111111"
commit_sha="2222222222222222222222222222222222222222"
mismatch_sha="3333333333333333333333333333333333333333"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  "if [[ \"\$1\" != \"ls-remote\" ]] ||" \
  "  [[ \"\$2\" != \"https://github.com/actions/checkout.git\" ]]; then" \
  '  exit 2' \
  'fi' \
  "printf \"%s\\t%s\\n\" \"$tag_object_sha\" \"refs/tags/v1.2.3\" \"$commit_sha\" \"refs/tags/v1.2.3^{}\"" \
  > "${fake_bin}/git"
chmod +x "${fake_bin}/git"

printf '# https://github.com/actions/checkout\nuses: actions/checkout@%s # v1.2.3\n' \
  "$commit_sha" > "$action_file"
PATH="${fake_bin}:${PATH}" bash "$CHECK_SCRIPT" "$action_file" > "$output"
grep -Fq "OK ($commit_sha)" "$output"

printf '# https://github.com/actions/checkout\nuses: actions/checkout/subpath@%s # v1.2.3\n' \
  "$commit_sha" > "$action_file"
PATH="${fake_bin}:${PATH}" bash "$CHECK_SCRIPT" "$action_file" > "$output"
grep -Fq "OK ($commit_sha)" "$output"

printf '# https://github.com/actions/checkout\nuses: actions/checkout@%s # v1.2.3\n' \
  "$mismatch_sha" > "$action_file"
if PATH="${fake_bin}:${PATH}" bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected a tag commit mismatch to fail" >&2
  exit 1
fi
grep -Fq "MISMATCH (Expected: $mismatch_sha, Got: $commit_sha)" "$output"

printf '# https://github.com/actions/checkout\nuses: actions/checkout@%s\n' \
  "$commit_sha" > "$action_file"
if bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected a missing tag comment to fail" >&2
  exit 1
fi
grep -Fq "FAILED (missing '# <tag>' comment)" "$output"

printf '# https://github.com/actions/checkout\nuses: actions/checkout@%s #  \n' \
  "$commit_sha" > "$action_file"
if bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected an empty tag comment to fail" >&2
  exit 1
fi
grep -Fq "FAILED (missing '# <tag>' comment)" "$output"

printf 'uses: actions/checkout@%s # v1.2.3\n' "$commit_sha" > "$action_file"
if PATH="${fake_bin}:${PATH}" bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected a missing source comment to fail" >&2
  exit 1
fi
grep -Fq \
  "FAILED (expected source comment # https://github.com/actions/checkout immediately above)" \
  "$output"

printf '# https://github.com/actions/cache\nuses: actions/checkout@%s # v1.2.3\n' \
  "$commit_sha" > "$action_file"
if PATH="${fake_bin}:${PATH}" bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected an incorrect source comment to fail" >&2
  exit 1
fi
grep -Fq \
  "FAILED (expected source comment # https://github.com/actions/checkout immediately above)" \
  "$output"

printf '# https://github.com/actions/checkout\n- name: Checkout\nuses: actions/checkout@%s # v1.2.3\n' \
  "$commit_sha" > "$action_file"
if PATH="${fake_bin}:${PATH}" bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected a displaced source comment to fail" >&2
  exit 1
fi
grep -Fq \
  "FAILED (expected source comment # https://github.com/actions/checkout immediately above)" \
  "$output"

printf '%s\n' \
  '# https://github.com/actions/checkout' \
  'uses: actions/checkout@v7 # v7.0.0' \
  > "$action_file"
if bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected a tag action ref to fail" >&2
  exit 1
fi
grep -Fq "FAILED (expected full 40-character commit SHA): actions/checkout@v7" "$output"

printf '%s\n' \
  '# https://github.com/actions/checkout' \
  'uses: actions/checkout@222222222222 # v1.2.3' \
  > "$action_file"
if bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected a short action SHA to fail" >&2
  exit 1
fi
grep -Fq \
  "FAILED (expected full 40-character commit SHA): actions/checkout@222222222222" \
  "$output"

printf '%s\n' \
  '# https://github.com/actions/checkout' \
  'uses: actions/checkout # v1.2.3' \
  > "$action_file"
if bash "$CHECK_SCRIPT" "$action_file" > "$output"; then
  echo "Expected an action ref without an at-sign to fail" >&2
  exit 1
fi
grep -Fq "FAILED (expected full 40-character commit SHA): actions/checkout" "$output"

printf '%s\n' 'uses: ./.github/actions/common-setup' > "$action_file"
bash "$CHECK_SCRIPT" "$action_file" > "$output"
grep -Fq "No GitHub Action SHAs found." "$output"

if bash "$CHECK_SCRIPT" > "$output" 2>&1; then
  echo "Expected a missing action-file argument to fail" >&2
  exit 1
fi
grep -Fq "Usage:" "$output"
echo "GitHub Action pin check tests passed"
