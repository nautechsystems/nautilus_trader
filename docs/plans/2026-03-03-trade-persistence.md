# Execution Fill Persistence (OrderFilled -> SQL + ArcticDB) Implementation Plan

**Goal:** Persist live execution fills (`OrderFilled` events) as immutable records to durable storage for audit, reporting, and downstream analytics; optionally mirror the dataset into ArcticDB.

**Architecture:** Subscribe to the internal MessageBus fill topic family `events.fills.*` (published by the Cython `ExecutionEngine`) and write each `OrderFilled` through one idempotent persistence path. Use a transactional DB (SQLite for local/dev; Postgres for production) as the source of record. Optionally mirror fills to a columnar event log using Nautilus’ existing Arrow/Feather streaming (`StreamingFeatherWriter`) and ingest that into ArcticDB for fast analytics.

**Tech Stack:** Nautilus MessageBus topics; `OrderFilled` model event; optional `Actor`-based subscriber; SQLite (`sqlite3`) for MVP; Postgres as follow-up; existing Arrow serializers (`nautilus_trader.serialization.arrow`) and streaming writer (`nautilus_trader.persistence.writer.StreamingFeatherWriter`); optional `arcticdb` Python package for analytics.

---

## Context (what exists today)

1. The canonical “trade fill” event in Nautilus is `OrderFilled`.
1. `ExecutionEngine` publishes fills on `events.fills.<instrument_id>` and order lifecycle events on `events.order.<strategy_id>`.
1. The kernel can already stream Arrow-serializable objects (including `OrderFilled`) to Feather files via `StreamingFeatherWriter` when `NautilusKernelConfig.streaming` is configured.
1. Cache/message-bus database backings are currently Redis-only in the kernel (`DatabaseConfig.type == "redis"`). There is no first-class “execution fills to SQL DB” sink.

Reference files (source of truth):
1. [`nautilus_trader/model/events/order.pyx`](/home/ubuntu/nautilus_trader/nautilus_trader/model/events/order.pyx) (`OrderFilled` fields/semantics).
1. [`nautilus_trader/execution/engine.pyx`](/home/ubuntu/nautilus_trader/nautilus_trader/execution/engine.pyx) (publishes `events.fills.<instrument_id>`).
1. [`nautilus_trader/persistence/writer.py`](/home/ubuntu/nautilus_trader/nautilus_trader/persistence/writer.py) (`StreamingFeatherWriter` and include-types filtering).
1. [`nautilus_trader/system/kernel.py`](/home/ubuntu/nautilus_trader/nautilus_trader/system/kernel.py) (wires streaming; Redis-only DB backings).
1. [`nautilus_trader/serialization/arrow/schema.py`](/home/ubuntu/nautilus_trader/nautilus_trader/serialization/arrow/schema.py) (Arrow schema for `OrderFilled`).

## Terminology

1. **Fill:** One execution match from the venue. In Nautilus this is `OrderFilled`.
1. **Trade tick:** Public market data “trade print” (Nautilus `TradeTick`). Not the same as a fill.
1. **Persistence path:** The single, authoritative write pipeline for fills (append-only, idempotent).

Note on naming:
1. This plan uses “trade persistence” to mean **execution fills** (`OrderFilled`), not public market `TradeTick`.
1. If your goal is persisting public trade ticks (`TradeTick`), prefer the existing DataCatalog/streaming path rather than building a new SQL sink.

## Success criteria

1. Every `OrderFilled` delivered to the persistence component is written idempotently using `PRIMARY KEY (trader_id, event_id)` (duplicates tolerated). Note: async buffering can lose fills between enqueue and flush on process crash; Option C (durable outbox + consumer) removes this window.
1. The write path does not block the trading hot path (buffering + bounded queue; configurable error policy).
1. The stored record is immutable (no updates, only append).
1. Common queries are fast: by time range, by strategy, by instrument, by account.
1. Optional analytics path exists: easy export to ArcticDB without coupling trading to ArcticDB runtime failures.

## Explicit non-goals (MVP)

1. Computing realized/unrealized PnL rollups (store raw fills first).
1. Multi-table transactional guarantees across orders, positions, accounts (fills-only MVP).
1. A full “reconciliation service” (MVP stores what Nautilus already accepted as a fill).
1. A UI/reporting layer (MVP focuses on storage + query primitives).

## Review decisions to lock early

1. Primary persistence mode for MVP: SQLite-only, or SQLite + Feather (Arrow IPC) streaming in parallel.
1. Production path: in-process Postgres writer, or out-of-process consumer (recommended) reading from a stream/outbox.
1. Canonical idempotency key: recommended `(trader_id, event_id)` (because `trade_id` is adapter/venue-derived and not reliably unique across adapters).
1. Failure policy: `FAIL_FAST` (stop the node) vs `LOG_AND_DROP` vs `BUFFER_UNTIL_FULL_THEN_FAIL`.
1. Whether to persist `info` payload (recommended yes, as JSON string).

---

## Event contract: `OrderFilled`

`OrderFilled` (defined in Cython) contains at minimum:
1. Identity: `trader_id`, `event_id`, `strategy_id`, `instrument_id`, `client_order_id`, `venue_order_id`, `account_id`, `trade_id` (venue match id), optional `position_id`.
1. Economics: `order_side`, `order_type`, `last_qty`, `last_px`, `currency`, `commission`, `liquidity_side`.
1. Timestamps: `ts_event` (venue/event time in ns), `ts_init` (object creation time in ns).
1. Metadata: `info` (freeform dict), `reconciliation` flag.

Arrow schema (already defined) stores most numeric-like fields as strings for precision safety:
1. `last_qty`, `last_px`, `commission` are strings.
1. `ts_event`, `ts_init` are `uint64`.
1. `info` is encoded JSON bytes.

---

## Capture point (what to subscribe to)

Recommendation: subscribe to `events.fills.*`, not `events.order.*`.

Topic wildcard note:
1. MessageBus subscriptions support `*` and `?` wildcards over the full topic string (see MessageBus `subscribe(...)` docs in [`nautilus_trader/common/component.pyx`](/home/ubuntu/nautilus_trader/nautilus_trader/common/component.pyx)).
1. `InstrumentId` string representations include dots (for example `BTCUSDT.BINANCE`), so a fill topic looks like `events.fills.BTCUSDT.BINANCE`.
1. A subscription of `events.fills.*` is intended to match `events.fills.<instrument_id>` including dots inside `<instrument_id>`.

Reasons:
1. `events.fills.<instrument_id>` carries only fills and is published once per accepted fill.
1. `events.order.<strategy_id>` carries the full order lifecycle (more volume) and also includes `OrderFilled`, increasing duplicate risk if you subscribe broadly.

This is aligned with how `ExecutionEngine` publishes fills:
1. It always publishes the fill to `events.fills.<instrument_id>` in `_handle_order_fill(...)`.
1. It publishes the same `OrderFilled` to `events.order.<strategy_id>` earlier in `_handle_event(...)`.

---

## Design options (including ArcticDB)

### Option A (fastest): Use existing streaming to Feather (Arrow IPC) as the fill event log

How it works:
1. Configure `NautilusKernelConfig.streaming`.
1. Set `StreamingConfig.include_types` to include `OrderFilled` (and optionally `OrderInitialized`).
1. The kernel subscribes to `*` and writes Arrow-serializable objects to `.../order_filled_*.feather`.

Pros:
1. No new DB dependencies.
1. Very low coupling to execution path (already part of kernel).
1. Great for analytics and easy to ingest into ArcticDB.

Cons:
1. No SQL constraints, joins, or transactional guarantees.
1. Point lookups and ad-hoc filtering are file and scan based unless you build extra indexing.
1. Append-only log semantics: ingestion to SQL/ArcticDB should dedupe on `(trader_id, event_id)`.

### Option B (recommended MVP “database”): Add a fills-only SQL sink (SQLite) with idempotent upsert

How it works:
1. A dedicated component subscribes to `events.fills.*`.
1. Each `OrderFilled` is appended to an `execution_fill` table.
1. A unique key enforces idempotency (`PRIMARY KEY (trader_id, event_id)`).

Pros:
1. Queryable immediately (`SELECT ... WHERE ts_event BETWEEN ...`).
1. Hard idempotency guarantees via unique constraints.
1. SQLite is zero-dependency and fine for local/dev and many single-node deployments.

Cons:
1. SQLite may bottleneck under very high fill rates.
1. You must be careful not to block the message handling hot path (needs buffering).

### Option C (recommended production): Outbox + consumer process that writes to Postgres and/or ArcticDB

How it works:
1. Trading node writes fills to a durable outbox (Redis Streams via MessageBus DB, or Feather streaming).
1. A separate consumer service reads and writes to Postgres as source of record and optionally to ArcticDB.
1. Redis Streams note: stream names do not support wildcard topics; consumers should read the configured stream(s) and filter messages where `topic.startswith("events.fills.")`.

Pros:
1. DB failures do not directly impact trading runtime.
1. Consumers can be scaled independently.
1. Clean separation of concerns (trading vs reporting/analytics).

Cons:
1. More moving parts to run and monitor.

### ArcticDB recommendation

Treat ArcticDB as a secondary analytics store, not the source of record:
1. ArcticDB is strong at append-heavy time-series DataFrame workloads and fast research queries.
1. ArcticDB is weaker at relational constraints and “single truth” transactional semantics across entities.

Recommended integration:
1. Persist fills to SQL (or at least to the Feather event log).
1. Periodically ingest the fills dataset into ArcticDB (batch job) to keep the trading runtime decoupled.

---

## Proposed MVP (spec)

### Persistence contract

1. Persist only `OrderFilled` from `events.fills.*`.
1. Treat each fill as immutable.
1. Use DB uniqueness for idempotency.
1. Store `info` as JSON string (`info_json`) to avoid losing venue-specific details.

### Database schema (SQLite MVP)

```sql
CREATE TABLE IF NOT EXISTS execution_fill (
  trader_id TEXT NOT NULL,
  event_id TEXT NOT NULL,

  strategy_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  instrument_id TEXT NOT NULL,

  trade_id TEXT NOT NULL,
  client_order_id TEXT NOT NULL,
  venue_order_id TEXT NOT NULL,
  position_id TEXT,

  order_side TEXT NOT NULL,
  order_type TEXT NOT NULL,
  last_qty TEXT NOT NULL,
  last_px TEXT NOT NULL,
  currency TEXT NOT NULL,
  commission TEXT NOT NULL,
  liquidity_side TEXT NOT NULL,

  ts_event INTEGER NOT NULL,
  ts_init INTEGER NOT NULL,

  reconciliation INTEGER NOT NULL DEFAULT 0,
  info_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, event_id)
);

CREATE INDEX IF NOT EXISTS execution_fill_ts_event_idx
  ON execution_fill (ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_strategy_ts_event_idx
  ON execution_fill (strategy_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_instrument_ts_event_idx
  ON execution_fill (instrument_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_account_ts_event_idx
  ON execution_fill (account_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_trade_id_idx
  ON execution_fill (trade_id);

-- Optional (point lookups / reconciliation)
CREATE INDEX IF NOT EXISTS execution_fill_client_order_id_idx
  ON execution_fill (client_order_id);

CREATE INDEX IF NOT EXISTS execution_fill_venue_order_id_idx
  ON execution_fill (venue_order_id);
```

Notes:
1. Numeric-like values are stored as strings to preserve exact precision (`last_qty`, `last_px`, `commission`), mirroring Arrow.
1. `reconciliation` is stored as `0/1` for SQLite portability.
1. The `PRIMARY KEY (trader_id, event_id)` provides idempotency for retries and replays.
1. `trade_id` is not assumed unique; it is stored and indexed for query/reconciliation.

### Writer behavior

1. Inserts use `INSERT ... ON CONFLICT(trader_id, event_id) DO NOTHING`.
1. Writes are buffered and flushed in batches (single transaction per batch).
1. A bounded queue prevents unbounded memory growth.
1. The MessageBus handler is strictly non-blocking: `put_nowait` into the queue only (no waits, no DB I/O). On `queue.Full`, apply the configured failure policy immediately.
1. Flush work is bounded so it cannot monopolize the actor thread:
1. Each flush processes at most `max_batch_size` rows (even if the queue is larger).
1. Optionally stop a flush after `flush_time_budget_ms` and resume on the next tick.

### Error handling (configurable)

1. `FAIL_FAST`: mark persistence failed on the first write error; with `propagate_errors_to_bus=True`
   this raises into the caller, otherwise persistence disables new ingress.
1. `LOG_AND_DROP`: log the error and drop the fill (not recommended for audit).
1. `BUFFER_UNTIL_FULL_THEN_FAIL`: retry writes with backoff; when queue is full, disable new ingress
   or raise immediately if `propagate_errors_to_bus=True`.

### SQLite operational notes

1. SQLite is single-writer; do not point multiple processes/nodes at the same `db_path` (use Option C + Postgres for multi-node).
1. Consider a `busy_timeout_ms` / connection `timeout` to reduce transient "database is locked" failures.
1. `journal_mode` and `synchronous` are durability/latency tradeoffs; document chosen defaults for the MVP.

### Observability (minimum)

1. Counters: enqueued, persisted, deduped, dropped, db_write_errors, info_encode_errors.
1. Gauges: queue_depth, persistence_lag_ns (`now - max(ts_event)` persisted).
1. Timings: flush_duration_ms, batch_size.

---

## How ArcticDB fits (concrete)

### Phase 1 (no new runtime deps): Use streaming + offline ingestion

1. Enable streaming and include `OrderFilled`:

```python
from nautilus_trader.config import StreamingConfig
from nautilus_trader.model.events import OrderFilled

streaming = StreamingConfig(
    catalog_path="/var/lib/nautilus/catalog",
    include_types=[OrderFilled],
)
```

2. A separate script/job reads the produced `order_filled_*.feather` files and writes to ArcticDB.
1. Ingest should treat Feather as an append-only event log and dedupe on `(trader_id, event_id)`.

### Phase 2 (optional): In-process ArcticDB mirror

1. Add an optional dependency group (for example `arcticdb` extra).
1. Implement an `ArcticDbFillSink` that appends rows to an ArcticDB library keyed by:
1. `instrument_id` (fast per-instrument queries), or
1. `strategy_id` (fast per-strategy PnL research), or
1. a hybrid (two libraries).

Recommendation: do Phase 1 first.

---

## Status tracker

Legend: `TODO` | `DOING` | `DONE` | `BLOCKED`

| Item | Status | Notes | PR/Link |
| --- | --- | --- | --- |
| Task 1: Lock MVP scope and decisions | TODO | Decide: SQLite-only vs SQLite+streaming; choose default failure policy | |
| Task 2: SQLite fills store | DONE | Schema + writer + idempotency complete; precision as TEXT; indexes added | Branch `plan-execution-fill-persistence`, commit `bdea10c1e` |
| Task 3: Fill persistence actor | DONE | Non-blocking enqueue; bounded flush; overflow + DB-down tests complete | Branch `plan-execution-fill-persistence`, commit `bdea10c1e` |
| Task 4: Usage docs | DONE | Node config example + streaming/ingestion notes added | Branch `plan-execution-fill-persistence`, commit `bdea10c1e` |

## Implementation plan (tasks)

### Task 1: Lock MVP scope and decisions

**Files:**
1. Modify: `docs/plans/2026-03-03-trade-persistence.md`

**Step 1: Choose the MVP persistence surface**

Decide:
1. SQLite-only MVP.
1. SQLite + streaming enabled in parallel (recommended).

Update this doc’s “Proposed MVP” section accordingly.

**Step 2: Choose failure policy**

Pick one policy and document it:
1. `BUFFER_UNTIL_FULL_THEN_FAIL` (recommended default).
1. `FAIL_FAST`.
1. `LOG_AND_DROP`.

---

### Task 2: Add a fills persistence package (SQLite store)

**Files:**
1. Create: `nautilus_trader/persistence/fills/__init__.py`
1. Create: `nautilus_trader/persistence/fills/schema.py`
1. Create: `nautilus_trader/persistence/fills/sqlite.py`
1. Test: `tests/unit_tests/persistence/test_execution_fill_sqlite.py`

**Step 1: Define the schema SQL**

In `nautilus_trader/persistence/fills/schema.py`:
```python
EXECUTION_FILL_SCHEMA_SQL = \"\"\"\
CREATE TABLE IF NOT EXISTS execution_fill (
  trader_id TEXT NOT NULL,
  event_id TEXT NOT NULL,

  strategy_id TEXT NOT NULL,
  account_id TEXT NOT NULL,
  instrument_id TEXT NOT NULL,
  trade_id TEXT NOT NULL,
  client_order_id TEXT NOT NULL,
  venue_order_id TEXT NOT NULL,
  position_id TEXT,
  order_side TEXT NOT NULL,
  order_type TEXT NOT NULL,
  last_qty TEXT NOT NULL,
  last_px TEXT NOT NULL,
  currency TEXT NOT NULL,
  commission TEXT NOT NULL,
  liquidity_side TEXT NOT NULL,
  ts_event INTEGER NOT NULL,
  ts_init INTEGER NOT NULL,
  reconciliation INTEGER NOT NULL DEFAULT 0,
  info_json TEXT NOT NULL DEFAULT '{}',
  created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now')),
  PRIMARY KEY (trader_id, event_id)
);

CREATE INDEX IF NOT EXISTS execution_fill_ts_event_idx
  ON execution_fill (ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_strategy_ts_event_idx
  ON execution_fill (strategy_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_instrument_ts_event_idx
  ON execution_fill (instrument_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_account_ts_event_idx
  ON execution_fill (account_id, ts_event);

CREATE INDEX IF NOT EXISTS execution_fill_trade_id_idx
  ON execution_fill (trade_id);

-- Optional (point lookups / reconciliation)
CREATE INDEX IF NOT EXISTS execution_fill_client_order_id_idx
  ON execution_fill (client_order_id);

CREATE INDEX IF NOT EXISTS execution_fill_venue_order_id_idx
  ON execution_fill (venue_order_id);
\"\"\"
```

**Step 2: Implement a minimal SQLite writer**

In `nautilus_trader/persistence/fills/sqlite.py`:
1. `connect(path: str) -> sqlite3.Connection` with `PRAGMA journal_mode=WAL;` and `PRAGMA synchronous=NORMAL;` as defaults.
1. `ensure_schema(conn)` executes `EXECUTION_FILL_SCHEMA_SQL`.
1. `fill_to_row(fill: OrderFilled) -> tuple[...]` converts to primitives using:
1. Identifiers: `.value` (for example `fill.instrument_id.value`).
1. Value types: `str(...)` for `last_qty`, `last_px`, `commission`.
1. `info_json`: `msgspec.json.encode(fill.info, enc_hook=msgspec_encoding_hook).decode("utf-8")` (on encoding failure, store `'{}'` and increment `info_encode_errors`).
1. `insert_fills(conn, rows: list[tuple])` runs `executemany` with `ON CONFLICT(trader_id, event_id) DO NOTHING` inside a single transaction per batch.

**Step 3: Write unit tests for idempotency**

In `tests/unit_tests/persistence/test_execution_fill_sqlite.py`:
1. Create a synthetic fill using stubs:

```python
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs

instrument = TestInstrumentProvider.btcusdt_binance()
order = TestExecStubs.make_accepted_order(instrument=instrument)
fill = TestEventStubs.order_filled(order=order, instrument=instrument, ts_event=123)
```

2. Insert the same `fill` twice and assert only one row exists.
1. Also assert the primary key columns match expected strings (`trader_id`, `event_id`).
1. Add a collision test: insert two fills with the same `trade_id` but different `event_id` and assert both rows exist.

**Step 4: Run tests**

Run: `pytest tests/unit_tests/persistence/test_execution_fill_sqlite.py -q`
Expected: PASS.

---

### Task 3: Add an ExecutionFillPersistence actor (subscribes to `events.fills.*`)

**Files:**
1. Create: `nautilus_trader/persistence/fills/config.py`
1. Create: `nautilus_trader/persistence/fills/actor.py`
1. Test: `tests/unit_tests/persistence/test_execution_fill_persistence_actor.py`

**Step 1: Define actor config**

In `nautilus_trader/persistence/fills/config.py`:
1. Define `ExecutionFillPersistenceActorConfig(ActorConfig)` with fields:
1. `db_path: str`
1. `topic: str = "events.fills.*"`
1. `flush_interval_ms: int = 250`
1. `max_batch_size: int = 1000`
1. `flush_time_budget_ms: int = 10` (optional, for tighter latency guarantees)
1. `max_queue_size: int = 10000`
1. `on_error: str = "buffer_until_full_then_fail"`

**Step 2: Implement the actor**

In `nautilus_trader/persistence/fills/actor.py`:
1. Subclass `nautilus_trader.common.actor.Actor`.
1. On `on_start`:
1. Connect to SQLite.
1. Ensure schema.
1. Subscribe to `config.topic` using a handler which buffers `OrderFilled` without blocking.
1. Recommendation for MVP: use a dedicated writer thread which owns the SQLite connection.
1. On `on_stop`:
1. Flush remaining items.
1. Close DB connection.

Keep the handler non-blocking:
1. The msgbus handler only enqueues.
1. Enqueue must be non-blocking (`put_nowait` only). On `queue.Full`, apply `on_error` immediately.
1. The writer thread does DB I/O + batching.
1. Do not pass `OrderFilled` objects across threads; convert to primitive row tuples before enqueueing.
1. Bound flush work: each flush writes at most `max_batch_size` rows (and optionally stops after `flush_time_budget_ms`).

**Step 3: Unit test basic end-to-end persistence**

In `tests/unit_tests/persistence/test_execution_fill_persistence_actor.py`:
1. Use a temp SQLite path.
1. Instantiate the actor with the config.
1. Add a subscription test proving `events.fills.*` matches dotted instrument IDs (for example publish to `events.fills.BTCUSDT.BINANCE` and assert the handler receives it).
1. Call `actor.on_order_filled(fill)` directly (unit test) and force a flush method on the actor/sink.
1. Stop (ensures final flush + close).
1. Assert the DB contains the row.
1. Add overflow tests using a tiny `max_queue_size` to validate the configured `on_error` policy.
1. Add DB-down tests by injecting a writer/repo stub which raises on insert:
1. `FAIL_FAST`: first write failure marks persistence failed; raises only when
   `propagate_errors_to_bus=True`.
1. `BUFFER_UNTIL_FULL_THEN_FAIL`: fills stay queued with retry backoff; once queue is full the actor
   disables new ingress or raises immediately if propagation is enabled.
1. `LOG_AND_DROP`: dropped counter increments; DB row count matches non-dropped events.

**Step 4: Run tests**

Run: `pytest tests/unit_tests/persistence/test_execution_fill_persistence_actor.py -q`
Expected: PASS.

---

### Task 4: Document usage (node config)

**Files:**
1. Modify: `docs/concepts/execution.md`
1. Modify: `docs/concepts/message_bus.md` (optional, for outbox pattern)
1. Modify: `docs/getting_started/` (optional)

**Step 1: Add an example enabling fill persistence actor**

Show how to attach the actor using `ImportableActorConfig`:
```python
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    actors=[
        ImportableActorConfig(
            actor_path="nautilus_trader.persistence.fills.actor:ExecutionFillPersistenceActor",
            config_path="nautilus_trader.persistence.fills.config:ExecutionFillPersistenceActorConfig",
            config={
                "component_id": "FILL-DB",
                "db_path": "/var/lib/nautilus/fills.sqlite",
                "topic": "events.fills.*",
            },
        ),
    ],
)
```

**Step 2: Add an example enabling streaming for ArcticDB ingestion**

Include a snippet configuring `streaming=StreamingConfig(..., include_types=[OrderFilled])`.

---

## Follow-ups (post-MVP)

1. Add a Postgres backend for the fill store:
1. Either via an optional Python dependency (`psycopg`) or via a new Rust/PyO3 binding dedicated to fills.
1. Use the same idempotency key and schema shape; store `info` in `JSONB`.
1. Add a consumer that ingests from Feather stream output into Postgres for production outbox mode.
1. Add an ArcticDB batch ingestor script under `scripts/` that reads `order_filled_*.feather` and appends to an ArcticDB library.
1. Add schema versioning/migrations (SQLite + Postgres) and document the supported evolution strategy.
1. Consider wiring `DatabaseConfig.type == "postgres"` for cache backing in the kernel using the existing `PostgresCacheDatabase` binding and SQL schema under [`schema/sql/`](/home/ubuntu/nautilus_trader/schema/sql).
