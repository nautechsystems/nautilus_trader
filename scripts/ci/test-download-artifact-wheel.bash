#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/../.." && pwd)"
action="${repo_root}/.github/actions/download-artifact-wheel/action.yml"
work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

fail() {
  echo "::error::$1" >&2
  exit 1
}

assert_line() {
  if ! grep -Fxq "$2" "$1"; then
    fail "Expected exact line '$2' in $1"
  fi
}

awk -v directory="$work_dir" '
  /^    - name:/ { step++ }
  step { print > (directory "/step-" step) }
  END { print step > (directory "/step-count") }
' "$action"
[[ "$(cat "${work_dir}/step-count")" == "5" ]] || fail "Expected three downloads and two backoffs"

condition=""
reference=""
for attempt in 1 2 3; do
  step="${work_dir}/step-$((attempt * 2 - 1))"
  assert_line "$step" "      id: download-${attempt}"
  assert_line "$step" "        path: dist"
  assert_line "$step" '        pattern: "nautilus_trader-*.whl"'
  assert_line "$step" "        merge-multiple: true"
  assert_line "$step" "        digest-mismatch: error"

  uses="$(sed -n 's/^      uses: //p' "$step")"
  [[ "$uses" =~ ^actions/download-artifact@[0-9a-f]{40}[[:space:]] ]] ||
    fail "Download attempt ${attempt} must use the pinned upstream action"
  if [[ "$attempt" == "1" ]]; then
    reference="$uses"
    if grep -q '^      if:' "$step"; then
      fail "First download attempt must run without a retry condition"
    fi
  else
    [[ "$uses" == "$reference" ]] || fail "Download attempts must use the same action pin"
    assert_line "$step" "      if: ${condition}"
    backoff="${work_dir}/step-$((attempt * 2 - 2))"
    assert_line "$backoff" "      if: ${condition}"
  fi

  if [[ "$attempt" == "3" ]]; then
    if grep -q '^      continue-on-error:' "$step"; then
      fail "Exhausted downloads must fail the composite and block publication"
    fi
  else
    assert_line "$step" "      continue-on-error: true"
  fi
  condition+="${condition:+ && }steps.download-${attempt}.outcome == 'failure'"
done

mkdir -p "${work_dir}/bin" "${work_dir}/workspace"
cat > "${work_dir}/bin/sleep" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ ! -e dist ]] || exit 91
printf '%s\n' "$*" >> "${MOCK_SLEEP_LOG:?}"
MOCK
chmod +x "${work_dir}/bin/sleep"

for attempt in 2 3; do
  backoff="${work_dir}/step-$((attempt * 2 - 2))"
  assert_line "$backoff" "      shell: bash"
  awk '/^      run: \|$/ { capture = 1; next } capture { sub(/^        /, ""); print }' \
    "$backoff" > "${work_dir}/backoff.bash"
  mkdir -p "${work_dir}/workspace/dist"
  printf 'unverified wheel\n' > "${work_dir}/workspace/dist/partial-${attempt}.whl"
  printf 'preserve\n' > "${work_dir}/workspace/other-file"
  (
    cd "${work_dir}/workspace"
    PATH="${work_dir}/bin:$PATH" MOCK_SLEEP_LOG="${work_dir}/sleep.log" \
      GITHUB_WORKSPACE="$PWD" RUNNER_TEMP="$work_dir" \
      bash --noprofile --norc -eo pipefail "${work_dir}/backoff.bash"
  )
  [[ ! -e "${work_dir}/workspace/dist" ]] || fail "Retry kept files from the failed download"
  [[ "$(cat "${work_dir}/workspace/other-file")" == "preserve" ]] ||
    fail "Download cleanup changed another workspace file"
done
[[ "$(cat "${work_dir}/sleep.log")" == $'10\n30' ]] || fail "Expected 10s and 30s backoffs"

download="${work_dir}/download-step"
awk '
  /^  publish:/ { publish = 1 }
  publish && /^      - name: Download built wheels$/ { capture = 1; next }
  capture && /^      - / { exit }
  capture { print }
' "${repo_root}/.github/workflows/build.yml" > "$download"
assert_line "$download" '        uses: $/.github/actions/download-artifact-wheel'
if grep -q '^        continue-on-error:' "$download"; then
  fail "Publication must not ignore exhausted artifact downloads"
fi

echo "Wheel artifact download recovery, exhaustion, and cleanup checks passed"
