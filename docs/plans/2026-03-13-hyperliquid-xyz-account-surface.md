# Hyperliquid XYZ Account Surface Implementation Plan

> **For the execution agent:** REQUIRED SUB-SKILL: Before implementing this plan, choose exactly one execution mode and use the matching skill: `superpowers:subagent-driven-development` for same-session execution or `superpowers:executing-plans` for a separate-session handoff.
> **Progress:** The Progress Tracker in this document is the execution source of truth and must be updated on every state change.

**Goal:** Make the equities stack surface the full live Hyperliquid `xyz` master-account state consistently across shared-account projection, portfolio snapshot, balances API, and Fluxboard, including cash, perp positions, and account totals.

**Architecture:** Treat `hyperliquid.xyz.main` as one shared-account source of truth for the funded master account. The Hyperliquid account projection provider must fetch both `spotClearinghouseState` and `clearinghouseState` with the configured `dex`, publish spot cash rows plus builder-deployed perp position rows, and carry those rows unchanged through `portfolio_snapshot_v2` into the API and Fluxboard. Strategy-local inventory remains separate and continues to describe per-strategy local maker state; shared-account Hyperliquid rows describe real account state, including positions that are not part of the retained 10-name basket.

**Tech Stack:** Python Flux runners/API, Rust Hyperliquid adapter via PyO3, Redis, pytest, cargo test, Fluxboard, live Hyperliquid info endpoint.

## Continuation Context

- Live evidence on `2026-03-13 10:07 UTC` proves the current balances surface is underreporting Hyperliquid:
  - `POST /info` with `{"type":"clearinghouseState","user":"0x6ed25...","dex":"xyz"}` returns live positions for `xyz:NVDA`, `xyz:COIN`, and `xyz:GOOGL`.
  - The same query without `dex` returns `assetPositions: []`, which is the wrong perp scope for this account.
  - `userFunding` for the same address also shows current `xyz:NVDA`, `xyz:COIN`, and `xyz:GOOGL` funding deltas, confirming those positions are live.
- The current equities balances API still shows only Hyperliquid cash rows (`USDC`, `USDE`, `USDH`) plus IBKR stock positions.
- The current Hyperliquid shared-account projection provider is still incomplete in two distinct ways:
  - `systems/flux/flux/runners/shared/profile_accounts.py` posts manual `clearinghouseState` / `spotClearinghouseState` payloads without `dex`.
  - A direct live provider probe with `dex="xyz"` still returns only cash rows, which means the Hyperliquid adapter path `request_position_status_reports(..., dex="xyz")` is currently returning zero parsed reports for these builder-deployed perp positions.
- For now, the desired product behavior is to surface **all** live Hyperliquid `xyz` positions on the master account, not just positions that map to the retained 10 equities strategies. Filtering can be a later product decision.

## Acceptance Criteria

1. Shared-account Hyperliquid projection for `hyperliquid.xyz.main` includes:
   - spot cash rows from `spotClearinghouseState(dex="xyz")`
   - perp position rows from `clearinghouseState(dex="xyz")`
   - account totals from `clearinghouseState(dex="xyz")`
2. A fresh live `/api/v1/balances?profile=equities` includes Hyperliquid rows for `NVDA`, `COIN`, and `GOOGL` when those positions are live on the master account.
3. Hyperliquid row semantics remain correct:
   - cash rows => `product_type="spot"`, `contract_type="cash"`
   - perp positions => `product_type="perp"`, `contract_type="perp"`
4. Fluxboard holdings/risk views render the shared-account Hyperliquid rows and totals without hiding them behind legacy grouping or store dedupe bugs.
5. Strategy-local inventory and shared-account Hyperliquid rows coexist without row-id collisions or accidental deduplication in `portfolio_snapshot_v2`.
6. Live verification proves the raw Hyperliquid `xyz` position state and the balances API/GUI agree on the same positions.

## Progress Tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Owner | Depends On | Write Scope | Lane Branch | Worktree Path | Commit / Diff | Verification | Notes / Last Update |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| Overall | completed | main | none | `crates/adapters/hyperliquid`, `systems/flux/flux/runners/shared`, `systems/flux/flux/api`, `fluxboard`, `docs/plans` | `shared` | `shared` | working tree diff vs `HEAD` | local pytest/cargo/vitest checks pass; release rebuild + live/public balances verification pass after service restart | `2026-03-13 11:26 UTC` rebuilt the worktree Pyo3 extension and Fluxboard dist, restarted `flux@equities-portfolio`, `flux@equities-api`, and `flux@tokenmm-api`, and verified both local `:5024` and public `:5022` balances now show shared Hyperliquid `NVDA/COIN/GOOGL` positions plus master totals |
| Task 1: Lock The Hyperliquid XYZ Account Contract In Tests | completed | main | none | `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `fluxboard/api.flux.test.ts` | `shared` | `shared` | working tree diff vs `HEAD` | `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k hyperliquid -p no:rerunfailures` => pass; `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k hyperliquid -p no:rerunfailures` => pass; `pnpm --dir fluxboard exec vitest run api.flux.test.ts -t \"hyperliquid xyz shared account\"` => pass | `2026-03-13 11:19 UTC` the contract is now locked across provider, API, and frontend parser layers; task-owned expectations were aligned with real uppercase `XYZ:` instrument ids, freshness gating, and absolute `qty_raw` UI semantics. |
| Task 2: Fix Dex-Aware Hyperliquid Position Fetching In Provider And Adapter | completed | main | Task 1: Lock The Hyperliquid XYZ Account Contract In Tests | `systems/flux/flux/runners/shared/profile_accounts.py`, `crates/adapters/hyperliquid/src/http/client.rs`, `crates/adapters/hyperliquid/src/python/http.rs`, `crates/adapters/hyperliquid/tests/http.rs`, `crates/adapters/hyperliquid/tests/exec_client.rs` | `shared` | `shared` | working tree diff vs `HEAD` | `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k hyperliquid -p no:rerunfailures` => pass; `cargo test -p nautilus-hyperliquid test_http_client_request_position_status_reports_with_explicit_dex_returns_builder_positions -- --nocapture` => pass; `cargo test -p nautilus-hyperliquid test_exec_client_connect_uses_account_address_and_dex_for_clearinghouse_state -- --nocapture` => pass | `2026-03-13 11:24 UTC` provider now includes `dex` on raw info payloads, and the Rust client lazily loads + caches perp instruments before parsing builder `xyz:*` positions instead of silently skipping them |
| Task 3: Publish Shared Hyperliquid Cash Plus Perp Positions Into Portfolio Snapshot | completed | main | Task 2: Fix Dex-Aware Hyperliquid Position Fetching In Provider And Adapter | `systems/flux/flux/common/account_projection.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`, `systems/flux/flux/common/portfolio_snapshot.py`, `tests/unit_tests/flux/common/test_account_projection.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py` | `shared` | `shared` | working tree diff vs `HEAD` | `./.venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py -p no:rerunfailures` => pass; `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k "hyperliquid or profile_account_projection" -p no:rerunfailures` => pass | `2026-03-13 11:22 UTC` shared Hyperliquid cash + position rows now get stable scope-aware row IDs and shared-account totals survive through `portfolio_snapshot_v2` |
| Task 4: Correct Balances API Semantics For Shared Hyperliquid Positions | completed | main | Task 3: Publish Shared Hyperliquid Cash Plus Perp Positions Into Portfolio Snapshot | `systems/flux/flux/api/_payloads_common.py`, `systems/flux/flux/api/_payloads_balances.py`, `systems/flux/flux/api/app.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py` | `shared` | `shared` | working tree diff vs `HEAD` | `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k hyperliquid -p no:rerunfailures` => pass | `2026-03-13 11:22 UTC` balances API now carries shared Hyperliquid `USDE/NVDA/COIN/GOOGL` rows and totals cleanly through the equities profile contract |
| Task 5: Align Fluxboard Holdings And Risk Views With Shared Hyperliquid Rows | completed | main | Task 4: Correct Balances API Semantics For Shared Hyperliquid Positions | `fluxboard/api.ts`, `fluxboard/types.ts`, `fluxboard/Balances.tsx`, `fluxboard/Balances.test.tsx`, `fluxboard/api.flux.test.ts` | `shared` | `shared` | working tree diff vs `HEAD` | `pnpm --dir fluxboard exec vitest run api.flux.test.ts -t "hyperliquid xyz shared account"` => pass; `pnpm --dir fluxboard exec vitest run Balances.test.tsx` => pass | `2026-03-13 11:22 UTC` Fluxboard renders shared Hyperliquid rows/totals and now normalizes position quantities from `signed_qty` so short HL positions stay signed in the balances UI |
| Task 6: Re-Verify Live Equities Account State End To End | completed | main | Task 5: Align Fluxboard Holdings And Risk Views With Shared Hyperliquid Rows | `docs/plans/2026-03-12-equities-live-trading-readiness.md`, `docs/plans/2026-03-13-equities-prod-hardening-universe-pruning.md`, `docs/plans/2026-03-13-hyperliquid-xyz-account-surface.md` | `shared` | `shared` | release build completed in the worktree | `build.py` => pass; `pnpm --dir fluxboard build` => pass; restarted `flux@equities-portfolio`, `flux@equities-api`, `flux@tokenmm-api`; local/public balances endpoints now show shared HL positions + totals | `2026-03-13 11:26 UTC` live verification succeeded after restart: `http://127.0.0.1:5024/api/v1/balances?profile=equities` and `http://13.213.194.42:5022/api/v1/balances?profile=equities` both include `USDC`, `USDE`, `USDH`, and shared Hyperliquid `NVDA/COIN/GOOGL` perp rows with signed shorts and master account totals |

---

### Task 1: Lock The Hyperliquid XYZ Account Contract In Tests

**Files:**
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`
- Modify: `fluxboard/api.flux.test.ts`

**Dependencies:** `none`

**Write Scope:** `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`, `fluxboard/api.flux.test.ts`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k hyperliquid -p no:rerunfailures`
- `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k hyperliquid -p no:rerunfailures`
- `pnpm --dir fluxboard exec vitest run api.flux.test.ts -t "hyperliquid xyz shared account"`

**Step 1: Write the failing tests**

Add regressions that encode the desired contract:

```python
def test_hyperliquid_shared_projection_uses_xyz_dex_for_account_state_payloads(...) -> None:
    ...
    assert captured_info_payloads == [
        {"type": "clearinghouseState", "user": funded_account, "dex": "xyz"},
        {"type": "spotClearinghouseState", "user": funded_account, "dex": "xyz"},
    ]
```

```python
def test_hyperliquid_shared_projection_includes_xyz_perp_positions(...) -> None:
    assert {
        (row["exchange"], row["asset"], row.get("kind"), row.get("contract_type"))
        for row in snapshot["rows"]
    } >= {
        ("hyperliquid", "NVDA", "position", "perp"),
        ("hyperliquid", "COIN", "position", "perp"),
        ("hyperliquid", "GOOGL", "position", "perp"),
    }
```

```python
def test_equities_balances_profile_contract_includes_shared_hyperliquid_xyz_positions() -> None:
    ...
    assert {row["coin"] for row in hyperliquid_position_rows} >= {"NVDA", "COIN", "GOOGL"}
```

Add the matching Fluxboard contract test so the frontend payload parser expects those Hyperliquid position rows instead of only cash rows.

**Step 2: Run tests to verify they fail**

Run the verification commands above.

Expected: FAIL because the current provider still omits `dex` on manual account-state requests and no Hyperliquid position rows reach the API payload.

**Step 3: Write minimal implementation**

Do not implement full behavior yet. Only keep the tests in place and capture the exact failing shape so the adapter/provider work in Task 2 is targeted.

**Step 4: Re-run the focused tests**

Run the same verification commands and confirm they still fail for the intended missing behavior, not for syntax or fixture errors.

**Step 5: Commit**

```bash
git add \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py \
  fluxboard/api.flux.test.ts
git commit -m "test: lock hyperliquid xyz shared-account contract"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 2: Fix Dex-Aware Hyperliquid Position Fetching In Provider And Adapter

**Files:**
- Modify: `systems/flux/flux/runners/shared/profile_accounts.py`
- Modify: `crates/adapters/hyperliquid/src/http/client.rs`
- Modify: `crates/adapters/hyperliquid/src/python/http.rs`
- Modify: `crates/adapters/hyperliquid/tests/http.rs`
- Modify: `crates/adapters/hyperliquid/tests/exec_client.rs`

**Dependencies:** `Task 1: Lock The Hyperliquid XYZ Account Contract In Tests`

**Write Scope:** `systems/flux/flux/runners/shared/profile_accounts.py`, `crates/adapters/hyperliquid/src/http/client.rs`, `crates/adapters/hyperliquid/src/python/http.rs`, `crates/adapters/hyperliquid/tests/http.rs`, `crates/adapters/hyperliquid/tests/exec_client.rs`

**Verification Commands:**
- `cargo test -p nautilus-hyperliquid request_position_status_reports_with_dex -- --nocapture`
- `cargo test -p nautilus-hyperliquid test_exec_client_connect_uses_account_address_and_dex_for_clearinghouse_state -- --nocapture`
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k hyperliquid -p no:rerunfailures`

**Step 1: Write the failing Rust regression**

Add a regression proving builder-deployed `xyz:*` positions are parsed when `dex="xyz"` is supplied:

```rust
#[tokio::test]
async fn test_http_client_request_position_status_reports_with_xyz_dex_returns_builder_positions() {
    // mock clearinghouseState returns assetPositions for xyz:NVDA / xyz:COIN / xyz:GOOGL
    // assert len == 3 and instrument ids map to xyz:NVDA-USD-PERP.HYPERLIQUID etc.
}
```

Add a focused Python/provider regression if needed to assert the provider passes `dex` through to both:
- `request_position_status_reports(..., dex="xyz")`
- `_post_hyperliquid_info(... payload={"type":"clearinghouseState", ..., "dex":"xyz"})`

**Step 2: Run tests to verify they fail**

Run the verification commands above.

Expected: FAIL because the current live-shaped `xyz:*` positions do not parse into reports.

**Step 3: Write minimal implementation**

Fix the actual root causes:
- in `profile_accounts.py`, include `dex` in the manual `clearinghouseState` and `spotClearinghouseState` payloads
- in the Hyperliquid adapter/client path, make `request_position_status_reports_with_dex` return reports for builder-deployed `xyz:*` positions instead of zero
- keep the API contract explicit: the same configured `dex` must drive both totals/cash queries and position queries

If the root cause is instrument lookup for `xyz:*` coins, fix that in the adapter rather than adding provider-side special-case parsing.

**Step 4: Run tests to verify they pass**

Run:

```bash
cargo test -p nautilus-hyperliquid request_position_status_reports_with_dex -- --nocapture
cargo test -p nautilus-hyperliquid test_exec_client_connect_uses_account_address_and_dex_for_clearinghouse_state -- --nocapture
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k hyperliquid -p no:rerunfailures
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/runners/shared/profile_accounts.py \
  crates/adapters/hyperliquid/src/http/client.rs \
  crates/adapters/hyperliquid/src/python/http.rs \
  crates/adapters/hyperliquid/tests/http.rs \
  crates/adapters/hyperliquid/tests/exec_client.rs
git commit -m "fix(hyperliquid): fetch xyz master positions in shared account projection"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 3: Publish Shared Hyperliquid Cash Plus Perp Positions Into Portfolio Snapshot

**Files:**
- Modify: `systems/flux/flux/common/account_projection.py`
- Modify: `systems/flux/flux/runners/shared/portfolio_runner.py`
- Modify: `systems/flux/flux/common/portfolio_snapshot.py`
- Modify: `tests/unit_tests/flux/common/test_account_projection.py`
- Modify: `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Dependencies:** `Task 2: Fix Dex-Aware Hyperliquid Position Fetching In Provider And Adapter`

**Write Scope:** `systems/flux/flux/common/account_projection.py`, `systems/flux/flux/runners/shared/portfolio_runner.py`, `systems/flux/flux/common/portfolio_snapshot.py`, `tests/unit_tests/flux/common/test_account_projection.py`, `tests/unit_tests/examples/strategies/test_equities_run_portfolio.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py -p no:rerunfailures`
- `./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k "hyperliquid or profile_account_projection" -p no:rerunfailures`

**Step 1: Write the failing tests**

Add snapshot-level regressions proving shared Hyperliquid position rows survive projection and portfolio aggregation:

```python
def test_profile_account_projection_assigns_stable_row_ids_for_hyperliquid_positions() -> None:
    assert row_ids >= {
        "equities:shared:hyperliquid.xyz.main:pos:hyperliquid:HYPERLIQUID-master:XYZ:NVDA-USD-PERP.HYPERLIQUID",
    }
```

```python
def test_equities_portfolio_snapshot_v2_keeps_shared_hyperliquid_cash_and_perp_rows() -> None:
    assert hyperliquid_cash_rows
    assert hyperliquid_position_rows
```

**Step 2: Run tests to verify they fail**

Run the verification commands above.

Expected: FAIL because row ids, snapshot merge, or downstream aggregation still assume cash-only Hyperliquid account rows.

**Step 3: Write minimal implementation**

Update the shared-account projection and snapshot merge paths so:
- Hyperliquid shared-account position rows get stable, non-colliding row ids
- shared-account Hyperliquid positions are not deduped away by portfolio snapshot semantics
- shared-account Hyperliquid totals remain additive with the existing IBKR shared-account rows

Do not conflate shared-account Hyperliquid positions with strategy-local inventory components.

**Step 4: Run tests to verify they pass**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/common/test_account_projection.py -p no:rerunfailures
./.venv/bin/pytest -q tests/unit_tests/examples/strategies/test_equities_run_portfolio.py -k "hyperliquid or profile_account_projection" -p no:rerunfailures
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/common/account_projection.py \
  systems/flux/flux/runners/shared/portfolio_runner.py \
  systems/flux/flux/common/portfolio_snapshot.py \
  tests/unit_tests/flux/common/test_account_projection.py \
  tests/unit_tests/examples/strategies/test_equities_run_portfolio.py
git commit -m "fix(equities): carry shared hyperliquid xyz positions through portfolio snapshot"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 4: Correct Balances API Semantics For Shared Hyperliquid Positions

**Files:**
- Modify: `systems/flux/flux/api/_payloads_common.py`
- Modify: `systems/flux/flux/api/_payloads_balances.py`
- Modify: `systems/flux/flux/api/app.py`
- Modify: `tests/unit_tests/flux/api/test_payloads.py`
- Modify: `tests/unit_tests/flux/api/test_app.py`
- Modify: `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Dependencies:** `Task 3: Publish Shared Hyperliquid Cash Plus Perp Positions Into Portfolio Snapshot`

**Write Scope:** `systems/flux/flux/api/_payloads_common.py`, `systems/flux/flux/api/_payloads_balances.py`, `systems/flux/flux/api/app.py`, `tests/unit_tests/flux/api/test_payloads.py`, `tests/unit_tests/flux/api/test_app.py`, `tests/unit_tests/flux/api/test_equities_profile_contract.py`

**Verification Commands:**
- `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_payloads.py -k hyperliquid -p no:rerunfailures`
- `./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k balances -p no:rerunfailures`

**Step 1: Write the failing tests**

Add payload regressions for Hyperliquid shared-account positions:

```python
def test_balances_payload_classifies_hyperliquid_xyz_positions_as_perp() -> None:
    assert row["product_type"] == "perp"
    assert row["contract_type"] == "perp"
    assert row["display_name_short"] == "NVDA Perp"
```

```python
def test_equities_balances_api_returns_all_live_hyperliquid_xyz_positions() -> None:
    assert {row["coin"] for row in hyperliquid_rows if row.get("kind") == "position"} >= {"NVDA", "COIN", "GOOGL"}
```

**Step 2: Run tests to verify they fail**

Run the verification commands above.

Expected: FAIL because the API currently exposes only Hyperliquid cash rows.

**Step 3: Write minimal implementation**

Update the balances payload shaping so:
- Hyperliquid shared-account cash and perp rows both serialize correctly
- row naming and contract semantics are correct for builder-deployed perps
- totals stay intact and no IBKR equity semantics regress

**Step 4: Run tests to verify they pass**

Run:

```bash
./.venv/bin/pytest -q tests/unit_tests/flux/api/test_payloads.py -k hyperliquid -p no:rerunfailures
./.venv/bin/pytest -q tests/unit_tests/flux/api/test_app.py tests/unit_tests/flux/api/test_equities_profile_contract.py -k balances -p no:rerunfailures
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  systems/flux/flux/api/_payloads_common.py \
  systems/flux/flux/api/_payloads_balances.py \
  systems/flux/flux/api/app.py \
  tests/unit_tests/flux/api/test_payloads.py \
  tests/unit_tests/flux/api/test_app.py \
  tests/unit_tests/flux/api/test_equities_profile_contract.py
git commit -m "fix(equities): expose shared hyperliquid xyz positions in balances api"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 5: Align Fluxboard Holdings And Risk Views With Shared Hyperliquid Rows

**Files:**
- Modify: `fluxboard/api.ts`
- Modify: `fluxboard/types.ts`
- Modify: `fluxboard/Balances.tsx`
- Modify: `fluxboard/Balances.test.tsx`
- Modify: `fluxboard/api.flux.test.ts`

**Dependencies:** `Task 4: Correct Balances API Semantics For Shared Hyperliquid Positions`

**Write Scope:** `fluxboard/api.ts`, `fluxboard/types.ts`, `fluxboard/Balances.tsx`, `fluxboard/Balances.test.tsx`, `fluxboard/api.flux.test.ts`

**Verification Commands:**
- `pnpm --dir fluxboard exec vitest run api.flux.test.ts -t "hyperliquid xyz shared account"`
- `pnpm --dir fluxboard exec vitest run Balances.test.tsx -t "hyperliquid|shared account|perp"`
- `pnpm --dir fluxboard build`

**Step 1: Write the failing tests**

Add UI regressions that prove the frontend stays in sync with the API contract:

```tsx
it('renders shared hyperliquid xyz perp rows alongside cash rows', async () => {
  expect(screen.getByText('NVDA')).toBeInTheDocument();
  expect(screen.getByText('NVDA Perp')).toBeInTheDocument();
  expect(screen.getByText('USDE')).toBeInTheDocument();
});
```

```ts
it('normalizes hyperliquid shared xyz perp rows without collapsing them into cash rows', () => {
  expect(payload.rows.find((row) => row.coin === 'NVDA')?.children?.[0]?.contract_type).toBe('perp');
});
```

**Step 2: Run tests to verify they fail**

Run the verification commands above.

Expected: FAIL because the frontend contracts currently only exercise Hyperliquid cash rows.

**Step 3: Write minimal implementation**

Update Fluxboard normalization and rendering so:
- Hyperliquid shared cash and shared perp rows both display correctly
- totals remain visible
- no legacy grouping hides the new Hyperliquid perp rows
- UI stays faithful to the backend payload rather than adding venue-specific hacks

**Step 4: Run tests to verify they pass**

Run:

```bash
pnpm --dir fluxboard exec vitest run api.flux.test.ts -t "hyperliquid xyz shared account"
pnpm --dir fluxboard exec vitest run Balances.test.tsx -t "hyperliquid|shared account|perp"
pnpm --dir fluxboard build
```

Expected: PASS.

**Step 5: Commit**

```bash
git add \
  fluxboard/api.ts \
  fluxboard/types.ts \
  fluxboard/Balances.tsx \
  fluxboard/Balances.test.tsx \
  fluxboard/api.flux.test.ts
git commit -m "fix(fluxboard): render shared hyperliquid xyz positions"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.

### Task 6: Re-Verify Live Equities Account State End To End

**Files:**
- Modify: `docs/plans/2026-03-12-equities-live-trading-readiness.md`
- Modify: `docs/plans/2026-03-13-equities-prod-hardening-universe-pruning.md`
- Modify: `docs/plans/2026-03-13-hyperliquid-xyz-account-surface.md`

**Dependencies:** `Task 5: Align Fluxboard Holdings And Risk Views With Shared Hyperliquid Rows`

**Write Scope:** `docs/plans/2026-03-12-equities-live-trading-readiness.md`, `docs/plans/2026-03-13-equities-prod-hardening-universe-pruning.md`, `docs/plans/2026-03-13-hyperliquid-xyz-account-surface.md`

**Verification Commands:**
- `curl -fsS http://127.0.0.1:5024/api/v1/balances?profile=equities | jq '{hyperliquid_rows: [.data.rows[] | select(.exchange=="hyperliquid") | {coin, kind, product_type, contract_type, signed_qty, mv_raw}], totals: .data.totals}'`
- `./.venv/bin/python - <<'PY' ... clearinghouseState/spotClearinghouseState with dex="xyz" ... PY`
- `pnpm --dir fluxboard build`
- `git diff --check`

**Step 1: Rebuild and roll out from the worktree**

After Tasks 1-5 are green, rebuild the Fluxboard bundle and, if Python/Rust code changed, rebuild the worktree-backed runtime artifacts before restarting only the affected services.

**Step 2: Verify raw Hyperliquid API and balances API agree**

Run a fresh raw `POST /info` verification for:
- `clearinghouseState(dex="xyz")`
- `spotClearinghouseState(dex="xyz")`

Then compare it with `/api/v1/balances?profile=equities`.

Expected: the same live Hyperliquid `NVDA`, `COIN`, and `GOOGL` positions appear in both.

**Step 3: Verify Fluxboard**

Check `/equities/balances` on both:
- `http://127.0.0.1:5024/equities`
- `http://13.213.194.42:5022/equities`

Expected: shared Hyperliquid cash rows, shared Hyperliquid perp rows, and account totals are all visible together.

**Step 4: Update the trackers**

Record:
- the exact live Hyperliquid rows now surfaced
- whether any non-retained `xyz` positions are present
- whether any remaining mismatch remains between raw Hyperliquid state and balances API

**Step 5: Commit**

```bash
git add \
  docs/plans/2026-03-12-equities-live-trading-readiness.md \
  docs/plans/2026-03-13-equities-prod-hardening-universe-pruning.md \
  docs/plans/2026-03-13-hyperliquid-xyz-account-surface.md
git commit -m "docs(equities): record full hyperliquid xyz account integration verification"
```

**Progress Updates:** After finishing any step that changes task state, commit state, or verification state, update the Progress Tracker before moving on.
