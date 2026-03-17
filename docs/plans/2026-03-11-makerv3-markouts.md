# MakerV3 Markouts Implementation Plan

**Goal:** Produce same-day preliminary MakerV3 markout numbers using existing Redis telemetry, then add a minimal live-forward persistence path for 30s, 60s, and 120s markouts vs `fv_market` mid. For v0, treat `fv_market` as the existing `fv` field published on `flux.makerv3.fv` unless product requirements say otherwise.

**Architecture:** Split the work into two layers. First, add a read-only reporting script that reads the existing `flux:v1:trades:stream:{strategy_id}` and `flux:v1:fv:stream:{strategy_id}` streams so we can compute preliminary numbers immediately when Redis retention still covers the requested horizons. Second, add a lightweight local persistence actor that subscribes to fills and `flux.makerv3.fv`, resolves the 30s/60s/120s markouts online, and stores only final markout rows in SQLite next to existing fills and orders; do not persist a raw live market-data history in v0. The broader Nautilus live `streaming` / catalog path stays out of scope for this first PR because the Flux live runner does not wire it today and it is heavier than needed for same-day markout delivery.

**Tech Stack:** Flux Redis bridge, MakerV3 topics (`flux.makerv3.trade`, `flux.makerv3.fv`, `events.fills.*`), TokenMM live runner, SQLite persistence actors, Python CLI/reporting, pytest.

## Progress tracker

**Source of truth:** Update this table whenever task state changes. Do not rely on memory, chat history, or TodoWrite alone.

| Task | Status | Notes / last update |
| --- | --- | --- |
| Overall | completed | 2026-03-11: Task 1 report path, Task 2 live-forward persistence, Task 3 TokenMM wiring, and Task 4 operator docs are implemented and verified. |
| Task 1: Build The Read-Only Redis Markout Report | completed | 2026-03-11: completed after fixing the FV-window truncation bug and adding stable `fill_id` output; verification: `pytest -q --noconftest tests/unit_tests/ops/test_makerv3_markouts.py` (`7 passed`), `python ops/scripts/makerv3_markouts.py --help`, and `git diff --check`; residual gap: no end-to-end test yet for bridge-shaped Redis payloads or live profile-config resolution. |
| Task 2: Add Live-Forward Markout Persistence | completed | 2026-03-11: completed after actor and test implementation; verification: `pytest -q tests/unit_tests/persistence/test_markout_persistence_actor.py` (`3 passed`) plus adjacent persistence suites (`48 passed`). |
| Task 3: Wire Markouts Into TokenMM Telemetry Config | completed | 2026-03-11: completed with TokenMM runner wiring and deploy defaults; verification: `pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k markout` (`3 passed`) and the full run-node suite (`42 passed`). |
| Task 4: Document The Operator Workflow And Future Design Boundaries | completed | 2026-03-11: completed with `docs/runbooks/makerv3-markouts.md`, the MakerV3 observability doc update, and the docs contract test; verification: `pytest -q tests/unit_tests/docs/test_makerv3_markouts_docs.py` (`1 passed`) and the combined ops/docs slice (`8 passed`). |

---

### Task 1: Build the read-only Redis markout report

**Files:**
- Create: `ops/scripts/makerv3_markouts.py`
- Create: `tests/unit_tests/ops/test_makerv3_markouts.py`

**Step 1: Write the failing test**

Create `tests/unit_tests/ops/test_makerv3_markouts.py` with deterministic fixtures for trades and FV rows. Cover:

- selecting the first FV sample at or after `fill_ts_ms + horizon_s * 1000`
- signed markout math (`BUY => future_fv - fill_px`, `SELL => fill_px - future_fv`)
- summary aggregation by strategy and horizon

```python
from decimal import Decimal

from ops.scripts.makerv3_markouts import compute_markout_rows
from ops.scripts.makerv3_markouts import summarize_markout_rows


def test_compute_markout_rows_uses_first_fv_at_or_after_each_horizon() -> None:
    trade_rows = [
        {
            "strategy_id": "plumeusdt_bybit_perp_makerv3",
            "trade_id": "trade-1",
            "side": "BUY",
            "price": "100",
            "qty": "2",
            "ts_ms": 1_000,
        },
    ]
    fv_rows = [
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "101", "ts_ms": 31_000},
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "103", "ts_ms": 61_500},
        {"strategy_id": "plumeusdt_bybit_perp_makerv3", "fv": "104", "ts_ms": 121_250},
    ]

    rows = compute_markout_rows(trade_rows=trade_rows, fv_rows=fv_rows, horizons_s=(30, 60, 120))

    assert [(row["trade_id"], row["horizon_s"]) for row in rows] == [
        ("trade-1", 30),
        ("trade-1", 60),
        ("trade-1", 120),
    ]
    assert rows[0]["markout_abs"] == Decimal("1")
    assert rows[1]["markout_abs"] == Decimal("3")
    assert rows[2]["markout_abs"] == Decimal("4")


def test_summarize_markout_rows_groups_by_horizon() -> None:
    summary = summarize_markout_rows(
        [
            {"strategy_id": "s1", "horizon_s": 30, "markout_abs": Decimal("1"), "markout_bps": Decimal("100")},
            {"strategy_id": "s1", "horizon_s": 30, "markout_abs": Decimal("-0.5"), "markout_bps": Decimal("-50")},
        ],
    )

    assert summary[0]["horizon_s"] == 30
    assert summary[0]["count"] == 2
    assert summary[0]["avg_markout_abs"] == Decimal("0.25")
```

**Step 2: Run test to verify it fails**

Run: `python -m pytest -q tests/unit_tests/ops/test_makerv3_markouts.py`

Expected: FAIL because `ops/scripts/makerv3_markouts.py` does not exist yet.

**Step 3: Write minimal implementation**

Create `ops/scripts/makerv3_markouts.py` as a read-only script. Use existing helpers instead of inventing new Redis parsing:

- `flux.common.keys.FluxRedisKeys`
- `flux.api._payloads_common.extract_stream_rows`

Implement:

- `load_stream_rows(redis_client, strategy_id) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]`
- `compute_markout_rows(trade_rows, fv_rows, horizons_s)`
- `summarize_markout_rows(rows)`
- CLI flags: `--strategy`, `--profile`, `--horizons`, `--limit`, `--json`

Keep the benchmark contract simple:

```python
def signed_markout(side: str, fill_px: Decimal, benchmark_px: Decimal) -> Decimal:
    side_upper = side.upper()
    if side_upper == "BUY":
        return benchmark_px - fill_px
    if side_upper == "SELL":
        return fill_px - benchmark_px
    raise ValueError(f"Unsupported side {side!r}")
```

When no future FV row is available for a horizon, emit a row with `status="missing_future_fv"` and exclude it from the aggregate averages.

**Step 4: Run tests to verify it passes**

Run:

- `python -m pytest -q tests/unit_tests/ops/test_makerv3_markouts.py`
- `python ops/scripts/makerv3_markouts.py --help`

Expected: PASS. The script should print CLI usage without touching live state.

**Step 5: Commit**

```bash
git add ops/scripts/makerv3_markouts.py \
  tests/unit_tests/ops/test_makerv3_markouts.py
git commit -m "feat(markouts): add makerv3 redis markout report"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 2: Add live-forward markout persistence

**Files:**
- Create: `systems/flux/flux/persistence/markouts/__init__.py`
- Create: `systems/flux/flux/persistence/markouts/config.py`
- Create: `systems/flux/flux/persistence/markouts/schema.py`
- Create: `systems/flux/flux/persistence/markouts/sqlite.py`
- Create: `systems/flux/flux/persistence/markouts/actor.py`
- Modify: `systems/flux/flux/persistence/__init__.py`
- Create: `tests/unit_tests/persistence/test_markout_persistence_actor.py`

**Step 1: Write the failing test**

Create `tests/unit_tests/persistence/test_markout_persistence_actor.py` using the same actor-testing pattern as the existing fill/order/quote-cycle persistence tests. Cover:

- a BUY fill resolving positive markouts when later FV rises
- a SELL fill resolving positive markouts when later FV falls
- per-horizon rows for 30s/60s/120s
- `run_id` / `quote_cycle_id` enrichment from `flux.makerv3.order_intent`
- missing benchmark rows producing `resolution_status="expired"` after a bounded wait

```python
def test_markout_actor_persists_resolved_rows_for_each_horizon(tmp_path) -> None:
    actor, msgbus, db_path = _make_actor(tmp_path, horizons_s=(30, 60, 120))
    instrument = TestInstrumentProvider.btcusdt_binance()
    fill = _make_fill(instrument=instrument, side="BUY", last_px="100", ts_event=1_000_000_000)

    actor.start()
    msgbus.publish(topic=f"events.fills.{instrument.id}", msg=fill)
    msgbus.publish(topic="flux.makerv3.fv", msg='{"strategy_id":"MAKERV3-001","fv":"101","ts_ms":31_000}')
    msgbus.publish(topic="flux.makerv3.fv", msg='{"strategy_id":"MAKERV3-001","fv":"102","ts_ms":61_000}')
    msgbus.publish(topic="flux.makerv3.fv", msg='{"strategy_id":"MAKERV3-001","fv":"103","ts_ms":121_000}')
    actor.flush()
    actor.stop()

    rows = _fetch_rows(
        db_path,
        "SELECT horizon_s, benchmark_px, markout_abs, resolution_status FROM execution_markout ORDER BY horizon_s",
    )
    assert [(row["horizon_s"], row["benchmark_px"], row["markout_abs"], row["resolution_status"]) for row in rows] == [
        (30, "101", "1", "resolved"),
        (60, "102", "2", "resolved"),
        (120, "103", "3", "resolved"),
    ]
```

**Step 2: Run test to verify it fails**

Run: `python -m pytest -q tests/unit_tests/persistence/test_markout_persistence_actor.py`

Expected: FAIL because the new persistence module does not exist.

**Step 3: Write minimal implementation**

Create a new Flux persistence actor that subscribes to:

- `events.fills.*`
- `flux.makerv3.fv`
- optionally `flux.makerv3.order_intent` for `run_id`, `quote_cycle_id`, `reason_code`, and `level_index`

Use a small in-memory pending queue keyed by `(event_id, horizon_s)`. Resolve a markout when the actor has observed the first FV row with `ts_ms >= fill_ts_ms + horizon_s * 1000`.

Persist only final rows, not the raw benchmark stream. Use a schema like:

```sql
CREATE TABLE IF NOT EXISTS execution_markout (
  trader_id TEXT NOT NULL,
  event_id TEXT NOT NULL,
  trade_id TEXT NOT NULL,
  strategy_id TEXT NOT NULL,
  instrument_id TEXT NOT NULL,
  client_order_id TEXT NOT NULL,
  order_side TEXT NOT NULL,
  fill_px TEXT NOT NULL,
  fill_qty TEXT NOT NULL,
  benchmark_name TEXT NOT NULL,
  horizon_s INTEGER NOT NULL,
  target_ts_ms INTEGER NOT NULL,
  benchmark_ts_ms INTEGER,
  benchmark_px TEXT,
  markout_abs TEXT,
  markout_bps TEXT,
  resolution_status TEXT NOT NULL,
  run_id TEXT,
  quote_cycle_id TEXT,
  reason_code TEXT,
  level_index INTEGER,
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, event_id, horizon_s)
);
```

Use `benchmark_name="fv_market_mid"` for v0. Store `markout_bps = markout_abs / fill_px * 10000` when `fill_px > 0`.

Add a bounded expiry path such as `max_pending_ms = 180_000` so unresolved rows can close as `expired` instead of staying in memory forever.

**Step 4: Run tests to verify it passes**

Run:

- `python -m pytest -q tests/unit_tests/persistence/test_markout_persistence_actor.py`
- `python -m pytest -q tests/unit_tests/persistence/test_execution_fill_persistence_actor.py tests/unit_tests/persistence/test_order_action_persistence_actor.py tests/unit_tests/persistence/test_quote_cycle_persistence_actor.py`

Expected: PASS. The new actor tests should pass and adjacent persistence tests should remain green.

**Step 5: Commit**

```bash
git add systems/flux/flux/persistence/markouts \
  systems/flux/flux/persistence/__init__.py \
  tests/unit_tests/persistence/test_markout_persistence_actor.py
git commit -m "feat(markouts): persist live-forward makerv3 markouts"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 3: Wire markouts into TokenMM telemetry config

**Files:**
- Modify: `systems/flux/flux/runners/tokenmm/run_node.py`
- Modify: `deploy/tokenmm/tokenmm.live.toml`
- Modify: `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

**Step 1: Write the failing test**

Extend `tests/unit_tests/examples/strategies/test_tokenmm_run_node.py` so the runner wires the new actor when local telemetry persistence is enabled.

Cover:

- `_build_telemetry_actor_configs(...)` adds the markout actor when `markouts_db_path` is present
- `_prepare_telemetry_paths(...)` creates the parent directory for `markouts_db_path`
- deploy config pins the default horizons to `30`, `60`, and `120`

```python
def test_build_telemetry_actor_configs_includes_markouts_actor() -> None:
    actors = run_node._build_telemetry_actor_configs(
        {
            "telemetry_shipper": {
                "enable_local_persistence": True,
                "fills_db_path": "/tmp/fills.sqlite",
                "orders_db_path": "/tmp/orders.sqlite",
                "quote_cycles_db_path": "/tmp/quote_cycles.sqlite",
                "markouts_db_path": "/tmp/markouts.sqlite",
                "markout_horizons_s": [30, 60, 120],
            },
        },
    )

    assert any(
        actor.actor_path.endswith("markouts.actor:ExecutionMarkoutPersistenceActor")
        for actor in actors
    )
```

**Step 2: Run test to verify it fails**

Run: `python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k markout`

Expected: FAIL because no runner wiring or deploy config exists yet.

**Step 3: Write minimal implementation**

Update `systems/flux/flux/runners/tokenmm/run_node.py`:

- add `markouts_db_path` and `markout_horizons_s` support under `[telemetry_shipper]`
- append an `ImportableActorConfig` for the new actor
- create the parent directory for the DB path in `_prepare_telemetry_paths(...)`

Update `deploy/tokenmm/tokenmm.live.toml`:

```toml
[telemetry_shipper]
enable_local_persistence = true
fills_db_path = "/var/lib/nautilus/telemetry/tokenmm/fills.sqlite"
orders_db_path = "/var/lib/nautilus/telemetry/tokenmm/orders.sqlite"
quote_cycles_db_path = "/var/lib/nautilus/telemetry/tokenmm/quote_cycles.sqlite"
markouts_db_path = "/var/lib/nautilus/telemetry/tokenmm/markouts.sqlite"
markout_horizons_s = [30, 60, 120]
```

Do not add Postgres shipper support in v0. Keep the first cut local so preliminary numbers can land today without widening the warehouse surface.

**Step 4: Run tests to verify it passes**

Run:

- `python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py -k markout`
- `python -m pytest -q tests/unit_tests/examples/strategies/test_tokenmm_run_node.py`

Expected: PASS.

**Step 5: Commit**

```bash
git add systems/flux/flux/runners/tokenmm/run_node.py \
  deploy/tokenmm/tokenmm.live.toml \
  tests/unit_tests/examples/strategies/test_tokenmm_run_node.py
git commit -m "feat(tokenmm): wire makerv3 markout telemetry"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.

### Task 4: Document the operator workflow and future design boundaries

**Files:**
- Create: `docs/runbooks/makerv3-markouts.md`
- Modify: `systems/flux/docs/makerv3.md`
- Create: `tests/unit_tests/docs/test_makerv3_markouts_docs.py`

**Step 1: Write the failing test**

Create a small documentation contract test so the operator workflow stays explicit.

```python
from pathlib import Path


def test_markouts_docs_call_out_v0_scope_and_join_keys() -> None:
    runbook = Path("docs/runbooks/makerv3-markouts.md").read_text()
    strategy_doc = Path("systems/flux/docs/makerv3.md").read_text()

    assert "live-forward only" in runbook
    assert "flux:v1:trades:stream:{strategy_id}" in runbook
    assert "flux:v1:fv:stream:{strategy_id}" in runbook
    assert "execution_fill" in runbook
    assert "quote_cycle_id" in runbook
    assert "raw live market-data history is out of scope" in runbook
    assert "markouts" in strategy_doc.lower()
```

**Step 2: Run test to verify it fails**

Run: `python -m pytest -q tests/unit_tests/docs/test_makerv3_markouts_docs.py`

Expected: FAIL because the runbook and strategy-doc updates do not exist yet.

**Step 3: Write minimal implementation**

Create `docs/runbooks/makerv3-markouts.md` with:

- what we can compute today from existing Redis streams
- why that path is best-effort and retention-bound
- the live-forward persistence flow (`events.fills.*` + `flux.makerv3.fv` -> `execution_markout`)
- how to join markouts back to `execution_fill` and `order_action`
- explicit scope statement:
  - live-forward only
  - `fv_market_mid` only
  - TokenMM runner only
  - raw live market-data history is out of scope
  - core Nautilus `streaming` / Parquet catalog capture remains a future option, not part of this first PR
  - Postgres shipper / warehouse integration is out of scope for the first PR

Update `systems/flux/docs/makerv3.md` in the observability section so the canonical MakerV3 doc lists markouts as a derived telemetry surface and points readers at the runbook.

**Step 4: Run tests to verify it passes**

Run:

- `python -m pytest -q tests/unit_tests/docs/test_makerv3_markouts_docs.py`
- `python -m pytest -q tests/unit_tests/ops/test_makerv3_markouts.py tests/unit_tests/docs/test_makerv3_markouts_docs.py`

Expected: PASS.

**Step 5: Commit**

```bash
git add docs/runbooks/makerv3-markouts.md \
  systems/flux/docs/makerv3.md \
  tests/unit_tests/docs/test_makerv3_markouts_docs.py
git commit -m "docs(markouts): add makerv3 operator workflow"
```

**Progress Updates:** After finishing any step that changes task state, update the Progress Tracker before moving on.
