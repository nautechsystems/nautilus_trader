#!/usr/bin/env bash
set -euo pipefail

# Decide whether the security-audit workflow's audit jobs need to run.
#
# Trust note: this script is `pull_request`-triggered, so a PR can edit it.
# CODEOWNERS gates /scripts/ to @nautechsystems/core and the develop ruleset
# requires code-owner review (require_code_owner_review = true), so any change
# to this gate must be approved by the core team before merge. Do not weaken
# either control without also moving the gate decision to a trusted base-ref
# context (e.g. pull_request_target with explicit checkout of base-ref code).
#
# Output (to $GITHUB_OUTPUT):
#   audit_needed - "true" if audit-relevant paths changed (or event forces a
#                  run), "false" otherwise. Consumed by the `if:` condition on
#                  the audit jobs in .github/workflows/security-audit.yml.
#
# Required env vars:
#   EVENT_NAME       - github.event_name
#   PR_BASE_REF      - github.event.pull_request.base.ref (PR only)
#   PR_HEAD_SHA      - github.event.pull_request.head.sha (PR only)
#   PUSH_BEFORE_SHA  - github.event.before (push only)
#   PUSH_AFTER_SHA   - github.event.after  (push only)
#
# Audit-relevant paths. Keep in sync with the `security_audit_paths` anchor in
# .github/workflows/security-audit.yml.
#   - Lock files                Cargo.lock, uv.lock, python/uv.lock
#   - Manifests                 Cargo.toml, crates/(...)?Cargo.toml,
#                               pyproject.toml, python/pyproject.toml
#   - Audit policy              deny.toml, osv-scanner.toml, .cargo/audit.toml,
#                               .supply-chain/*, .zizmor.yml
#   - Toolchain config          .cargo/config.toml, rust-toolchain.toml,
#                               tools.toml
#   - Audit helpers             scripts/{cargo-tool-version,rust-toolchain,
#                               uv-version}.sh,
#                               .github/actions/*
#   - CI config                 .pre-commit-config.yaml, .github/workflows/*

emit() {
  echo "audit_needed=$1" >> "$GITHUB_OUTPUT"
  echo "audit_needed=$1 ($2)"
}

case "$EVENT_NAME" in
  schedule | workflow_dispatch)
    emit true "forced by ${EVENT_NAME}"
    exit 0
    ;;
  push)
    base="$PUSH_BEFORE_SHA"
    head="$PUSH_AFTER_SHA"
    if [[ -z "$base" || "$base" =~ ^0+$ ]]; then
      emit true "new branch push, no base to diff"
      exit 0
    fi
    if ! git cat-file -e "${base}^{commit}" 2> /dev/null; then
      emit true "push base SHA ${base} not present locally"
      exit 0
    fi
    ;;
  pull_request)
    # The PR event payload freezes base.sha at PR creation time, so intervening
    # pushes to the base branch make that SHA stale. Diff against the
    # merge-base with the current base-branch tip so the gate reflects only
    # the PR's own changes. Mirrors scripts/ci/plan.sh.
    head="$PR_HEAD_SHA"
    if ! base="$(git merge-base "origin/${PR_BASE_REF}" "$head" 2> /dev/null)"; then
      emit true "failed to compute merge-base against origin/${PR_BASE_REF}"
      exit 0
    fi
    if [[ -z "$base" ]]; then
      emit true "empty merge-base against origin/${PR_BASE_REF}"
      exit 0
    fi
    ;;
  *)
    emit true "unknown event ${EVENT_NAME}"
    exit 0
    ;;
esac

pattern='^('
pattern+='Cargo\.(lock|toml)'
pattern+='|crates/(.*/)?Cargo\.toml'
pattern+='|uv\.lock|pyproject\.toml'
pattern+='|\.pre-commit-config\.yaml'
pattern+='|python/(uv\.lock|pyproject\.toml)'
pattern+='|deny\.toml|osv-scanner\.toml|\.supply-chain/.*|\.zizmor\.yml'
pattern+='|tools\.toml|\.cargo/(config|audit)\.toml|rust-toolchain\.toml'
pattern+='|scripts/(cargo-tool-version|rust-toolchain|uv-version)\.sh'
pattern+='|scripts/ci/security-audit-gate\.sh'
pattern+='|\.github/actions/.*'
pattern+='|\.github/workflows/.*'
pattern+=')$'

changed=$(git diff --name-only "$base" "$head")
matches=$(printf '%s\n' "$changed" | grep -E "$pattern" || true)

if [[ -n "$matches" ]]; then
  emit true "matched paths"
  printf '%s\n' "$matches" | sed 's/^/  matched: /'
else
  emit false "no matching paths"
fi
