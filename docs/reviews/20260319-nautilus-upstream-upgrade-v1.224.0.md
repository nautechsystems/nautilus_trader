# Nautilus Upstream Upgrade Evidence: v1.224.0

## Scope

This note records the first run of the monorepo upstream-upgrade workflow for
Nautilus Trader on `2026-03-19`.

The goal of this run was to:

- verify the current official upstream release target
- prepare a clean upstream mirror branch
- prepare a reviewable upgrade branch from the monorepo `origin/main`
- determine whether the repo is already current at the release level
- identify post-release PRs worth separate review or cherry-pick

## Verified upstream target

- Upstream release target: `v1.224.0`
- Release commit: `076fe374f769ab65654aa5d472f8970ae47607eb`
- Release date: `2026-03-03`
- Source: <https://github.com/nautechsystems/nautilus_trader/releases/tag/v1.224.0>

## Fork-point result

This monorepo fork point is exactly upstream `v1.224.0`.

Verified facts:

- `git merge-base main upstream/master` = `076fe374f769ab65654aa5d472f8970ae47607eb`
- `git describe --tags --exact-match 076fe374f7` = `v1.224.0`
- `git rev-list --left-right --count main...upstream/master` = `617 0`

Interpretation:

- local `main` is `617` commits ahead of the `v1.224.0` fork point
- local `main` is `0` commits behind `upstream/master`
- at the release level, the repo is already current with upstream `v1.224.0`

## Branches prepared

Prepared by `tooling/dev/prepare-nautilus-upgrade.sh`:

- `upstream-sync/v1.224.0`
- `upgrade/nautilus-20260319-v1.224.0`

Branch comparison:

- `git rev-parse origin/main` = `d15ac5d90ae762ac948d050e457501102129a10e`
- `git rev-parse upgrade/nautilus-20260319-v1.224.0` = `d15ac5d90ae762ac948d050e457501102129a10e`
- `git rev-parse upstream-sync/v1.224.0` = `076fe374f769ab65654aa5d472f8970ae47607eb`
- `git rev-list --left-right --count origin/main...upgrade/nautilus-20260319-v1.224.0` = `0 0`
- `git rev-list --left-right --count upstream-sync/v1.224.0...upgrade/nautilus-20260319-v1.224.0` = `0 617`

Outcome:

- the upgrade branch was created successfully
- merging `upstream-sync/v1.224.0` into the upgrade branch was a no-op
- the prepared upgrade branch is currently identical to `origin/main`
- the shared repo root was switched back to `main` after branch preparation so
  day-to-day work stays on the normal branch

## Post-release PR triage queue

No post-release PRs were cherry-picked in this first run.

Reason:

- the repo is already at the latest verified release tag
- venue usage policy was not pinned tightly enough in this run to justify
  automatic adapter cherry-picks

Recommended next-review candidates:

- engine/runtime safety:
  - `#3680` Fix RiskEngine RefCell re-entrancy panic on order denial
  - `#3673` Fix reconciliation when trigger_price is set for non-conditional orders
  - `#3647` Include .cargo config in sdist for correct platform builds
- if Binance is used:
  - `#3670` Fix Binance SBE price/quantity precision derivation
  - `#3665` Fix Binance algo order update
  - `#3646` Fix Binance algo order cancellation parsing
  - `#3641` Fix BinanceSymbol COIN-M perpetual symbol conversion
- if Interactive Brokers is used:
  - `#3723` Fix IB inactive order status handling to prevent silent dropping
  - `#3719` Fix IB adapter shared historical request dedup for concurrent warmup
  - `#3715` Fix IB live-session synchronization
  - `#3731` Refine IB option symbols to be OCC compliant

## Commands and verification

Commands run:

- `git fetch upstream --tags --prune`
- `git merge-base main upstream/master`
- `REPO_ROOT_OVERRIDE=/home/ubuntu/nautilus_trader UPGRADE_DATE=20260319 /home/ubuntu/nautilus_trader/.worktrees/nautilus-upstream-upgrade-20260319/tooling/dev/prepare-nautilus-upgrade.sh`
- `git -C /home/ubuntu/nautilus_trader branch --list 'upstream-sync/*' 'upgrade/*'`
- `git -C /home/ubuntu/nautilus_trader rev-list --left-right --count origin/main...upgrade/nautilus-20260319-v1.224.0`
- `git -C /home/ubuntu/nautilus_trader rev-list --left-right --count upstream-sync/v1.224.0...upgrade/nautilus-20260319-v1.224.0`

Tooling verification for the workflow itself:

- `bash tooling/dev/test-prepare-nautilus-upgrade.sh`
  - `PASS`
- `bash -n tooling/dev/prepare-nautilus-upgrade.sh`
  - `PASS`
- `bash -n scripts/sync_upstream.sh`
  - `PASS`

## Review conclusion

This repo does not need a release-level upstream merge right now because it was
forked from the current upstream release `v1.224.0`.

The correct immediate workflow is:

1. keep `upgrade/nautilus-20260319-v1.224.0` as the prepared review branch for
   this run
2. review whether the post-release PR queue contains venue-specific or
   engine-safety fixes worth cherry-picking
3. rerun the workflow bi-weekly to target the next upstream release or a newly
   justified cherry-pick set
