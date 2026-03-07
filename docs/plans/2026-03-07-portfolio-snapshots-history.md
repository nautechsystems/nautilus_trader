# Portfolio Snapshots History Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Persist historical strategy balance/position snapshots and TokenMM shared portfolio inventory snapshots to local SQLite and ship them to the shared RDS PostgreSQL sink for historical reconciliation.

**Architecture:** Keep the existing live/latest portfolio surfaces unchanged: Redis remains the fast latest-only read path, while historical storage is append-only and off the trading hot path. Add two new persistence surfaces: a Flux balance-snapshot persistence actor that records per-strategy account/position snapshots from the existing `*.balances` payloads, and a TokenMM portfolio-inventory snapshot writer in `run_portfolio` that records the shared aggregate inventory feed. Reuse the current `local SQLite -> async shipper -> RDS PostgreSQL` topology.

**Tech Stack:** Python, SQLite WAL, psycopg/PostgreSQL, Flux Redis, TokenMM runners, pytest, ruff.

---

## Design choice

### Recommended approach

Persist historical portfolio state from the existing canonical producers:

1. `flux.makerv3.balances` becomes the source for strategy-local historical account/position snapshots.
2. `flux.runners.tokenmm.run_portfolio` becomes the source for shared TokenMM aggregate inventory history.
3. Both write locally first, then ship asynchronously to the shared RDS telemetry sink.

This keeps the trading/runtime hot path isolated, preserves the current Redis/API contract, and gives SQL-friendly historical reconciliation.

### Alternatives considered

1. **Redis stream history only**
   - Pros: fast to wire into current Flux bridge.
   - Cons: wrong durability layer, duplicates retention logic, and still needs a second exporter to reach RDS.

2. **Direct RDS writes from strategy or portfolio processes**
   - Pros: fewer moving parts.
   - Cons: network DB on the live path, worse failure coupling, contradicts the persistence design we just established.

3. **One giant JSON snapshot table**
   - Pros: simplest write path.
   - Cons: poor queryability for reconciliation, hard to diff positions over time, and awkward to join to fills/orders.

Recommendation: use **queryable normalized rows plus raw snapshot grouping IDs**, backed by local SQLite and the existing shipper.

## Data model

### 1. Strategy balance snapshots

Create two new tables:

- `flux_balance_snapshot`
  - one row per emitted balance snapshot payload
  - fields:
    - `trader_id`
    - `strategy_id`
    - `snapshot_id`
    - `topic`
    - `snapshot_hash`
    - `ts_event_ns`
    - `ts_ms`
    - `ts_ingest_ns`
    - `account_count`
    - `position_count`
    - `payload_json`
    - `created_at`
- `flux_balance_snapshot_row`
  - one normalized row per cash/position row inside the snapshot
  - fields:
    - `trader_id`
    - `strategy_id`
    - `snapshot_id`
    - `row_key`
    - `kind`
    - `exchange`
    - `account_id`
    - `account`
    - `asset`
    - `instrument_id`
    - `side`
    - `signed_qty`
    - `quantity`
    - `free`
    - `locked`
    - `total`
    - `avg_px_open`
    - `avg_px_close`
    - `realized_pnl`
    - `ts_ms`
    - `row_json`
    - `created_at`

Identity and dedupe:

- `snapshot_id` must be deterministic, derived from canonical JSON:
  - `sha256(strategy_id + topic + canonical_payload_json)`
- `row_key` must be deterministic inside a snapshot:
  - cash rows: `{exchange}:{account_id_or_account}:{asset}`
  - position rows: `{exchange}:{instrument_id}:{position_id_or_side}`

Write policy:

- persist on payload change
- also persist an unchanged snapshot at a configurable heartbeat interval so liveness gaps are visible

### 2. TokenMM shared portfolio inventory snapshots

Create one table for v1:

- `portfolio_inventory_snapshot`
  - one row per persisted aggregate payload from `run_portfolio`
  - fields:
    - `portfolio_id`
    - `base_currency`
    - `snapshot_id`
    - `snapshot_hash`
    - `global_qty`
    - `degraded`
    - `missing_required_json`
    - `components_json`
    - `ts_ms`
    - `ts_ingest_ns`
    - `created_at`

Identity and dedupe:

- `snapshot_id = sha256(portfolio_id + base_currency + canonical_payload_json)`

Write policy:

- persist only when the aggregate payload changes
- also persist an unchanged payload at a heartbeat interval
- do **not** write every `0.25s` recompute tick if the payload is unchanged

### 3. Shared RDS sink

Extend the existing shipper and Postgres sink with:

- `telemetry.flux_balance_snapshot`
- `telemetry.flux_balance_snapshot_row`
- `telemetry.portfolio_inventory_snapshot`

As with the other telemetry tables, sink identity must include `source_profile` so TokenMM and future equities profiles can share one physical database safely.

## Scope decisions

- Keep Redis latest-only reads exactly as they are.
- Do **not** add a historical API endpoint in this wave.
- Do **not** add direct RDS writes from nodes or `run_portfolio`.
- Do **not** create a separate equities database; reuse the shared RDS DB with `source_profile`.
- Do **not** store market-value enrichment in v1 unless it is already present in the emitted payload. Historical mark/mv can be joined later from market data if needed.

## Task 1: Add normalized balance snapshot persistence surface

**Files:**
- Create: `systems/flux/flux/persistence/balance_snapshots/__init__.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/config.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/schema.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/sqlite.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/actor.py`
- Create: `systems/flux/flux/persistence/balance_snapshots/normalize.py`
- Test: `tests/unit_tests/persistence/test_flux_balance_snapshot_sqlite.py`
- Test: `tests/unit_tests/persistence/test_flux_balance_snapshot_actor.py`

**Step 1: Write the failing normalization tests**

```python
def test_balance_snapshot_normalizer_flattens_accounts_and_positions() -> None:
    snapshot, rows = normalize_balance_snapshot(
        trader_id="TRADER-001",
        strategy_id="maker_v3_01",
        topic="flux.makerv3.balances",
        payload={
            "strategy_id": "maker_v3_01",
            "accounts": [{"account_id": "BYBIT-001", "events": [{"account_id": "BYBIT-001", "balances": [{"currency": "PLUME", "total": "100"}]}]}],
            "positions": [{"instrument_id": "PLUMEUSDT.BYBIT_SPOT", "signed_qty": "50", "side": "LONG"}],
            "ts_event": 123_000_000_000,
            "ts_ms": 123_000,
        },
        ts_ingest_ns=124_000_000_000,
    )
    assert snapshot["strategy_id"] == "maker_v3_01"
    assert len(rows) == 2
```

**Step 2: Run test to verify it fails**

Run: `uv run --active --no-sync pytest tests/unit_tests/persistence/test_flux_balance_snapshot_sqlite.py::test_balance_snapshot_normalizer_flattens_accounts_and_positions -q`

Expected: FAIL because the new persistence surface does not exist yet.

**Step 3: Write the minimal normalization + schema implementation**

Implementation notes:

- keep normalization deterministic and ASCII-safe
- compute `snapshot_hash` / `snapshot_id` from canonical compact JSON
- move JSON encoding and flattening off the publish hot path, in the worker thread
- store `payload_json` once on the header table and `row_json` once per normalized row

**Step 4: Run the focused tests**

Run: `uv run --active --no-sync pytest tests/unit_tests/persistence/test_flux_balance_snapshot_sqlite.py tests/unit_tests/persistence/test_flux_balance_snapshot_actor.py -q`

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/persistence/balance_snapshots tests/unit_tests/persistence/test_flux_balance_snapshot_sqlite.py tests/unit_tests/persistence/test_flux_balance_snapshot_actor.py
git commit -m "feat: persist historical flux balance snapshots"
```

## Task 2: Wire balance snapshot persistence into TokenMM nodes

**Files:**
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Step 1: Write the failing wiring test**

```python
def test_build_node_adds_balance_snapshot_actor_when_enabled() -> None:
    actor_paths = [cfg.actor_path for cfg in build_node(...).config.actors]
    assert "flux.persistence.balance_snapshots.actor:FluxBalanceSnapshotPersistenceActor" in actor_paths
```

**Step 2: Run test to verify it fails**

Run: `uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k balance_snapshot -q`

Expected: FAIL because the shared config does not yet wire the actor.

**Step 3: Implement the node wiring**

Implementation notes:

- add a `[portfolio_snapshots]` table to `deploy/tokenmm/tokenmm.live.toml`
- include local SQLite paths under `/var/lib/nautilus/telemetry/tokenmm`
- attach the actor only when local persistence is enabled
- subscribe to the existing balance topic, not a new strategy payload

**Step 4: Run the focused tests**

Run: `uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -q`

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/tokenmm/run_node.py deploy/tokenmm/tokenmm.live.toml tests/unit_tests/examples/strategies/test_tokenmm_run_node.py
git commit -m "feat: wire balance snapshot persistence into tokenmm nodes"
```

## Task 3: Add TokenMM aggregate portfolio inventory history

**Files:**
- Create: `systems/flux/flux/persistence/portfolio_inventory_snapshots/__init__.py`
- Create: `systems/flux/flux/persistence/portfolio_inventory_snapshots/schema.py`
- Create: `systems/flux/flux/persistence/portfolio_inventory_snapshots/sqlite.py`
- Modify: `systems/flux/flux/runners/tokenmm/run_portfolio.py`
- Test: `tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py`
- Test: `tests/unit_tests/persistence/test_portfolio_inventory_snapshot_sqlite.py`

**Step 1: Write the failing persistence-on-change test**

```python
def test_run_portfolio_persists_only_changed_or_heartbeat_snapshots(tmp_path: Path) -> None:
    writer = PortfolioInventorySnapshotWriter(...)
    writer.maybe_persist(payload={"portfolio_id": "tokenmm", "base_currency": "PLUME", "global_qty": "10", "components": []}, ts_ms=1)
    writer.maybe_persist(payload={"portfolio_id": "tokenmm", "base_currency": "PLUME", "global_qty": "10", "components": []}, ts_ms=2)
    assert writer.count() == 1
```

**Step 2: Run test to verify it fails**

Run: `uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py -k snapshot -q`

Expected: FAIL because `run_portfolio` has no historical writer yet.

**Step 3: Implement the writer**

Implementation notes:

- write from `run_portfolio`, not from Redis reads
- hash the encoded aggregate payload to dedupe unchanged states
- persist a heartbeat row after a configurable interval even when unchanged
- keep SQLite writes outside the strategy hot path; `run_portfolio` itself is not latency-sensitive

**Step 4: Run the focused tests**

Run: `uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py tests/unit_tests/persistence/test_portfolio_inventory_snapshot_sqlite.py -q`

Expected: PASS

**Step 5: Commit**

```bash
git add systems/flux/flux/persistence/portfolio_inventory_snapshots systems/flux/flux/runners/tokenmm/run_portfolio.py tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py tests/unit_tests/persistence/test_portfolio_inventory_snapshot_sqlite.py
git commit -m "feat: persist tokenmm portfolio inventory history"
```

## Task 4: Extend the telemetry shipper and Postgres sink

**Files:**
- Modify: `nautilus_trader/persistence/shipper/config.py`
- Modify: `nautilus_trader/persistence/shipper/postgres.py`
- Modify: `nautilus_trader/persistence/shipper/service.py`
- Test: `tests/unit_tests/persistence/test_telemetry_shipper.py`

**Step 1: Write the failing shipper test**

```python
def test_shipper_ships_balance_and_inventory_snapshot_tables(tmp_path: Path) -> None:
    shipper = SQLiteToPostgresTelemetryShipper(...)
    result = shipper.ship_once()
    assert result["flux_balance_snapshot"].shipped == 1
    assert result["portfolio_inventory_snapshot"].shipped == 1
```

**Step 2: Run test to verify it fails**

Run: `uv run --active --no-sync pytest tests/unit_tests/persistence/test_telemetry_shipper.py -k snapshot -q`

Expected: FAIL because the shipper does not know about the new tables.

**Step 3: Implement the sink/table support**

Implementation notes:

- add local DB path config for the two new sources
- add RDS DDL for the new tables
- include `source_profile` in sink identity keys
- keep rowid cursoring and pruning behavior aligned with the existing shipper

**Step 4: Run the focused tests**

Run: `uv run --active --no-sync pytest tests/unit_tests/persistence/test_telemetry_shipper.py -q`

Expected: PASS

**Step 5: Commit**

```bash
git add nautilus_trader/persistence/shipper tests/unit_tests/persistence/test_telemetry_shipper.py
git commit -m "feat: ship portfolio snapshot history to postgres"
```

## Task 5: Update deploy surfaces and runbook

**Files:**
- Modify: `deploy/tokenmm/systemd/common.env.example`
- Modify: `deploy/tokenmm/README.md`
- Modify: `deploy/tokenmm/TELEMETRY_RDS_RUNBOOK.md`
- Modify: `ops/scripts/deploy/install_tokenmm_systemd.sh`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py`

**Step 1: Write the failing deploy-contract test**

```python
def test_tokenmm_stack_contract_mentions_portfolio_snapshot_history() -> None:
    assert "portfolio_inventory_snapshot" in readme
    assert "flux_balance_snapshot" in runbook
```

**Step 2: Run test to verify it fails**

Run: `uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -k snapshot -q`

Expected: FAIL because the deploy docs do not mention the new historical surfaces yet.

**Step 3: Update deploy/docs**

Documentation requirements:

- explain that Redis remains latest-only
- explain that historical portfolio reconciliation comes from RDS
- document the one-DB policy for TokenMM + future equities
- document retention and dedupe behavior for unchanged snapshots

**Step 4: Run the focused tests**

Run: `uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q`

Expected: PASS

**Step 5: Commit**

```bash
git add deploy/tokenmm ops/scripts/deploy/install_tokenmm_systemd.sh tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
git commit -m "docs: add portfolio snapshot deployment guidance"
```

## Task 6: Add analyst-facing SQL examples and history semantics docs

**Files:**
- Modify: `systems/flux/docs/redis_schema.md`
- Create: `systems/flux/docs/portfolio_history.md`
- Modify: `docs/concepts/execution.md`

**Step 1: Write the docs update**

Required sections:

- current-state vs historical-state distinction
- safe join keys:
  - `strategy_id`
  - `account_id`
  - `instrument_id`
  - `snapshot_id`
  - `quote_cycle_id` when reconciling a position/fill sequence
- sample SQL:
  - latest known position before a fill
  - position changes between two times
  - TokenMM aggregate global inventory by base asset over time
  - per-strategy vs aggregate reconciliation at a timestamp

**Step 2: Run documentation contract tests if added**

Run: `uv run --active --no-sync pytest tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q`

Expected: PASS

**Step 3: Commit**

```bash
git add systems/flux/docs docs/concepts/execution.md
git commit -m "docs: describe historical portfolio snapshot model"
```

## Task 7: Full verification pass

**Files:**
- Verify the full touched surface only

**Step 1: Run unit tests**

Run:

```bash
uv run --active --no-sync pytest \
  tests/unit_tests/persistence/test_flux_balance_snapshot_sqlite.py \
  tests/unit_tests/persistence/test_flux_balance_snapshot_actor.py \
  tests/unit_tests/persistence/test_portfolio_inventory_snapshot_sqlite.py \
  tests/unit_tests/persistence/test_telemetry_shipper.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py -q
```

Expected: PASS

**Step 2: Run static checks**

Run:

```bash
uv run --active --no-sync ruff check \
  systems/flux/flux/persistence/balance_snapshots \
  systems/flux/flux/persistence/portfolio_inventory_snapshots \
  systems/flux/flux/runners/tokenmm/run_node.py \
  systems/flux/flux/runners/tokenmm/run_portfolio.py \
  nautilus_trader/persistence/shipper \
  tests/unit_tests/persistence/test_flux_balance_snapshot_sqlite.py \
  tests/unit_tests/persistence/test_flux_balance_snapshot_actor.py \
  tests/unit_tests/persistence/test_portfolio_inventory_snapshot_sqlite.py \
  tests/unit_tests/persistence/test_telemetry_shipper.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py \
  tests/unit_tests/examples/strategies/test_tokenmm_run_portfolio.py \
  tests/unit_tests/examples/strategies/test_tokenmm_stack_contract.py
python3 -m py_compile \
  systems/flux/flux/persistence/balance_snapshots/*.py \
  systems/flux/flux/persistence/portfolio_inventory_snapshots/*.py \
  systems/flux/flux/runners/tokenmm/run_node.py \
  systems/flux/flux/runners/tokenmm/run_portfolio.py \
  nautilus_trader/persistence/shipper/*.py
git diff --check
```

Expected: all commands pass with no diff hygiene issues.

**Step 3: Final commit**

```bash
git add systems/flux nautilus_trader/persistence/shipper deploy/tokenmm docs tests
git commit -m "feat: add historical portfolio snapshot persistence"
```

## Acceptance criteria

- Historical strategy-local account and position snapshots are queryable from RDS.
- Historical TokenMM aggregate inventory is queryable from RDS.
- Live Redis/API balances behavior is unchanged.
- No direct network DB writes are added to strategy hot paths.
- Shared RDS database remains safe for both TokenMM and future equities via `source_profile`-scoped sink identity.
- Unchanged portfolio states do not produce unbounded write amplification; dedupe + heartbeat behavior is documented and tested.

Plan complete and saved to `docs/plans/2026-03-07-portfolio-snapshots-history.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
