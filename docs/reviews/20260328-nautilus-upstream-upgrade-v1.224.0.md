# Nautilus Upstream Upgrade Evidence: v1.224.0

## Scope

This note records the `2026-03-28` upstream upgrade review run for Nautilus
Trader in this monorepo.

This run started after PR `#67`
(<https://github.com/clickconfirm/nautilus-trader/pull/67>) was merged into
`main`. That PR closed the workflow implementation review, addressed the open
review comments on `tooling/dev/prepare-nautilus-upgrade.sh`, and hardened the
cherry-pick input validation used for this run.

Run metadata:

- operator or agent: `Codex`
- target upstream release tag: `v1.224.0`
- upstream release date: `2026-03-03T07:48:59Z`
- base branch: `origin/main`
- base branch head before sync: `5e413a4af56e3d6a7910f36e83375e60b66973fb`
- upgrade branch: `upgrade/nautilus-20260328-v1.224.0`
- upgrade branch head after sync: `aff0d04921c930e9bac9671a4f14536cd5a87469`
- upstream sync branch: `upstream-sync/v1.224.0`

## Verified upstream baseline

The latest upstream release is still `v1.224.0` as of `2026-03-28`.

Verified facts:

- `gh api repos/nautechsystems/nautilus_trader/releases/latest`
  - `tag_name`: `v1.224.0`
  - `published_at`: `2026-03-03T07:48:59Z`
- `git merge-base origin/main upstream/master`
  - `076fe374f769ab65654aa5d472f8970ae47607eb`
- `git describe --tags --exact-match 076fe374f769ab65654aa5d472f8970ae47607eb`
  - `v1.224.0`
- `git rev-list --left-right --count origin/main...upstream/master`
  - `964 0`

Interpretation:

- this repo still forks exactly from upstream release `v1.224.0`
- local `origin/main` is `964` commits ahead of the fork point
- local `origin/main` is `0` commits behind `upstream/master`
- the review value in this run comes from curated post-release fixes, not from a
  newer upstream tag

## Upstream summary

This run focused on post-release fixes worth taking before the next full
release-level sync:

- engine correctness
- reconciliation correctness
- Binance adapter correctness for an active venue
- Interactive Brokers adapter correctness for an active venue

Recent merged PRs considered after the release were triaged against the repo's
current venue usage and runtime risk. Only the narrow set below was taken.

## Branch preparation

Canonical script used:

```bash
TARGET_TAG=v1.224.0 \
UPGRADE_DATE=20260328 \
CHERRY_PICK_COMMITS="90dc33e69f88cb8874f828ce65afe2426de3e420 5b8d5813eedf580f120c098dd80526a5fd539a0e f02e6247ad2cbf908923cb974f77916e7af8e6d1 91576bcc78ad24cd3317927233fef98b14569eeb 371d32c53bd2e549d93be4592766da30562fb952 f26812136e6888aaafa0ba33956bba0d5c7c4787 046d86d74fd82150e0cda68a78dd11eb71fc33c1 ebc62c348e0ba0197af7b21f70e3870588119b10 b912b0bb248bfc5c891f7dfa1be767308cfea272" \
tooling/dev/prepare-nautilus-upgrade.sh
```

Compatibility wrapper used:

- `not used`

Script output summary:

- created or refreshed `upstream-sync/v1.224.0` at
  `076fe374f769ab65654aa5d472f8970ae47607eb`
- created `upgrade/nautilus-20260328-v1.224.0` from `origin/main`
- cherry-picked nine upstream merge commits onto the upgrade branch
- stopped for conflicts in the IB-related follow-up commits, which were resolved
  manually and continued with `git cherry-pick --continue`

## Cherry-picks

Included:

- `#3680` `90dc33e69f88cb8874f828ce65afe2426de3e420`
  Fix RiskEngine RefCell re-entrancy panic on order denial
- `#3673` `5b8d5813eedf580f120c098dd80526a5fd539a0e`
  Fix reconciliation when `trigger_price` is set for non-conditional orders
- `#3670` `f02e6247ad2cbf908923cb974f77916e7af8e6d1`
  Fix Binance SBE price/quantity precision derivation
- `#3665` `91576bcc78ad24cd3317927233fef98b14569eeb`
  Fix Binance algo order update
- `#3715` `371d32c53bd2e549d93be4592766da30562fb952`
  Fix IB live-session synchronization
- `#3719` `f26812136e6888aaafa0ba33956bba0d5c7c4787`
  Fix IB adapter shared historical request dedup for concurrent warmup
- `#3723` `046d86d74fd82150e0cda68a78dd11eb71fc33c1`
  Fix IB inactive order status handling to prevent silent dropping
- `#3731` `ebc62c348e0ba0197af7b21f70e3870588119b10`
  Refine IB option symbols to be OCC compliant
- `#3753` `b912b0bb248bfc5c891f7dfa1be767308cfea272`
  Fix IB spread instrument not found on restart reconciliation

Excluded:

- all other post-release merged PRs through `2026-03-28` that were not tied to
  engine correctness, active-venue adapter correctness, or an immediate
  operational blocker for this repo

Reasoning:

- the fork point is already at the latest upstream release tag
- a narrow cherry-pick set keeps conflict risk bounded
- the selected set covers the highest-signal fixes for active venues in this
  repo: Binance and Interactive Brokers

## Conflicts and resolutions

Files with conflicts:

- `nautilus_trader/live/execution_engine.py`
- `tests/integration_tests/adapters/interactive_brokers/test_execution.py`
- `tests/unit_tests/live/test_execution_recon.py`
- `nautilus_trader/adapters/interactive_brokers/client/order.py`
- `nautilus_trader/adapters/interactive_brokers/execution.py`
- `nautilus_trader/adapters/interactive_brokers/providers.py`
- `tests/integration_tests/adapters/interactive_brokers/test_execution_reconciliation.py`

Resolution summary:

- preserved local `OmsType` and event-import surfaces while taking the upstream
  IB live-session synchronization fixes
- kept local reconciliation test coverage and updated it for the upstream tuple
  return shape in `_query_position_status_reports()`
- carried forward upstream inactive-order handling, including both
  `why_held` and `ts_order_status_recv_ns`
- retained the local `venue: str | None = None` provider surface while taking
  upstream OCC option-symbol refinement
- combined the local and upstream reconciliation test imports/helpers so local
  instrument scoping tests and upstream spread restart tests both remain present

Any follow-up risk:

- the IB Python test surface was not rerun end-to-end in this worktree because
  the local `nautilus_pyo3` extension was not built here
- current confidence comes from targeted Rust tests, Python syntax validation,
  `git diff --check`, and careful conflict resolution review

## Verification

Workflow tooling verification:

- `bash tooling/dev/test-prepare-nautilus-upgrade.sh`
  - `PASS`
- `bash -n tooling/dev/prepare-nautilus-upgrade.sh`
  - `PASS`
- `bash -n scripts/sync_upstream.sh`
  - `PASS`

Upgrade-specific verification:

- `cargo test -p nautilus-execution --lib`
  - `255 passed; 0 failed`
- `cargo test -p nautilus-risk --lib`
  - `10 passed; 0 failed`
- `cargo test -p nautilus-binance --lib`
  - `166 passed; 0 failed`
- `cargo test -p nautilus-common --lib`
  - unrelated existing logger tests failed; not in the changed sync surface
- `cargo test -p nautilus-common msgbus::switchboard::tests:: --lib`
  - `9 passed; 0 failed`
- `python3 -m py_compile` on changed Python files
  - `PASS`
- `git diff --check`
  - `PASS`

Verification gap:

- `python3 -m pytest ...` in this worktree was not usable as final evidence
  because the local extension module backing `nautilus_trader.core.data` was not
  built in the plain shell environment

## Reviewer focus

- engine or matching semantics to inspect:
  - risk-engine order denial path and reconciliation behavior when
    `trigger_price` is set for non-conditional orders
- adapter/config/env changes to inspect:
  - Binance SBE precision derivation and algo-order updates
  - IB live-session synchronization
  - IB historical warmup dedup
  - IB inactive-order status handling
  - IB option symbol formatting
  - IB spread restart reconciliation
- build or packaging changes to inspect:
  - none in the selected cherry-pick set
- data or catalog compatibility to inspect:
  - IB provider instrument parsing against locally cached instrument metadata

## Recommendation

- ready for human review: `yes`
- merge blocked on: no blocking failures found in the selected sync surface
- suggested next agent step:
  - fast-forward `main` to `upgrade/nautilus-20260328-v1.224.0`, push
    `origin/main`, and then retire the review worktrees
