# Nautilus Upstream Upgrade Review Template

## Scope

- Run date:
- Operator or agent:
- Target upstream release tag:
- Upstream release date:
- Base branch:
- Upgrade branch:
- Upstream sync branch:

## Upstream summary

- Why this release matters:
- Relevant release-note themes:
- Recent merged PRs considered after the release:

## Branch preparation

- Canonical script used:
  `tooling/dev/prepare-nautilus-upgrade.sh`
- Compatibility wrapper used:
  `scripts/sync_upstream.sh` or `not used`
- Script output summary:

## Cherry-picks

- Included:
- Excluded:
- Reasoning:

## Conflicts and resolutions

- Files with conflicts:
- Resolution summary:
- Any follow-up risk:

## Verification

- `bash tooling/dev/test-prepare-nautilus-upgrade.sh`
- `bash -n tooling/dev/prepare-nautilus-upgrade.sh`
- `bash -n scripts/sync_upstream.sh`
- additional repo-specific verification:

## Reviewer focus

- Engine or matching semantics to inspect:
- Adapter/config/env changes to inspect:
- Build or packaging changes to inspect:
- Data/catalog compatibility to inspect:

## Recommendation

- Ready for human review:
- Merge blocked on:
- Suggested next agent step:
