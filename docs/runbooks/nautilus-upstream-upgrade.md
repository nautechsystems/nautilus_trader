# Nautilus Upstream Upgrade Runbook

This runbook is the operator and agent checklist for preparing a reviewable
Nautilus upstream upgrade branch in this monorepo.

Use the canonical script under `tooling/dev/prepare-nautilus-upgrade.sh`.
Treat `scripts/sync_upstream.sh` as a compatibility wrapper only.

## When to run it

Run this workflow every two weeks and also after any upstream release or merged
PR that looks security-sensitive, engine-critical, or relevant to a venue this
repo actively uses.

## Current baseline

Do not hard-code fork-point or ahead/behind numbers in this runbook.

At the start of each run, recompute and record current baseline facts in the
dated evidence note for that run, for example:

- current merge-base between local `main` and `upstream/master`
- whether the fork point matches an upstream release tag
- how far local `main` is ahead or behind upstream
- whether the latest upstream release is already included

## What the script prepares

The workflow prepares two review artifacts:

- `upstream-sync/<tag>`
  Clean branch pinned to the target upstream release tag.
- `upgrade/nautilus-<date>-<tag>`
  Review branch based on `origin/main` with the upstream branch merged in.

The workflow does not merge anything into `main`.

## Inputs

Default behavior:

- upstream remote: `upstream`
- upstream URL: `https://github.com/nautechsystems/nautilus_trader.git`
- base remote: `origin`
- base branch: `main`
- target tag: latest upstream `v*` release tag
- evidence path:
  `docs/reviews/<date>-nautilus-upstream-upgrade-<tag>.md`

Useful overrides:

- `TARGET_TAG=v1.224.0`
- `UPGRADE_DATE=20260319`
- `CHERRY_PICK_COMMITS="<sha1> <sha2>"`
- `EVIDENCE_PATH=docs/reviews/<custom>.md`

## Bi-weekly procedure

1. Start from a clean worktree on the branch that should orchestrate the review.
2. Read the latest upstream release notes and recent merged PRs.
3. Choose the target release tag. Default to the latest upstream release unless
   you intentionally need an earlier tag.
4. Decide whether any post-release PRs are worth cherry-picking.
5. Run the canonical script:

```bash
tooling/dev/prepare-nautilus-upgrade.sh
```

Targeting a specific release:

```bash
TARGET_TAG=v1.224.0 tooling/dev/prepare-nautilus-upgrade.sh
```

Targeting a release plus curated post-release fixes:

```bash
TARGET_TAG=v1.224.0 \
CHERRY_PICK_COMMITS="<sha1> <sha2>" \
tooling/dev/prepare-nautilus-upgrade.sh
```

6. Record the run in a new evidence note copied from
   `docs/reviews/nautilus-upstream-upgrade-template.md`.
7. Stop for human review before any merge decision.

Important rerun rule:

- if `upgrade/nautilus-<date>-<tag>` already exists, the script exits instead of
  resetting that branch
- for a fresh rerun, use a new `UPGRADE_DATE` or intentionally delete the old
  review branch before rerunning the same target

## Cherry-pick policy

Cherry-pick only when the change is not worth waiting for the next full sync.

Good candidates:

- security fixes
- engine correctness fixes
- build or packaging fixes that block your environment
- adapter fixes for venues this repo actually uses

Usually skip:

- new venue features you do not use
- broad refactors with unclear downstream value
- cosmetic docs-only upstream changes

## Evidence to capture

Every run should record:

- target upstream release tag and date
- branch names created
- merge conflicts and resolutions
- cherry-picked commits and why they were included
- verification commands and outcomes
- follow-up questions for the reviewer

## Verification

Minimum tooling verification:

- `bash tooling/dev/test-prepare-nautilus-upgrade.sh`
- `bash -n tooling/dev/prepare-nautilus-upgrade.sh`
- `bash -n scripts/sync_upstream.sh`
- `git diff --check`

Upgrade-specific verification should be added to the evidence note based on the
areas affected by the sync, for example:

- adapter smoke tests
- strategy replay/backtest checks
- engine regression tests
- build validation

## Stop conditions

Stop and hand off for review if:

- the merge conflicts are not obviously mechanical
- the upgrade changes engine semantics in active strategies
- the release removes or renames an adapter/config surface you use
- verification reveals backtest, build, or live-adapter regressions
