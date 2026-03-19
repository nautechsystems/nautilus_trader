# Nautilus Upstream Upgrade Workflow Design

**Goal:** Add a repeatable upstream-sync workflow that prepares a reviewable upgrade branch for this monorepo, captures evidence in-repo, and can be re-run by an agent every two weeks without relying on chat history.

## Scope

- Add one canonical tooling entrypoint for preparing an upstream upgrade branch.
- Keep `main` untouched during sync preparation.
- Use official upstream release tags as the default target.
- Allow selective cherry-picks of recent upstream PRs after the chosen release tag.
- Add in-repo runbook and evidence docs so future agents can repeat the workflow.
- Execute an initial sync lane against the current upstream release after the tooling lands.

## Non-Goals

- Do not auto-merge upgrade branches into `main`.
- Do not auto-push or auto-open PRs without an explicit human step.
- Do not try to continuously mirror every upstream commit.
- Do not create CI-only automation in this first slice.

## Workflow Shape

1. Ensure an `upstream` remote exists for `https://github.com/nautechsystems/nautilus_trader.git`.
2. Fetch upstream branches and tags.
3. Identify the target release tag, defaulting to the latest verified upstream release.
4. Create or refresh a clean mirror branch for the upstream target, for example `upstream-sync/v1.224.0`.
5. Create an upgrade branch from local `main`, for example `upgrade/nautilus-20260319-v1.224.0`.
6. Merge the upstream mirror branch into the upgrade branch instead of rebasing `main`.
7. Optionally cherry-pick a curated list of post-release upstream PR commits relevant to this repo's used venues or engine safety.
8. Run targeted verification and write an evidence report with conflicts, carried patches, cherry-picks, and verification output.
9. Hand the branch to a human or follow-on agent for review and merge decisions.

## Branch Model

- `main`: product branch, never rewritten by the sync tool.
- `upstream-sync/<tag-or-branch>`: clean reference branch mirroring upstream.
- `upgrade/nautilus-<date>-<tag>`: review branch containing the proposed sync.

This separates "what upstream shipped" from "what our monorepo carries."

## Tooling Shape

- Canonical script lives under `tooling/dev/` per repo workflow rules.
- `scripts/sync_upstream.sh` remains as a compatibility entrypoint that delegates to the canonical script.
- The canonical script handles:
  - remote bootstrap
  - tag/branch fetch
  - release target selection
  - upgrade branch naming
  - optional cherry-pick list
  - dry-run friendly logging
  - evidence-file path printing for agent handoff

## Documentation Shape

- Runbook under `docs/runbooks/` explaining how an agent should run the process bi-weekly.
- Review/evidence template under `docs/reviews/` for a concrete upgrade record.
- Implementation plan and design doc under `docs/plans/`.

## Verification

Because this repo depends on built native artifacts, full Python test execution is not a reliable baseline in a fresh worktree. The workflow should therefore verify the new tooling with targeted checks:

- shell syntax validation for the sync script
- repo structure checks if touched paths matter
- `git diff --check`
- successful creation of the expected upgrade branch/report for the first run

The upgrade evidence report should also list any additional engine or venue-specific verification a follow-on agent should run before merge.

## Bi-Weekly Agent Contract

An agent should be able to:

1. read the runbook
2. fetch upstream releases and recent merged PRs
3. prepare an upgrade branch
4. apply policy-based cherry-picks
5. write the evidence report
6. stop for human review

That keeps the process deterministic and safe while still reducing operator effort.
