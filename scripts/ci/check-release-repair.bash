#!/usr/bin/env bash
set -euo pipefail

[[ "${EVENT_NAME:-}" == push ]] || exit 1
[[ "${REF_NAME:-}" == develop || "${REF_NAME:-}" == master ]] || exit 1
[[ "${CI_RELEASE_REPAIR_TREE:-}" =~ ^[0-9a-f]{40}$ ]] || exit 1
[[ "$(git rev-parse 'HEAD^{tree}')" == "$CI_RELEASE_REPAIR_TREE" ]] || exit 1
[[ "${BEFORE_SHA:-}" =~ ^[0-9a-f]{40}$ ]] || exit 1
git cat-file -e "${BEFORE_SHA}^{commit}" 2> /dev/null || exit 1
changed_files="$(git diff --no-renames --name-only "$BEFORE_SHA" HEAD)"
[[ -n "$changed_files" ]] || exit 1

while IFS= read -r path; do
  case "$path" in
    .github/workflows/build.yml | .github/workflows/recover-release.yml | .github/workflows/docker.yml | \
      .github/actions/attest-build-provenance-retry/action.yml | SECURITY.md | \
      scripts/ci/check-release-repair.bash | scripts/ci/plan.sh | scripts/ci/test-plan.bash | \
      scripts/ci/sign-pypi-attestations.bash | scripts/ci/verify-published-registries.bash | \
      scripts/ci/recover-release.bash | scripts/ci/release-recovery.py | scripts/ci/publish-release-checksums.sh | \
      scripts/ci/test_recover_release.py | \
      scripts/ci/test-verify-published-registries-crates.bash | \
      docs/developer_guide/releases.md | docs/developer_guide/security.md) ;;
    *) exit 1 ;;
  esac
done <<< "$changed_files"
