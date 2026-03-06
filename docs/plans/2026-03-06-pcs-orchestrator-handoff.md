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
- The main remaining blockers are external CI account billing and one documented
  Python compile blocker in `nautilus-model`.

## Open PR Stack

| PR | Branch | Base | Head SHA | Worktree | URL | Status |
| --- | --- | --- | --- | --- | --- | --- |
| PR2 | `pr2/pyo3-execution-exposure` | `main` | `108f268dcbbb0bef1832827150a49ddb2d09b15d` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr2-pyo3-execution-exposure` | <https://github.com/clickconfirm/nautilus-trader/pull/12> | `UNSTABLE` |
| PR3 | `pr3/feature-gating-metadata-store` | `pr2/pyo3-execution-exposure` | `93ad4b230b4e954262943b2243855362488feca5` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr3-feature-gating-metadata-store` | <https://github.com/clickconfirm/nautilus-trader/pull/13> | `CLEAN` |
| PR4 | `pr4/dextype-pancakeswapv2` | `pr3/feature-gating-metadata-store` | `355bcdd08110027278db4dec1f80bfa22a83d064` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr4-dextype-pancakeswapv2` | <https://github.com/clickconfirm/nautilus-trader/pull/14> | `CLEAN` |
| PR5a | `pr5a/instrument-provider-minimal` | `pr4/dextype-pancakeswapv2` | `c2cae97f394a0dba7aad1625dc72a9c33a7bee8d` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr5a-instrument-provider-minimal` | <https://github.com/clickconfirm/nautilus-trader/pull/15> | `CLEAN` |
| PR6a | `pr6a/rpc-types-models` | `pr5a/instrument-provider-minimal` | `5dc713c7b10b2be77089a1a813f1b7c57d22a6a3` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr6a-rpc-types-models` | <https://github.com/clickconfirm/nautilus-trader/pull/16> | `CLEAN` |
| PR6b | `pr6b/rpc-http-methods` | `pr6a/rpc-types-models` | `b0c5c036540763cd76ecabc8f6de0654262ff09e` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr6b-rpc-http-methods` | <https://github.com/clickconfirm/nautilus-trader/pull/17> | `CLEAN` |
| PR7 | `pr7/remote-signer-client` | `pr6b/rpc-http-methods` | `67fb7080e4e57abefbe3a4815cdb1f5665a4f84d` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr7-remote-signer-client` | <https://github.com/clickconfirm/nautilus-trader/pull/18> | `CLEAN` |
| PR8 | `pr8/erc20-allowance` | `pr7/remote-signer-client` | `bd3603c54c8a66738b62efb5c574636b080665af` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr8-erc20-allowance` | <https://github.com/clickconfirm/nautilus-trader/pull/19> | `CLEAN` |
| PR9 | `pr9/defi-wallet` | `pr8/erc20-allowance` | `de2c293e6519402605cb8e7a22fdbd000ca05469` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr9-defi-wallet` | <https://github.com/clickconfirm/nautilus-trader/pull/23> | `CLEAN` |
| PR10 | `pr10/pcs-v2-router` | `pr9/defi-wallet` | `97f49db5c098aa9d97e09da611c38b833f80b297` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr10-pcs-v2-router` | <https://github.com/clickconfirm/nautilus-trader/pull/24> | `CLEAN` |
| PR11 | `pr11/receipt-fills` | `pr10/pcs-v2-router` | `7495479f181acce0eef2b1ef1753e774e30e3cfb` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr11-receipt-fills` | <https://github.com/clickconfirm/nautilus-trader/pull/25> | `CLEAN` |
| PR12a | `pr12a/journal-idempotency` | `pr11/receipt-fills` | `a3fbee1694d9256b4db8bcdc6ac19b5ef4dcc7ab` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr12a-journal-idempotency` | <https://github.com/clickconfirm/nautilus-trader/pull/26> | `CLEAN` |
| PR12b | `pr12b/happy-path-exec` | `pr12a/journal-idempotency` | `016513e28939223233ccb6d579440725788ae1c0` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr12b-happy-path-exec` | <https://github.com/clickconfirm/nautilus-trader/pull/20> | `UNSTABLE` |
| PR12c | `pr12c/ambiguous-retry-reorg` | `pr12b/happy-path-exec` | `d8710a3f1a5847c227ce07531d5818ab6db593a7` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr12c-ambiguous-retry-reorg` | <https://github.com/clickconfirm/nautilus-trader/pull/21> | `UNSTABLE` |
| PR13 | `pr13/python-surface` | `pr12c/ambiguous-retry-reorg` | `ce338d2563ac44aaa58176b1118ca916d445d32d` | `/home/ubuntu/nautilus-trader-dev/.worktrees/pr13-python-surface` | <https://github.com/clickconfirm/nautilus-trader/pull/22> | `CLEAN` |

Notes:

- `CLEAN` here means GitHub currently reports mergeable stack state for that PR.
- `UNSTABLE` on PR2, PR12b, and PR12c is not currently a code-quality signal by itself;
  see the external CI blocker below.

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

## Technical Blocker Still Inside The Repo

Some Python-feature verification commands are blocked by a pre-existing compile error
in `nautilus-model`:

- Failing commands:
  - `cargo test -p nautilus-blockchain --features python`
  - `cargo test -p nautilus-pyo3 --features defi`
- Failing file:
  - `crates/model/src/python/defi/data.rs`
- Current error shape:
  - `Option<Address>` does not implement `FromStr`
  - `Option<Address>` does not implement `Display`
  - call sites currently fail around lines 801, 822, and 877

This blocker is already recorded in the plan doc and was treated as pre-existing
while authoring PR9 and PR13.

## Continuation Instructions

1. Restore GitHub Actions billing or otherwise get checks running again.
2. Re-run or re-trigger checks bottom-up from PR2.
3. Reproduce the `nautilus-model` Python compile blocker once CI is available.
4. If that blocker still prevents the required PR2/PR9/PR13 verification matrix, decide
   whether to:
   - patch the earliest affected branch in the stack, or
   - introduce a minimal blocker-fix PR with explicit plan deviation and tests.
5. Keep merges strictly sequential:
   - PR2 -> PR3 -> PR4 -> PR5a -> PR6a -> PR6b -> PR7 -> PR8 -> PR9 -> PR10 -> PR11 -> PR12a -> PR12b -> PR12c -> PR13
6. After each human merge:
   - append a new `## Progress Log` entry with merged SHA and status `merged`
   - rebase the next branch if `main` or its base changed
   - remove the merged worktree and delete the merged branch
7. Stop after PR13 unless the plan is explicitly extended for optional/post-MVP work.

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

Reproduce the Python compile blocker:

```bash
cd /home/ubuntu/nautilus-trader-dev/.worktrees/pr13-python-surface
cargo test -p nautilus-blockchain --features python
cargo test -p nautilus-pyo3 --features defi
```

Work inside an existing PR worktree:

```bash
cd /home/ubuntu/nautilus-trader-dev/.worktrees/pr12c-ambiguous-retry-reorg
git status --short
gh pr view 21
```

## Plan Completion Definition

For this wave, the authored MVP is effectively complete once PR2 through PR13 are
reviewed, any necessary blocker fix is landed, and the full stack is merged with plan
tracking updated. The optional/post-MVP items listed in the plan are not part of this
completion target unless explicitly scheduled.
