# Bybit Websocket Fee Currency Fix Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Fix Bybit websocket execution fee currency handling in Nautilus so Flux receives the correct fee asset for spot fills instead of a quote-hardcoded `Money`.

**Architecture:** Keep the fix inside the Nautilus Bybit adapter. Reuse the existing `fee_currency` from Bybit account-order websocket updates by caching it in the websocket handler and passing it into websocket fill parsing, while centralizing commission construction in shared parse code so HTTP and websocket paths cannot diverge. When explicit fee currency is unavailable, preserve current fallback behavior but make it observable with a warning instead of silently pretending certainty.

**Tech Stack:** Rust, Serde, rstest, Cargo workspace tests

**Context Docs:**
- Design: `none` (root cause and execution path were established from local code inspection and live `/api/v1/trades` evidence)
- PRD: `none`
- Relevant specs/runbooks: `none`

**Decision Summary:**
- The fix belongs in `crates/adapters/bybit`, not in Flux, Flux API, or Fluxboard.
- The repo evidence supports enriching executions from websocket account-order `fee_currency`; it does not justify side-based fee-asset inference rules.
- HTTP and websocket fill commission creation must share one helper so they cannot drift again.
- If websocket fill parsing lacks an explicit fee currency even after handler enrichment, keep the existing quote fallback for now and emit a warning for auditability.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | controller | none | `crates/adapters/bybit/src/common/parse.rs`, `crates/adapters/bybit/src/websocket/parse.rs`, `crates/adapters/bybit/src/websocket/handler.rs`, `crates/adapters/bybit/test_data/ws_account_order_spot_filled.json`, `crates/adapters/bybit/test_data/ws_account_execution_spot.json`, `crates/adapters/bybit/test_data/http_get_executions_spot_fee_currency.json`, `docs/plans/2026-03-31-bybit-websocket-fee-currency-fix.md` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/bybit-ws-fee-currency-cache` | `920ac31b15`, `89e44b64b4`, `7b586546f5` | `cargo test -p nautilus-bybit --lib` PASS; `cargo test -p nautilus-bybit --test http` PASS; `git diff --check` PASS | Adapter fix complete; new websocket fills use order `feeCurrency` when available, while existing persisted Flux rows remain unchanged until replay/backfill or new fills arrive |
| Task 1: Add failing regression coverage for explicit fee-currency propagation | in_review_quality | controller | none | `crates/adapters/bybit/src/common/parse.rs`, `crates/adapters/bybit/src/websocket/parse.rs`, `crates/adapters/bybit/src/websocket/handler.rs`, `crates/adapters/bybit/test_data/ws_account_order_spot_filled.json`, `crates/adapters/bybit/test_data/ws_account_execution_spot.json`, `crates/adapters/bybit/test_data/http_get_executions_spot_fee_currency.json` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/bybit-ws-fee-currency-cache` | `920ac31b15` | `cargo test -p nautilus-bybit common::parse::tests::parse_http_spot_execution_preserves_fee_currency -- --exact` PASS; `cargo test -p nautilus-bybit websocket::parse::tests::parse_ws_spot_execution_uses_explicit_fee_currency_override -- --exact` FAIL (`USDT` vs `BTC`); `cargo test -p nautilus-bybit websocket::handler::tests::handler_caches_order_fee_currency_for_spot_execution -- --exact` FAIL (`USDT` vs `BTC`) | Spec review found only controller-only plan-doc scope noise; committed Task 1 diff is otherwise compliant |
| Task 2: Centralize Bybit execution commission parsing | completed | controller | Task 1: Add failing regression coverage for explicit fee-currency propagation | `crates/adapters/bybit/src/common/parse.rs`, `crates/adapters/bybit/src/websocket/parse.rs` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/bybit-ws-fee-currency-cache` | `89e44b64b4` | `cargo test -p nautilus-bybit common::parse::tests::parse_http_spot_execution_preserves_fee_currency -- --exact` PASS; `cargo test -p nautilus-bybit websocket::parse::tests::parse_ws_spot_execution_uses_explicit_fee_currency_override -- --exact` PASS; `cargo test -p nautilus-bybit websocket::parse::tests::parse_ws_execution_into_fill_report -- --exact` PASS | Shared commission helper now backs HTTP and websocket parsing; handler cache still pending |
| Task 3: Enrich websocket execution parsing from order fee-currency cache | completed | controller | Task 2: Centralize Bybit execution commission parsing | `crates/adapters/bybit/src/websocket/handler.rs`, `crates/adapters/bybit/src/websocket/parse.rs` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/bybit-ws-fee-currency-cache` | `7b586546f5` | `cargo test -p nautilus-bybit websocket::handler::tests::handler_caches_order_fee_currency_for_spot_execution -- --exact` PASS; `cargo test -p nautilus-bybit websocket::parse::tests::parse_ws_spot_execution_uses_explicit_fee_currency_override -- --exact` PASS; `cargo test -p nautilus-bybit websocket::parse::tests::parse_ws_execution_into_fill_report -- --exact` PASS | Handler now caches non-empty order `feeCurrency` by `order_id` and applies it to matching websocket fills |
| Task 4: Verify adapter behavior and downstream impact boundaries | completed | controller | Task 3: Enrich websocket execution parsing from order fee-currency cache | `crates/adapters/bybit/src/common/parse.rs`, `crates/adapters/bybit/src/websocket/parse.rs`, `crates/adapters/bybit/src/websocket/handler.rs`, `crates/adapters/bybit/test_data/ws_account_order_spot_filled.json`, `crates/adapters/bybit/test_data/ws_account_execution_spot.json`, `crates/adapters/bybit/test_data/http_get_executions_spot_fee_currency.json`, `docs/plans/2026-03-31-bybit-websocket-fee-currency-fix.md` | `shared` | `/home/ubuntu/nautilus_trader/.worktrees/bybit-ws-fee-currency-cache` | working tree plan update | `cargo test -p nautilus-bybit --lib` PASS; `cargo test -p nautilus-bybit --test http` PASS; `git diff --check` PASS | Verification complete; adapter tests prove corrected fee asset on new fills when order `feeCurrency` is available, and historical Flux rows are unchanged without replay/backfill |

---

### Task 1: Add failing regression coverage for explicit fee-currency propagation

**Files:**
- Create: `crates/adapters/bybit/test_data/ws_account_order_spot_filled.json`
- Create: `crates/adapters/bybit/test_data/ws_account_execution_spot.json`
- Create: `crates/adapters/bybit/test_data/http_get_executions_spot_fee_currency.json`
- Modify: `crates/adapters/bybit/src/common/parse.rs`
- Modify: `crates/adapters/bybit/src/websocket/parse.rs`
- Modify: `crates/adapters/bybit/src/websocket/handler.rs`

**Dependencies:** `none`

**Write Scope:** `crates/adapters/bybit/src/common/parse.rs`, `crates/adapters/bybit/src/websocket/parse.rs`, `crates/adapters/bybit/src/websocket/handler.rs`, `crates/adapters/bybit/test_data/ws_account_order_spot_filled.json`, `crates/adapters/bybit/test_data/ws_account_execution_spot.json`, `crates/adapters/bybit/test_data/http_get_executions_spot_fee_currency.json`

**Verification Commands:**
- `cargo test -p nautilus-bybit parse_http_spot_execution_preserves_fee_currency -- --exact`
- `cargo test -p nautilus-bybit parse_ws_spot_execution_uses_explicit_fee_currency_override -- --exact`
- `cargo test -p nautilus-bybit handler_caches_order_fee_currency_for_spot_execution -- --exact`

**Step 1: Add paired spot websocket fixtures**

Create:
- `crates/adapters/bybit/test_data/ws_account_order_spot_filled.json`
- `crates/adapters/bybit/test_data/ws_account_execution_spot.json`

Use the same `orderId` in both fixtures. The order fixture must include Bybit’s existing `feeCurrency` field set to the actual charged asset. The execution fixture should not invent a websocket `feeCurrency` field because the current repo evidence does not show it on execution payloads.

**Step 2: Add HTTP spot execution parity fixture**

Create `crates/adapters/bybit/test_data/http_get_executions_spot_fee_currency.json` with the same economic execution represented through the HTTP execution schema, including explicit `feeCurrency`.

**Step 3: Write failing HTTP parse regression**

In `crates/adapters/bybit/src/common/parse.rs`, add a test asserting the HTTP fill parser preserves the explicit spot fee currency from the fixture.

**Step 4: Write failing websocket parse regression for explicit override**

In `crates/adapters/bybit/src/websocket/parse.rs`, add a test for websocket fill parsing that passes an explicit fee-currency override and asserts the resulting `report.commission.currency` matches the override instead of quote.

This test should fail against the current hardcoded quote behavior.

**Step 5: Write failing handler regression**

In `crates/adapters/bybit/src/websocket/handler.rs`, add a unit test covering the actual enrichment path:
- cache `feeCurrency` from the account-order fixture
- resolve fee currency for the matching execution fixture
- verify the resolved asset is the stored order fee currency

If helpful, add small handler helper methods to make this behavior testable without spinning the full async loop.

**Step 6: Run targeted tests and confirm they fail for the current bug**

Run the verification commands above.

Expected before implementation:
- HTTP regression should pass or remain green
- websocket parse override regression should fail
- handler regression should fail if the cache path is not implemented yet

**Step 7: Commit**

```bash
git add crates/adapters/bybit/src/common/parse.rs crates/adapters/bybit/src/websocket/parse.rs crates/adapters/bybit/src/websocket/handler.rs crates/adapters/bybit/test_data/ws_account_order_spot_filled.json crates/adapters/bybit/test_data/ws_account_execution_spot.json crates/adapters/bybit/test_data/http_get_executions_spot_fee_currency.json
git commit -m "test: add bybit websocket fee currency regressions"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Centralize Bybit execution commission parsing

**Files:**
- Modify: `crates/adapters/bybit/src/common/parse.rs`
- Modify: `crates/adapters/bybit/src/websocket/parse.rs`

**Dependencies:** `Task 1: Add failing regression coverage for explicit fee-currency propagation`

**Write Scope:** `crates/adapters/bybit/src/common/parse.rs`, `crates/adapters/bybit/src/websocket/parse.rs`

**Verification Commands:**
- `cargo test -p nautilus-bybit parse_http_spot_execution_preserves_fee_currency -- --exact`
- `cargo test -p nautilus-bybit parse_ws_spot_execution_uses_explicit_fee_currency_override -- --exact`
- `cargo test -p nautilus-bybit parse_ws_execution_into_fill_report -- --exact`

**Step 1: Extract a shared commission-construction helper**

In `crates/adapters/bybit/src/common/parse.rs`, add a helper that owns:
- `exec_fee` decimal parsing
- explicit fee-currency normalization (`None` for missing or blank strings)
- `Money` construction

For example:

```rust
pub(crate) fn parse_execution_commission(
    exec_fee: &str,
    explicit_fee_currency: Option<&str>,
    fallback_currency: Currency,
    source: &'static str,
) -> anyhow::Result<Money>
```

**Step 2: Keep fallback semantics narrow**

Resolution order must be:
1. non-empty explicit fee currency
2. fallback currency supplied by the caller

If the helper uses the fallback branch, emit a focused `log::warn!` that records:
- source (`http` or `websocket`)
- whether explicit fee currency was missing or blank
- fallback currency code

Do not add side-based or market-type fee-asset inference rules in this PR.

**Step 3: Repoint HTTP fill parsing to the shared helper**

Replace the inline commission parsing in `parse_fill_report(...)` with the shared helper, passing HTTP `execution.fee_currency` and preserving the current fallback currency only as a defensive backup.

**Step 4: Repoint websocket fill parsing to the shared helper**

Change `parse_ws_fill_report(...)` to accept an explicit fee-currency override argument from the handler and route commission construction through the same helper. Preserve current quote fallback behavior when no override is provided.

**Step 5: Run targeted tests**

Run the verification commands above until:
- HTTP regression passes
- websocket override regression passes
- existing linear websocket regression still passes

**Step 6: Commit**

```bash
git add crates/adapters/bybit/src/common/parse.rs crates/adapters/bybit/src/websocket/parse.rs
git commit -m "refactor: share bybit execution commission parsing"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Enrich websocket execution parsing from order fee-currency cache

**Files:**
- Modify: `crates/adapters/bybit/src/websocket/handler.rs`
- Modify: `crates/adapters/bybit/src/websocket/parse.rs`

**Dependencies:** `Task 2: Centralize Bybit execution commission parsing`

**Write Scope:** `crates/adapters/bybit/src/websocket/handler.rs`, `crates/adapters/bybit/src/websocket/parse.rs`

**Verification Commands:**
- `cargo test -p nautilus-bybit handler_caches_order_fee_currency_for_spot_execution -- --exact`
- `cargo test -p nautilus-bybit parse_ws_spot_execution_uses_explicit_fee_currency_override -- --exact`
- `cargo test -p nautilus-bybit parse_ws_execution_into_fill_report -- --exact`

**Step 1: Add handler fee-currency cache**

Extend `FeedHandler` with a small cache keyed by Bybit `order_id` that stores non-empty `fee_currency` values learned from `AccountOrder` messages.

Prefer a simple handler-owned map rather than a concurrent map because the handler already owns message sequencing and mutable state.

**Step 2: Cache fee currency from account-order updates**

When processing `BybitWsMessage::AccountOrder`, record non-empty `order.fee_currency` for the message’s `order_id` before or alongside order-status report generation.

Normalize blank strings to “missing” and do not cache them.

**Step 3: Resolve fee currency for matching executions**

When processing `BybitWsMessage::AccountExecution`, look up cached `fee_currency` by `execution.order_id` and pass that value into `parse_ws_fill_report(...)`.

If no cache entry exists, allow websocket fill parsing to take the warned fallback path introduced in Task 2.

**Step 4: Add bounded cleanup only if it is obvious and safe**

If there is a clean, low-risk signal already present in the execution or order message to retire terminal-order cache entries, use it. If not, prefer correctness and minimal scope over speculative cache eviction logic, and document the follow-up as out of scope rather than inventing fragile cleanup.

**Step 5: Run targeted tests**

Run the verification commands above until the handler regression passes and existing websocket fill parsing remains green.

**Step 6: Commit**

```bash
git add crates/adapters/bybit/src/websocket/handler.rs crates/adapters/bybit/src/websocket/parse.rs
git commit -m "fix: enrich bybit websocket fills with order fee currency"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Verify adapter behavior and downstream impact boundaries

**Files:**
- Modify: `docs/plans/2026-03-31-bybit-websocket-fee-currency-fix.md`

**Dependencies:** `Task 3: Enrich websocket execution parsing from order fee-currency cache`

**Write Scope:** `docs/plans/2026-03-31-bybit-websocket-fee-currency-fix.md`

**Verification Commands:**
- `cargo test -p nautilus-bybit --lib`
- `cargo test -p nautilus-bybit --test http`
- `git diff --check`

**Step 1: Run adapter verification**

Run:

```bash
cargo test -p nautilus-bybit --lib
cargo test -p nautilus-bybit --test http
```

If another existing integration target directly exercises websocket execution handling, include it too and record the exact command in the tracker.

**Step 2: Run diff hygiene**

Run:

```bash
git diff --check
```

Expected: PASS

**Step 3: Record downstream boundary explicitly**

Do not use historical Flux rows as the primary proof of correctness. Instead, record that:
- the adapter tests prove new fills will carry the corrected fee asset when order `fee_currency` is available
- existing persisted Flux trade rows will remain unchanged until new fills arrive or data is replayed/backfilled

If a live sanity check is available later, treat it as supplemental evidence, not the sole verification gate.

**Step 4: Update the tracker with exact results**

Record the commands and PASS/FAIL outcomes in the Progress Tracker. If any verification cannot be run in this environment, note that explicitly.

**Step 5: Commit**

```bash
git add docs/plans/2026-03-31-bybit-websocket-fee-currency-fix.md
git commit -m "docs: finalize bybit fee currency fix tracker"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
