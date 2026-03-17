# Hyperliquid Quote Quota And Equities Telemetry Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make equities Hyperliquid quoting operable on low-volume accounts by adding venue-quota guardrails, explicit quota tooling, and persistent equities telemetry for quote-cycle/order-action review.

**Architecture:** Treat the current AMD failure as a venue-integration gap, not a pricing bug. The fix should add one read-only quota visibility surface, one operator-controlled quota expansion path, one strategy/runtime guardrail that stops futile quote spam on quota exhaustion, and one equities telemetry persistence path matching the existing TokenMM MakerV3 surfaces.

**Tech Stack:** Python Flux runners and MakerV3 strategy, Rust Hyperliquid adapter, Hyperliquid REST `info` / `exchange` endpoints, local SQLite telemetry persistence actors.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | in_progress | main | none | `docs/plans/2026-03-13-hyperliquid-quote-quota-and-equities-telemetry.md` | `shared` | `shared` | working tree | focused verification green | `2026-03-13 16:14 UTC`: Task 1 and the core Task 3/4 code landed. Live AMD now hard-stops on Hyperliquid cumulative-request rejects instead of churning, and journald includes parsed quota fields. The operator quota tool exists and was exercised live, but the actual spend is blocked because the host only has the agent signer; `reserveRequestWeight` is being charged against the signing wallet and Hyperliquid returned `Must deposit before performing actions. User: 0x6b0c...`. |
| Task 1: Lock Hyperliquid Quota Contract In Tests | completed | main | none | `crates/adapters/hyperliquid/src/common/enums.rs`, `crates/adapters/hyperliquid/src/http/query.rs`, `crates/adapters/hyperliquid/src/http/client.rs`, `crates/adapters/hyperliquid/tests/*` | `shared` | `shared` | working tree | `cargo test -p nautilus-hyperliquid test_http_client_info_user_rate_limit_uses_account_address -- --nocapture`; `cargo test -p nautilus-hyperliquid test_http_client_reserve_request_weight_posts_default_action -- --nocapture` | Added stable `userRateLimit` request support plus signed `reserveRequestWeight` coverage and client methods. |
| Task 2: Add Equities Quota Visibility Surface | not_started | unassigned | Task 1 | `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/api/*`, `fluxboard/*`, `tests/unit_tests/*` | `shared` | `shared` | none | not_run | Expose live Hyperliquid request-cap state in API/UI |
| Task 3: Add Venue-Quota Circuit Breaker For MakerV3 | completed | main | Task 1 | `systems/flux/flux/strategies/makerv3/*`, `tests/unit_tests/flux/strategies/makerv3/*` | `shared` | `shared` | working tree | `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_order_safety.py -k 'cumulative_request_limit or rate_limit_triggers_venue_protection_circuit' -p no:rerunfailures` | The exact Hyperliquid cumulative-request rejection now trips venue protection, emits parsed quota fields in alerts/events, and journald shows `quota_requests_used`, `quota_requests_cap`, and `quota_cumulative_volume_traded`. After restarting AMD, live `nRequestsUsed` stayed flat over the next check window instead of continuing to climb. |
| Task 4: Add Explicit Quota-Reservation Tooling | in_progress | main | Task 1 | `crates/adapters/hyperliquid/src/http/*`, `systems/flux/flux/runners/live/*`, `ops/scripts/*`, `tests/*` | `shared` | `shared` | working tree | targeted cargo tests green; live tool dry-run/submit exercised | Added `ops/scripts/deploy/hyperliquid_request_quota.py` plus PyO3 bindings so the worktree can inspect and reserve quota. Live `--show-only` works, but the actual `--amount-usdc 10 --yes` call failed because the host only has `TRADE_XYZ_AGENT_PK`; Hyperliquid charged the action against the signing wallet and rejected it with `Must deposit before performing actions`. Completing this task operationally requires master-wallet signing material or manual reservation from the funded master wallet UI. |
| Task 5: Wire Equities Telemetry Persistence Parity | not_started | unassigned | Task 3 | `deploy/equities/*`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/examples/strategies/test_equities_*`, docs | `shared` | `shared` | none | not_run | Add local SQLite persistence for `quote_cycle`, `order_action`, `execution_fill`, and optional inventory/balance snapshots |
| Task 6: Re-run AMD Canary With Forensic Review | not_started | unassigned | Task 2, Task 3, Task 4, Task 5 | `docs/reviews/*`, tracker docs only | `shared` | `shared` | none | not_run | Re-enable minimal canary, capture quote-cycle/order-action evidence, and decide on broader rollout |

---

### Task 1: Lock Hyperliquid Quota Contract In Tests

**Files:**
- Modify: `crates/adapters/hyperliquid/src/common/enums.rs`
- Modify: `crates/adapters/hyperliquid/src/http/query.rs`
- Modify: `crates/adapters/hyperliquid/src/http/client.rs`
- Test: `crates/adapters/hyperliquid/tests/exec_client.rs`

**Dependencies:** `none`

**Write Scope:** `crates/adapters/hyperliquid/src/common/enums.rs`, `crates/adapters/hyperliquid/src/http/query.rs`, `crates/adapters/hyperliquid/src/http/client.rs`, `crates/adapters/hyperliquid/tests/exec_client.rs`

**Verification Commands:**
- `cargo test -p nautilus-hyperliquid user_rate_limit -- --nocapture`
- `cargo test -p nautilus-hyperliquid reserve_request_weight -- --nocapture`

**Step 1: Write the failing tests**

Add focused adapter tests that:
- parse `userRateLimit` into a stable contract (`cumVlm`, `nRequestsUsed`, `nRequestsCap`, `nRequestsSurplus`)
- submit a signed `reserveRequestWeight` exchange action

**Step 2: Run test to verify it fails**

Run: `cargo test -p nautilus-hyperliquid user_rate_limit -- --nocapture`
Expected: FAIL because the reserve path is not implemented.

**Step 3: Write minimal implementation**

Add request builders and client support only for:
- querying current user quota
- reserving additional actions

**Step 4: Run tests to verify they pass**

Run the commands above and ensure both targeted tests pass.

**Step 5: Commit**

```bash
git add crates/adapters/hyperliquid/src/common/enums.rs \
  crates/adapters/hyperliquid/src/http/query.rs \
  crates/adapters/hyperliquid/src/http/client.rs \
  crates/adapters/hyperliquid/tests/exec_client.rs
git commit -m "feat(hyperliquid): add quota endpoint support"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Add Equities Quota Visibility Surface

**Files:**
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `systems/flux/flux/api/_payloads_common.py`
- Modify: `fluxboard/*`
- Test: `tests/unit_tests/*`

**Dependencies:** `Task 1: Lock Hyperliquid Quota Contract In Tests`

**Write Scope:** `systems/flux/flux/runners/shared/profile_accounts.py`, `systems/flux/flux/api/*`, `fluxboard/*`, `tests/unit_tests/*`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/...`
- `pnpm --dir fluxboard test`

**Step 1: Write the failing tests**

Add backend/API/UI regressions proving equities can display:
- current requests used
- request cap
- request surplus/reserved
- derived over-cap / available status

**Step 2: Run tests to verify they fail**

Run the targeted backend/UI tests and confirm the quota fields are absent.

**Step 3: Write minimal implementation**

Publish quota fields under the Hyperliquid shared account projection and render them in Fluxboard.

**Step 4: Run tests to verify they pass**

Run the targeted backend/UI tests and ensure the quota surface appears.

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/shared/profile_accounts.py \
  systems/flux/flux/api/app.py \
  systems/flux/flux/api/_payloads_common.py \
  fluxboard
git commit -m "feat(equities): surface hyperliquid quota state"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Add Venue-Quota Circuit Breaker For MakerV3

**Files:**
- Modify: `systems/flux/flux/strategies/makerv3/strategy.py`
- Modify: `systems/flux/flux/strategies/makerv3/quote_engine.py`
- Modify: `systems/flux/flux/strategies/makerv3/constants.py`
- Test: `tests/unit_tests/flux/strategies/makerv3/*`

**Dependencies:** `Task 1: Lock Hyperliquid Quota Contract In Tests`

**Write Scope:** `systems/flux/flux/strategies/makerv3/strategy.py`, `systems/flux/flux/strategies/makerv3/quote_engine.py`, `systems/flux/flux/strategies/makerv3/constants.py`, `tests/unit_tests/flux/strategies/makerv3/*`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_quote_engine.py`
- `./.venv/bin/pytest -q tests/unit_tests/flux/strategies/makerv3/test_observability_and_exports.py`

**Step 1: Write the failing tests**

Add regressions proving that the exact Hyperliquid quota-rejection string:
- blocks further place attempts for a cooldown period
- publishes an actionable alert / state reason
- does not keep spamming the venue every quote cycle

**Step 2: Run tests to verify they fail**

Run the targeted tests and confirm the current behavior still places repeatedly.

**Step 3: Write minimal implementation**

Add a venue-protection circuit breaker keyed on the explicit Hyperliquid quota rejection.

**Step 4: Run tests to verify they pass**

Run the targeted Makerv3 suites and confirm the spam stops under quota exhaustion.

**Step 5: Commit**

```bash
git add systems/flux/flux/strategies/makerv3 \
  tests/unit_tests/flux/strategies/makerv3
git commit -m "fix(makerv3): back off on hyperliquid quota exhaustion"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Add Explicit Quota-Reservation Tooling

**Files:**
- Modify: `crates/adapters/hyperliquid/src/http/*`
- Create: `ops/scripts/*` or runner helper
- Test: `crates/adapters/hyperliquid/tests/*`

**Dependencies:** `Task 1: Lock Hyperliquid Quota Contract In Tests`

**Write Scope:** `crates/adapters/hyperliquid/src/http/*`, `ops/scripts/*`, `crates/adapters/hyperliquid/tests/*`

**Verification Commands:**
- targeted cargo tests
- dry-run/operator script verification

**Step 1: Write the failing tests**

Cover signed `reserveRequestWeight` requests and response handling.

**Step 2: Run tests to verify they fail**

Run the targeted cargo test and confirm the action does not exist yet.

**Step 3: Write minimal implementation**

Add the signed exchange action plus a simple operator-facing tool or script to reserve request weight.

**Step 4: Run tests to verify they pass**

Run the targeted cargo tests and a dry-run path if available.

**Step 5: Commit**

```bash
git add crates/adapters/hyperliquid ops/scripts
git commit -m "feat(hyperliquid): add reserve request weight tooling"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Wire Equities Telemetry Persistence Parity

**Files:**
- Modify: `deploy/equities/*`
- Modify: `systems/flux/flux/runners/equities/run_node.py`
- Test: `tests/unit_tests/examples/strategies/test_equities_*`
- Docs: `deploy/equities/README.md`

**Dependencies:** `Task 3: Add Venue-Quota Circuit Breaker For MakerV3`

**Write Scope:** `deploy/equities/*`, `systems/flux/flux/runners/equities/run_node.py`, `tests/unit_tests/examples/strategies/test_equities_*`, `deploy/equities/README.md`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_node.py`
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_stack_contract.py`

**Step 1: Write the failing tests**

Add deploy/runtime contract tests proving equities can persist:
- `orders.sqlite`
- `fills.sqlite`
- `quote_cycles.sqlite`
- optional balance / inventory snapshots

**Step 2: Run tests to verify they fail**

Run the targeted equities runner tests and confirm there is no telemetry shipper parity yet.

**Step 3: Write minimal implementation**

Mirror the TokenMM local telemetry persistence actor setup for equities.

**Step 4: Run tests to verify they pass**

Run the targeted equities test bundle and ensure the telemetry contract is wired.

**Step 5: Commit**

```bash
git add deploy/equities systems/flux/flux/runners/equities/run_node.py \
  tests/unit_tests/examples/strategies/test_equities_run_node.py \
  tests/unit_tests/examples/strategies/test_equities_stack_contract.py
git commit -m "feat(equities): persist makerv3 telemetry locally"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Re-run AMD Canary With Forensic Review

**Files:**
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Create: `docs/reviews/2026-03-13-equities-amd-quota-canary-review.md`

**Dependencies:** `Task 2: Add Equities Quota Visibility Surface`, `Task 3: Add Venue-Quota Circuit Breaker For MakerV3`, `Task 4: Add Explicit Quota-Reservation Tooling`, `Task 5: Wire Equities Telemetry Persistence Parity`

**Write Scope:** `docs/plans/2026-03-12-equities-live-trading-readiness.md`, `docs/reviews/2026-03-13-equities-amd-quota-canary-review.md`

**Verification Commands:**
- `python3 ... /api/v1/signals?profile=equities`
- `journalctl -u flux@equities-node-amd_tradexyz_makerv3.service -n 200 --no-pager`
- SQLite review commands against the new equities telemetry files

**Step 1: Re-enable minimal canary settings**

Use a true 1x1 canary and verify venue quota headroom before restart.

**Step 2: Gather forensic evidence**

Review:
- `quote_cycle`
- `order_action`
- `execution_fill`
- live signals / balances
- journald

**Step 3: Write review**

Capture whether the canary:
- keeps two-sided quotes
- avoids quota spam
- fills/cancels as expected
- remains operational for a defined interval

**Step 4: Commit**

```bash
git add docs/plans/2026-03-12-equities-live-trading-readiness.md \
  docs/reviews/2026-03-13-equities-amd-quota-canary-review.md
git commit -m "docs(equities): record amd quota canary review"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
