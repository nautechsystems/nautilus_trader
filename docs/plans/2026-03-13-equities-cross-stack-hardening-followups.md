# Equities Cross-Stack Hardening Follow-Ups Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Close the post-review correctness gaps across MakerV4, shared equities control-plane wiring, and shared operational tooling so the eventual HL-vs-IBKR live cutover is built on production-safe strategy truth, stable portfolio/API identity, and usable venue-headroom telemetry.

**Architecture:** Treat the new MakerV4 review findings as symptoms of a few shared contracts that need to be tightened: fail-closed strategy gating, managed-order lifecycle truth, hedge-order metadata propagation, account-scoped balances identity, and Hyperliquid quota observability. Fix family-specific behavior in MakerV4 where required, but push generic improvements into shared seams or adapter/control-plane code where that reduces future drift for MakerV3, TokenMM, or later strategy families. This plan complements `docs/plans/2026-03-13-makerv4-hl-ibkr-prod-cutover.md` and `docs/plans/2026-03-13-hyperliquid-quote-quota-and-equities-telemetry.md`; it does not replace their live rollout gates.

**Tech Stack:** Python strategy/runtime code, Nautilus Trader strategy and adapter APIs, Flux runners/API/profile contracts, Redis-backed portfolio/account projection feeds, Hyperliquid and IBKR adapters, Fluxboard, pytest, Rust Hyperliquid adapter tests, operator scripts.

## Scope And Assumptions

- MakerV4 remains a one-per-side maker contract for the first live wave.
- Residual hedge management remains out of scope; fail closed and alert on missed/partial hedge outcomes.
- Non-MakerV4 fixes in this plan should land in shared abstractions or adapter/control-plane seams where possible, not as equities-only hacks.
- Live canary work stays gated by IBKR auth health and Hyperliquid request headroom; this plan is about code/system hardening before or alongside that gate.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Notes / Last Update |
| --- | --- | --- | --- |
| Overall | completed | main | `2026-03-13 20:24 UTC` Tasks 1-6 are complete in the worktree. Cross-stack hardening is code-complete and locally verified; remaining blockers are live gates in the MakerV4 cutover plan (IBKR auth health, Hyperliquid request headroom, and a one-per-side canary). |
| Task 1: Unify Fail-Closed Strategy Gating | completed | main | `2026-03-13 19:53 UTC` verified in current worktree: MakerV4 now gates quote generation on `_can_quote()` and `_disable_hedging()` cancels managed maker orders. Local spec/quality review found no additional issues in this slice. Verification: targeted pytest `3 passed, 43 deselected`; full `tests/unit_tests/flux/strategies/makerv4` `69 passed`; `git diff --check` clean. |
| Task 2: Harden Maker Managed-Order Lifecycle Truth | completed | main | `2026-03-13 20:02 UTC` added red coverage for partial maker fills and restart-time reclaim of open maker orders, then implemented quantity-decrement reconciliation on maker fills plus `on_start()` reclaim from the claimed/open-order cache. Local spec/quality review found no additional issues in this slice. Verification: focused red/green tests `2 passed, 29 deselected`; planned task command `6 passed, 31 deselected`; full `tests/unit_tests/flux/strategies/makerv4` `71 passed`; mixed regression `54 passed`; `git diff --check` clean. |
| Task 3: Propagate Hedge Metadata Through The IBKR Order Path | completed | main | `2026-03-13 20:11 UTC` added red strategy + adapter coverage for outside-RTH hedge tags, introduced a shared IBKR tag helper, and now submit MakerV4 hedge IOC orders with `IBOrderTags(outsideRth=True)` when the hedge intent requires it. Local spec/quality review found no additional issues in this slice. Verification: focused pytest `2 passed, 56 deselected`; wider Makerv4 + IBKR tag slice `24 passed, 74 deselected`; `git diff --check` clean. |
| Task 4: Stabilize Mixed-Family Balance Identity And Provenance | completed | main | `2026-03-13 20:18 UTC` added payload + equities API contract coverage for account-unique shared position row ids and fixed the `portfolio_snapshot_v2` fallback row-id generator to include account identity for positions. Verification: focused payload/API slice `6 passed, 79 deselected`; full `tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/api/test_payloads.py` `85 passed`; `git diff --check` clean. |
| Task 5: Repair Hyperliquid Quota Tooling And Shared Telemetry | completed | main | `2026-03-13 20:24 UTC` fixed the quota CLI entrypoint to import `nautilus_pyo3` from the worktree, added env override support so it can run without root-readable host files, extracted shared venue-protection/quota parsing for MakerV3 + MakerV4, and MakerV4 now fails closed with structured quota diagnostics on venue-protection rejects. Verification: `hyperliquid_request_quota.py --common-env <tmp> --show-only` returned live `userRateLimit`; focused quota pytest `7 passed, 44 deselected`; `cargo test -p nautilus-hyperliquid --test exec_client -- --nocapture` `21 passed`; `git diff --check` clean. |
| Task 6: Repo-Wide Cleanup, Parity Review, And Regression Gates | completed | main | `2026-03-13 20:24 UTC` refreshed the canonical/readiness docs and reran the cross-stack regression bundle. Verification: `tests/unit_tests/examples/strategies/test_equities_run_node.py tests/unit_tests/examples/strategies/test_tokenmm_run_node.py tests/unit_tests/flux/api/test_equities_profile_contract.py tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py tests/unit_tests/flux/strategies/makerv4/test_strategy.py` `153 passed`; `git diff --check` clean. |

---

### Task 1: Unify Fail-Closed Strategy Gating

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Optional Create: `systems/flux/flux/strategies/shared/tradeability.py`
- Optional Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`

**Step 1: Write the failing tests**

Add tests that require:

- MakerV4 stops generating maker quote targets immediately after `_disable_hedging(...)`.
- MakerV4 cancels any resting maker quotes when the strategy becomes non-tradeable.
- Signal/state payloads stay internally consistent: blocked strategy state must not coincide with new quote generation.
- If a shared gating helper is introduced, MakerV3 safety tests prove existing behavior is unchanged.

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  -k 'disable or blocked or tradeable' \
  -p no:rerunfailures
```

Expected:
- MakerV4 still produces quote targets or leaves maker orders active after hedge-disable paths.

**Step 3: Write the minimal implementation**

Make tradeability a first-class gate for maker quoting:

- introduce one explicit “can quote” check instead of mixing `bot_on` and `tradeable` ad hoc
- ensure hedge-disable transitions stop new maker quoting and trigger maker-order cleanup
- reuse a shared helper only if it genuinely improves parity across MakerV3/MakerV4 without changing MakerV3 behavior

**Step 4: Run tests to verify they pass**

Run the same pytest command and:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4 -p no:rerunfailures
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/shared/tradeability.py \
  systems/flux/flux/strategies/makerv3/quote_engine.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py
git commit -m "fix(makerv4): stop quoting when hedge loop fails closed"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Harden Maker Managed-Order Lifecycle Truth

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv4/managed_orders.py`
- Optional Create: `systems/flux/flux/strategies/shared/managed_order_state.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py`

**Step 1: Write the failing tests**

Add tests that require:

- partial maker fills do not remove the side from managed-order truth until terminal completion
- maker-side terminal callbacks still clear the side cleanly
- restart/restore reconstructs or reclaims resting maker orders from the claimed/open-order surface instead of assuming the in-memory dict is authoritative
- restored strategy state does not double-place quotes when claimed maker orders already exist

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py \
  -k 'partial or restart or restore or claimed' \
  -p no:rerunfailures
```

Expected:
- MakerV4 clears maker-side state on partial fill.
- Restart paths cannot reconstruct claimed maker orders.

**Step 3: Write the minimal implementation**

Implement maker-order truth as a lifecycle, not a submission cache:

- distinguish non-terminal fill updates from terminal maker-order completion
- preserve or recompute remaining maker-side truth until cancel/reject/expire/full-fill
- on startup/restore, rebuild managed maker state from the claimed order surface or open-order cache for the strategy’s allowed maker instrument ids
- if a shared managed-order helper reduces duplication with MakerV3, extract only the generic state transitions

**Step 4: Run tests to verify they pass**

Run the same pytest command and:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv4 -p no:rerunfailures
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/makerv4/managed_orders.py \
  systems/flux/flux/strategies/shared/managed_order_state.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/unit_tests/flux/strategies/makerv4/test_inventory_contract.py
git commit -m "fix(makerv4): reconcile maker order state through fills and restarts"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Propagate Hedge Metadata Through The IBKR Order Path

**Files:**
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Optional Create: `systems/flux/flux/strategies/shared/ibkr_tags.py`
- Optional Modify: `nautilus_trader/adapters/interactive_brokers/common.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`
- Test: `tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py`

**Step 1: Write the failing tests**

Add tests that require:

- `_submit_hedge_intent(...)` carries `outside_rth_hedge_enabled` into the actual Nautilus order tags, not only the local intent object
- any future hedge-order metadata required by the strategy is propagated through a reusable helper instead of hardcoding MakerV4-only string literals inline
- adapter-level tag parsing still accepts the resulting order tags unchanged

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py \
  -k 'outside_rth or IBOrderTags' \
  -p no:rerunfailures
```

Expected:
- MakerV4 strategy test fails because the submitted order does not carry `IBOrderTags(... outsideRth=True)`.

**Step 3: Write the minimal implementation**

Implement one small shared seam for IBKR hedge-order tags:

- encode `outsideRth` through the standard `IBOrderTags` path on the order object
- keep MakerV4 responsible for deciding the metadata, but use a reusable helper if it keeps future strategy families from duplicating tag serialization logic
- do not broaden scope into residual hedge management

**Step 4: Run tests to verify they pass**

Run the same pytest command.

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/strategies/shared/ibkr_tags.py \
  nautilus_trader/adapters/interactive_brokers/common.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  tests/integration_tests/adapters/interactive_brokers/test_execution_order_transform.py
git commit -m "fix(makerv4): propagate outside-rth hedge tags through IBKR orders"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Stabilize Mixed-Family Balance Identity And Provenance

**Files:**
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Optional Modify: `systems/flux/flux/api/app.py`
- Optional Modify: `fluxboard/Balances.tsx`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Test: `tests/unit_tests/flux/api/test_payloads.py`

**Step 1: Write the failing tests**

Add tests that require:

- two position rows with the same instrument on different accounts get distinct `row_id` values
- mixed MakerV3/MakerV4 shared-account rows keep stable row ids and provenance under `portfolio_snapshot_v2`
- Fluxboard-facing payload shape stays backward-compatible for existing row fields

**Step 2: Run tests to verify they fail**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/api/test_payloads.py \
  -k 'row_id or provenance or portfolio_snapshot_v2' \
  -p no:rerunfailures
```

Expected:
- new multi-account position identity case fails because `row_id` drops account.

**Step 3: Write the minimal implementation**

Fix synthetic balance identity at the control-plane seam:

- include account identity in generated position `row_id` values
- preserve explicit `row_id` values from upstream rows when present
- keep shared-account provenance fields stable for mixed-family balances consumers
- update Fluxboard only if it assumes the old, non-unique synthetic key shape

**Step 4: Run tests to verify they pass**

Run the same pytest command and:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/api/test_equities_profile_contract.py -p no:rerunfailures
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/api/_payloads_balances.py \
  systems/flux/flux/api/app.py \
  fluxboard/Balances.tsx \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/api/test_payloads.py
git commit -m "fix(api): make mixed-family balance row identity account-stable"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 5: Repair Hyperliquid Quota Tooling And Shared Telemetry

**Files:**
- Modify: `ops/scripts/deploy/hyperliquid_request_quota.py`
- Modify: `crates/adapters/hyperliquid/src/http/client.rs`
- Modify: `crates/adapters/hyperliquid/src/python/http.rs`
- Modify: `systems/flux/flux/strategies/makerv3/failures.py`
- Modify: `systems/flux/flux/strategies/makerv4/strategy.py`
- Optional Modify: `systems/flux/flux/api/_payloads_signals.py`
- Test: `crates/adapters/hyperliquid/tests/exec_client.rs`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_order_safety.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`

**Step 1: Write the failing tests**

Add tests that require:

- the quota helper file entrypoint works from the checked-in worktree venv
- Hyperliquid quota failures surface structured request-cap fields, not only opaque strings
- MakerV3 and MakerV4 both classify quota exhaustion as a venue-protection / fail-closed condition instead of silently spinning
- if signal payloads are extended, the quota snapshot fields are optional and backward-compatible

**Step 2: Run tests and the broken script to verify failure**

Run:

```bash
./.venv/bin/python ops/scripts/deploy/hyperliquid_request_quota.py --show-only
./.venv/bin/pytest -q \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  -k 'quota or rate_limit or venue_protection' \
  -p no:rerunfailures
cargo test -p nautilus-hyperliquid --test exec_client -- --nocapture
```

Expected:
- script fails at import time
- quota-classification coverage is incomplete or missing for one or both strategy families

**Step 3: Write the minimal implementation**

Repair quota handling at the shared seams:

- fix the script import/runtime path so operators can inspect quota from the worktree environment
- keep the existing Hyperliquid quota parsing in the adapter, but make the request-cap fields reusable by both MakerV3 and MakerV4 failure paths
- add optional shared diagnostics to signal/log payloads only if they can be fed without introducing polling hacks

**Step 4: Run tests to verify they pass**

Run the same commands.

Expected:
- script runs successfully
- strategy tests and adapter tests pass

**Step 5: Commit**

```bash
git add \
  ops/scripts/deploy/hyperliquid_request_quota.py \
  crates/adapters/hyperliquid/src/http/client.rs \
  crates/adapters/hyperliquid/src/python/http.rs \
  systems/flux/flux/strategies/makerv3/failures.py \
  systems/flux/flux/strategies/makerv4/strategy.py \
  systems/flux/flux/api/_payloads_signals.py \
  crates/adapters/hyperliquid/tests/exec_client.rs \
  tests/unit_tests/flux/strategies/makerv3/test_order_safety.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py
git commit -m "fix(hyperliquid): repair quota tooling and unify venue protection telemetry"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 6: Repo-Wide Cleanup, Parity Review, And Regression Gates

**Files:**
- Modify: `docs/plans/2026-03-13-makerv4-hl-ibkr-prod-cutover.md`
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Optional Modify: `deploy/equities/README.md`
- Optional Modify: `systems/flux/flux/strategies/shared/*.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`
- Test: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- Test: `tests/unit_tests/flux/strategies/makerv4/test_strategy.py`

**Step 1: Add or update parity coverage where shared seams moved**

Pin the final repo-wide expectations:

- shared helper extraction does not regress MakerV3 or TokenMM contracts
- mixed-family equities runner/API surfaces remain stable
- plan docs and readiness docs clearly separate code-complete from live-gated

**Step 2: Run the regression bundle**

Run:

```bash
./.venv/bin/pytest -q \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py \
  -p no:rerunfailures
git diff --check
```

Expected: PASS and clean diff check.

**Step 3: Update the docs and live handoff notes**

Refresh:

- the MakerV4 cutover plan to reflect which review gaps are now closed
- the canonical equities readiness tracker so the next operator sees the remaining live gates
- any deploy/readme docs touched by new shared abstractions or API identity rules

**Step 4: Commit**

```bash
git add \
  docs/plans/2026-03-13-makerv4-hl-ibkr-prod-cutover.md \
  docs/plans/2026-03-12-equities-live-trading-readiness.md \
  deploy/equities/README.md \
  systems/flux/flux/strategies/shared \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py \
  tests/unit_tests/flux/strategies/makerv4/test_strategy.py
git commit -m "chore(equities): close cross-stack hardening follow-ups"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

## Live Gate After This Plan

Do not start or resume the live MakerV4 canary until all of the following are true:

1. This plan is complete and verified.
2. `docs/plans/2026-03-13-makerv4-hl-ibkr-prod-cutover.md` Task 5 is unblocked.
3. IBKR live auth is healthy again.
4. Hyperliquid request headroom is positively verified from the worktree.
5. One-symbol canary params are explicitly one-per-side.

Only after those gates should live smoke testing resume.
