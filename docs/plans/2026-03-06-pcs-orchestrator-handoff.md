# PCS Orchestrator Handoff

Date: 2026-03-06

## Purpose

This document is a continuity handoff for the next orchestrator on the PancakeSwap
(PCS) integration wave. The implementation wave has been authored through PR13.
The remaining work is review, blocker resolution, sequential merge coordination,
and final closeout against the source-of-truth plan.

## Source Of Truth

- Plan doc: `docs/plans/2026-03-04-pcs-integration.md`
- Base repo: `/home/ubuntu/nautilus-trader-dev`
- Reference-only repo: `/home/ubuntu/nautilus_trader`

Only edit these sections in the plan doc, append-only:

- `## Progress Log`
- `## Deviations / Decisions`
- `## Known Issues / Follow-ups`

## Hard Invariants

These remain fail-closed and must not be weakened:

- Signer-only execution. Nautilus never holds private keys.
- Typed tx hash correctness: `keccak256(raw_tx_bytes)` over the exact signer-returned
  raw bytes, including typed prefix byte.
- Approvals require post-mining allowance re-check, not only receipt success.
- DEX venue routing must remain config-driven.
- RPC usage must stay bounded and rate-limit aware.

## Current Wave State

Assume PR0 and PR1 are already merged to `main` from prior work. The authored open
stack starts at PR2 and runs through PR13.

Implementation status:

- PR2 through PR13 have code written, tests recorded in the plan, branches created,
  and GitHub PRs opened.
- PR9, PR10, PR11, and PR12a were opened after the initial wave authoring so the
  stacked chain is now complete.
- The earlier `nautilus-model` Python compile blocker was fixed and pushed on PR6a
  head `a03049858ea7f7ed0c32afa565c32a0b05f122eb`; PR16 currently reports
  `mergeStateStatus=CLEAN`.
- PR6b through PR13 have now been force-pushed on top of that PR6a fix, and GitHub
  currently reports PR17 through PR22 plus PR23 through PR26 `mergeStateStatus=CLEAN`.
- PR13 head `f350e8c191cb14776532e82ae3f2021923ac06be` includes the `signer_route`
  PyO3 fix in `crates/adapters/blockchain/src/python/config.rs`, and both
  `cargo test -p nautilus-blockchain --features python` and `cargo test -p
  nautilus-pyo3 --features defi` now pass locally on that committed head.
- The main remaining wave blocker is external GitHub Actions billing, which still
  leaves PR2 `UNSTABLE`.

## Open PR Stack

| PR | Branch | Base | Head SHA | Worktree | URL | Status |
| --- | --- | --- | --- | --- | --- | --- |
| PR2 | `pr2/pyo3-execution-exposure` | `main` | `108f268dcbbb0bef1832827150a49ddb2d09b15d` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr2-pyo3-execution-exposure` | <https://github.com/clickconfirm/nautilus-trader/pull/12> | `UNSTABLE` |
| PR3 | `pr3/feature-gating-metadata-store` | `pr2/pyo3-execution-exposure` | `93ad4b230b4e954262943b2243855362488feca5` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr3-feature-gating-metadata-store` | <https://github.com/clickconfirm/nautilus-trader/pull/13> | `CLEAN` |
| PR4 | `pr4/dextype-pancakeswapv2` | `pr3/feature-gating-metadata-store` | `355bcdd08110027278db4dec1f80bfa22a83d064` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr4-dextype-pancakeswapv2` | <https://github.com/clickconfirm/nautilus-trader/pull/14> | `CLEAN` |
| PR5a | `pr5a/instrument-provider-minimal` | `pr4/dextype-pancakeswapv2` | `c2cae97f394a0dba7aad1625dc72a9c33a7bee8d` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr5a-instrument-provider-minimal` | <https://github.com/clickconfirm/nautilus-trader/pull/15> | `CLEAN` |
| PR6a | `pr6a/rpc-types-models` | `pr5a/instrument-provider-minimal` | `a03049858ea7f7ed0c32afa565c32a0b05f122eb` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr6a-rpc-types-models` | <https://github.com/clickconfirm/nautilus-trader/pull/16> | `CLEAN` |
| PR6b | `pr6b/rpc-http-methods` | `pr6a/rpc-types-models` | `a72e362dce8f297f10d4d57c7ff04a83f024ec90` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr6b-rpc-http-methods` | <https://github.com/clickconfirm/nautilus-trader/pull/17> | `CLEAN` |
| PR7 | `pr7/remote-signer-client` | `pr6b/rpc-http-methods` | `df7c2dc81af25fd1cf68e1f2b749a4c0dc6a1c49` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr7-remote-signer-client` | <https://github.com/clickconfirm/nautilus-trader/pull/18> | `CLEAN` |
| PR8 | `pr8/erc20-allowance` | `pr7/remote-signer-client` | `aab2191336545cdc33f099a44b7c9843b9adeb99` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr8-erc20-allowance` | <https://github.com/clickconfirm/nautilus-trader/pull/19> | `CLEAN` |
| PR9 | `pr9/defi-wallet` | `pr8/erc20-allowance` | `7420641d9c734c15ff1269e422622dd57dce681f` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr9-defi-wallet` | <https://github.com/clickconfirm/nautilus-trader/pull/23> | `CLEAN` |
| PR10 | `pr10/pcs-v2-router` | `pr9/defi-wallet` | `8aa14d45149c290a95dfbf626509ce2e52b5ba12` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr10-pcs-v2-router` | <https://github.com/clickconfirm/nautilus-trader/pull/24> | `CLEAN` |
| PR11 | `pr11/receipt-fills` | `pr10/pcs-v2-router` | `6eb35aab063597a69fae12d8eaa866926cc3707f` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr11-receipt-fills` | <https://github.com/clickconfirm/nautilus-trader/pull/25> | `CLEAN` |
| PR12a | `pr12a/journal-idempotency` | `pr11/receipt-fills` | `ca2d452afb14d0ffcba0862dda57bc4f9d4f60f5` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr12a-journal-idempotency` | <https://github.com/clickconfirm/nautilus-trader/pull/26> | `CLEAN` |
| PR12b | `pr12b/happy-path-exec` | `pr12a/journal-idempotency` | `af578114208714ff134e4f654a9fb2e911029719` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr12b-happy-path-exec` | <https://github.com/clickconfirm/nautilus-trader/pull/20> | `CLEAN` |
| PR12c | `pr12c/ambiguous-retry-reorg` | `pr12b/happy-path-exec` | `b268cdee846fcc39b6a186d5e4d8cac9bee3d6c5` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr12c-ambiguous-retry-reorg` | <https://github.com/clickconfirm/nautilus-trader/pull/21> | `CLEAN` |
| PR13 | `pr13/python-surface` | `pr12c/ambiguous-retry-reorg` | `f350e8c191cb14776532e82ae3f2021923ac06be` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr13-python-surface` | <https://github.com/clickconfirm/nautilus-trader/pull/22> | `CLEAN` |

Notes:

- `CLEAN` here means GitHub currently reports mergeable stack state for that PR.
- `UNSTABLE` on PR2 is not currently a code-quality signal by itself; see the
  external CI blocker below.

## What Each PR Delivers

For exact test commands and historical updates, read the `## Progress Log` in the
plan doc. High-level summary:

- PR2: PyO3 execution config/factory exposure without requiring hypersync.
- PR3: Feature-gating cleanup plus metadata-store boundary for execution.
- PR4: `DexType::PancakeSwapV2` and venue parsing coverage.
- PR5a: Python-first config-driven PancakeSwap instrument provider.
- PR6a: RPC types/models plus typed tx-hash test vectors.
- PR6b: Execution HTTP RPC methods and mock-server tests.
- PR7: Remote signer client with strict raw-tx verification.
- PR8: ERC20 allowance/approve flow with post-approve allowance invariant.
- PR9: Wallet tracker and `QueryAccount` support.
- PR10: PCS V2 quote and swap calldata builder.
- PR11: Receipt-to-fills decoding with strict invariants.
- PR12a: Journal and idempotency primitives.
- PR12b: Happy-path execution vertical slice.
- PR12c: Ambiguous broadcast/retry/reorg handling.
- PR13: Python execution surface, docs, and example script.

## External Blocker

The visible failing GitHub checks are currently an account/billing problem, not a
repository lint problem.

Evidence:

- PR22 check-run annotation (`65940672379`) says:
  `The job was not started because recent account payments have failed or your spending limit needs to be increased. Please check the 'Billing & plans' section in your settings`
- The same annotation was observed for:
  - PR20 check-run `65921349976`
  - PR21 check-run `65929009684`

This means CI cannot presently validate merge readiness even though local hook runs
used in the wave passed where executed.

## Current Technical State Inside The Repo

The previously documented `nautilus-model` Python compile blocker is resolved on
pushed PR6a head `a03049858ea7f7ed0c32afa565c32a0b05f122eb`.

Current top-of-stack state in the PR13 worktree:

- Pushed PR13 fix file:
  - `crates/adapters/blockchain/src/python/config.rs`
- Current pushed fix shape:
  - changes the PyO3 constructor parameter from `signer_route: String` to
    `signer_route: &str`
  - converts the borrowed route into the owned config field with
    `signer_route.to_string()`
  - adds focused Python constructor tests for default and custom `signer_route`
- Verified locally on PR13 head `f350e8c191cb14776532e82ae3f2021923ac06be`:
  - `cargo test -p nautilus-blockchain --features python` (pass)
  - `cargo test -p nautilus-pyo3 --features defi` (pass)
  - `cargo fmt --all -- --check` (pass)

## Continuation Instructions

1. Restore GitHub Actions billing or otherwise get checks running again.
2. Re-run or re-trigger checks bottom-up from PR2 once GitHub Actions billing is
   restored.
3. Keep merges strictly sequential:
   - PR2 -> PR3 -> PR4 -> PR5a -> PR6a -> PR6b -> PR7 -> PR8 -> PR9 -> PR10 -> PR11 -> PR12a -> PR12b -> PR12c -> PR13
4. After each human merge:
   - append a new `## Progress Log` entry with merged SHA and status `merged`
   - rebase the next branch if `main` or its base changed
   - remove the merged worktree and delete the merged branch
5. Stop after PR13 unless the plan is explicitly extended for optional/post-MVP work.

## Commands For The Next Orchestrator

Refresh state:

```bash
cd /home/ubuntu/nautilus-trader-dev
git fetch --all --prune
gh pr list --state open --limit 100 --json number,title,headRefName,baseRefName,url,mergeStateStatus
git worktree list
```

Check the external CI billing blocker:

```bash
gh api repos/clickconfirm/nautilus-trader/check-runs/65940672379/annotations
gh api repos/clickconfirm/nautilus-trader/check-runs/65929009684/annotations
gh api repos/clickconfirm/nautilus-trader/check-runs/65921349976/annotations
```

Inspect and continue the PR13 worktree:

```bash
cd /home/ubuntu/nautilus-trader-dev/.worktrees/pr13-python-surface
git status --short
git diff -- crates/adapters/blockchain/src/python/config.rs
cargo test -p nautilus-blockchain --features python
cargo test -p nautilus-pyo3 --features defi
```

Reconfirm the current PR13 head if needed:

```bash
cd /home/ubuntu/nautilus-trader-dev/.worktrees/pr13-python-surface
git status --short
git rev-parse HEAD
cargo test -p nautilus-blockchain --features python
cargo test -p nautilus-pyo3 --features defi
cargo fmt --all -- --check
```

Refresh the current remote stack state:

```bash
cd /home/ubuntu/nautilus-trader-dev
gh pr list --state open --limit 100 --json number,title,headRefName,baseRefName,url,mergeStateStatus
gh pr view 22 --json number,headRefName,baseRefName,mergeStateStatus,url
```

## Plan Completion Definition

For this wave, the authored MVP is effectively complete once PR2 through PR13 are
reviewed, any necessary blocker fix is landed, and the full stack is merged with plan
tracking updated. The optional/post-MVP items listed in the plan are not part of this
completion target unless explicitly scheduled.
