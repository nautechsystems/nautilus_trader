# Order Action Persistence (Order* -> SQL) Implementation Plan

**Goal:** Persist engine-observed order lifecycle events related to placement and cancellation as immutable, queryable SQL records for audit/debugging (MM/HFT “flight recorder”), complementing execution fill persistence.

**Architecture:** Subscribe to internal MessageBus order topics (currently `events.order.<strategy_id>`), filter to a chosen `OrderEvent` set, normalize into a canonical `order_action` row, and write through one idempotent, buffered persistence path (SQLite MVP; Postgres/outbox later). Capture strategy “intent” metadata (`action_reason`, optional `action_id`, optional `ts_decision_ns`) via order tags (`OrderInitialized.tags`) and/or a parallel intent event stream for cancels.

**Tech stack:** Nautilus MessageBus + `Actor`, `OrderEvent` model events, SQLite (`sqlite3`) for MVP, optional Postgres follow-up, optional out-of-process consumer via external streams (Redis caveats documented).

---

## Context (what exists today)

1. `ExecutionEngine` publishes fills on `events.fills.<instrument_id>` and order lifecycle events on `events.order.<strategy_id>`.
1. Fill persistence exists and persists `OrderFilled` from `events.fills.*` (see [2026-03-03-trade-persistence.md](/home/ubuntu/nautilus_trader/docs/plans/2026-03-03-trade-persistence.md)).
1. For audit/debug, order lifecycle events are the engine-observed source of truth. Do not persist ad-hoc strategy logs as the canonical record.

Reference files (source of truth):
1. [`nautilus_trader/model/events/order.pyx`](/home/ubuntu/nautilus_trader/nautilus_trader/model/events/order.pyx) (order event fields/semantics).
1. [`docs/concepts/strategies.md`](/home/ubuntu/nautilus_trader/docs/concepts/strategies.md) (strategy order event handlers).
1. [`docs/concepts/message_bus.md`](/home/ubuntu/nautilus_trader/docs/concepts/message_bus.md) (wildcards vs Redis streams).
1. [`docs/concepts/data.md`](/home/ubuntu/nautilus_trader/docs/concepts/data.md) (`ts_event` / `ts_init` semantics).

## Terminology

1. **Order action:** a strategy decision to `PLACE` or `CANCEL` an order (intent), plus the resulting engine/venue lifecycle events (result).
1. **Strategy action reason:** why the strategy decided (example: `quote:reprice`). This is not the same as venue/engine rejection reasons.
1. **Rejection reason:** engine/venue-provided `reason` fields on reject/deny events (example: cancel rejected).
1. **Timestamps:**
1. `ts_event` and `ts_init` are UNIX epoch nanoseconds (see `docs/concepts/data.md`).
1. `ts_ingest` is the persistence component’s ingestion timestamp (UNIX ns) taken when the handler receives/enqueues the event.

## Success criteria

1. Append-only persistence of the selected order events with idempotency key `PRIMARY KEY (trader_id, event_id)`.
1. Hot path is enqueue-only: no waiting and no DB I/O in MessageBus handlers.
1. Query contract supports intent→result correlation for both PLACE and CANCEL.
1. Strategy “reason” is captured where available, without blocking persistence on the strategy framework being complete.
1. Compatible with later out-of-process consumption (Redis stream wildcard caveat documented).

## Explicit non-goals (MVP)

1. Full event-sourcing of every `OrderEvent`.
1. Cross-table ACID invariants with fills/positions.
1. Multi-process writers to SQLite (use Postgres/outbox for multi-node scaling).

## Review decisions locked (Task 1)

1. Event set (MVP): include `OrderInitialized`, `OrderSubmitted`, `OrderAccepted`, `OrderRejected`, `OrderPendingCancel`, `OrderCanceled`, `OrderCancelRejected`.
1. Strategy metadata capture (MVP): tags-based extraction from `OrderInitialized` for PLACE path; keep cancel `OrderActionIntent` stream as follow-up (Task 5), not required for Tasks 2-4.
1. SQLite layout (MVP default): separate DB files for fills vs orders (`fills.sqlite`, `orders.sqlite`).
1. Index set (MVP): baseline indexes only; optional indexes (`instrument_id`, `venue_order_id`, `action_id`) deferred until query-driven follow-up.

---

## Capture point (what to subscribe to)

Recommendation: subscribe to internal order event topics (currently `events.order.<strategy_id>`), not strategy callbacks/logs.

Topic notes:
1. Internal MessageBus subscriptions support `*` / `?` wildcards over the full topic string (see `Component.subscribe(...)` docs in `nautilus_trader/common/component.pyx`).
1. `events.order.<strategy_id>` carries the full order lifecycle (including fills), so this component must filter to the chosen `OrderEvent` types to control write load.
1. If later moved out-of-process using external publishing to Redis Streams:
1. Redis streams do not support wildcard topics when listening.
1. Prefer `stream_per_topic=False` and have the consumer filter `topic.startswith("events.order.")` (see `docs/concepts/message_bus.md`).

---

## Event contract (selected `OrderEvent` types)

### Locked MVP event set (intent -> result)

Persist the following (published on internal order event topics, currently `events.order.<strategy_id>`):

1. PLACE:
1. `OrderInitialized` (seed event; contains order parameters and `tags`)
1. `OrderSubmitted` (submitted by the system to the trading venue)
1. `OrderAccepted` (accepted/acknowledged by the trading venue)
1. `OrderRejected` (place rejected; includes `reason`)

1. CANCEL:
1. `OrderPendingCancel` (cancel requested/in-flight)
1. `OrderCanceled` (cancel completed)
1. `OrderCancelRejected` (cancel rejected; includes `reason`)

Notes:
1. `OrderInitialized` is the only order lifecycle event that reliably carries strategy-supplied `tags` today; most other order events do not carry tags/info.
1. Cancel lifecycle events do not have a native strategy metadata slot; see Strategy intent metadata section for how to persist cancel “reason”.
1. Scope lock: `OrderInitialized`, `OrderAccepted`, and `OrderRejected` are included in MVP (not optional).

---

## Data model: canonical `order_action` row

Treat each persisted record as an immutable event.

### Idempotency key

1. Canonical idempotency key: `(trader_id, event_id)` (mirrors fill persistence).
1. `client_order_id` is a query key (many events per order), not an idempotency key.

### Correlation contract (intent -> result)

1. Group order lifecycle events by `(trader_id, client_order_id)`.
1. PLACE correlation:
1. request: `OrderSubmitted`
1. result: `OrderAccepted` or `OrderRejected`
1. CANCEL correlation:
1. request: `OrderPendingCancel`
1. result: `OrderCanceled` or `OrderCancelRejected`

### Action type/state mapping

Persist `action_type` and `action_state` as stable, queryable enums derived from `event_type`.

Locked enum domains:

1. `action_type` in `{PLACE, CANCEL}`
1. `action_state` in `{INITIALIZED, SUBMITTED, ACCEPTED, REQUESTED, COMPLETED, REJECTED}`

Locked event mapping:

| event_type | action_type | action_state |
| --- | --- | --- |
| `OrderInitialized` | `PLACE` | `INITIALIZED` |
| `OrderSubmitted` | `PLACE` | `SUBMITTED` |
| `OrderAccepted` | `PLACE` | `ACCEPTED` |
| `OrderRejected` | `PLACE` | `REJECTED` |
| `OrderPendingCancel` | `CANCEL` | `REQUESTED` |
| `OrderCanceled` | `CANCEL` | `COMPLETED` |
| `OrderCancelRejected` | `CANCEL` | `REJECTED` |

---

## Schema (SQLite MVP)

### Design principle

1. Promote stable identity/time fields and common query filters to columns.
1. Store full event payload as JSON for forward compatibility.
1. Store numeric-like values as TEXT (decimal-as-string) for precision safety.

### Key note about “reason”

1. Some order events have a `reason` attribute meaning *rejection/denial reason* (engine/venue-provided).
1. The new requirement is a *strategy-supplied reason for the action* (why we placed/canceled).
1. Persist these separately:
1. `rejection_reason` (from event fields when present, for example `OrderCancelRejected.reason`).
1. `action_reason` (strategy-supplied; best-effort).

### Proposed table: `order_action`

Columns (baseline):

- `trader_id TEXT NOT NULL`
- `event_id TEXT NOT NULL`
- `strategy_id TEXT NOT NULL`
- `instrument_id TEXT NOT NULL`
- `client_order_id TEXT NOT NULL`
- `account_id TEXT` (nullable)
- `venue_order_id TEXT` (nullable)
- `position_id TEXT` (nullable)

- `action_type TEXT NOT NULL` (`PLACE` or `CANCEL`)
- `action_state TEXT NOT NULL` (`INITIALIZED`, `SUBMITTED`, `ACCEPTED`, `REQUESTED`, `COMPLETED`, `REJECTED`; locked enum set, aligned with mapping table above)
- `event_type TEXT NOT NULL` (e.g., `OrderSubmitted`)

Strategy intent metadata (best-effort, nullable):

- `action_id TEXT` (nullable)
- `action_reason TEXT` (nullable)
- `ts_decision_ns INTEGER` (nullable; UNIX ns)
- `signal_snapshot_json TEXT NOT NULL DEFAULT 'null'` (optional)

Order parameter columns (nullable; primarily from `OrderInitialized`):

- `order_side TEXT` (nullable)
- `order_type TEXT` (nullable)
- `time_in_force TEXT` (nullable)
- `post_only INTEGER` (nullable; `0/1`)
- `reduce_only INTEGER` (nullable; `0/1`)
- `order_qty TEXT` (nullable; decimal-as-string)
- `order_px TEXT` (nullable; decimal-as-string)

Rejection metadata:

- `rejection_reason TEXT` (nullable)

Timestamps:

- `ts_event INTEGER NOT NULL` (UNIX ns)
- `ts_init INTEGER NOT NULL` (UNIX ns)
- `ts_ingest INTEGER NOT NULL` (UNIX ns at enqueue/ingest)
- `reconciliation INTEGER NOT NULL DEFAULT 0`

Payload:

- `payload_json TEXT NOT NULL DEFAULT '{}'` (full `OrderEvent.to_dict()`)
- `created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))`

Primary key:

- `PRIMARY KEY (trader_id, event_id)`

Indexes (start minimal, expand based on real queries):

- `(strategy_id, ts_event)` (primary read path: per-strategy windows)
- `(client_order_id, ts_event)` (audit trail + join to fills)

Optional indexes:

- `(instrument_id, ts_event)`
- `(venue_order_id)`
- `(action_id)`

Schema notes:

1. `created_at` is a DB wall-clock convenience and must not be used for sequencing or latency analysis.
1. `ts_ingest` is the canonical ingestion timestamp for persistence ordering/latency attribution (not `created_at`).

---

## Strategy intent metadata (“reason”, action_id, decision time)

### Tagging convention (PLACE path, works today)

For PLACE actions, strategies should tag orders using namespaced tags on the `Order` (carried via `OrderInitialized.tags`).

Recommended tag keys:

1. `nautilus.intent.action_id=<uuid>`
1. `nautilus.intent.reason=<reason_code>`
1. `nautilus.intent.ts_decision_ns=<unix_ns>`
1. Optional: `nautilus.intent.signal=<small_json_or_hash>`

Persistence behavior:

1. Parse `OrderInitialized.tags` and lift `action_id`, `action_reason`, and `ts_decision_ns` into columns when present.
1. If missing/unparseable, store NULL and keep the full `payload_json` for later backfill once strategy conventions are enforced.

Reason format guidance (avoid free-text explosion):

1. Use a namespaced, enum-ish string (examples):
1. `risk:inventory_limit`
1. `quote:reprice`
1. `quote:stale_ref`
1. `ops:shutdown`

Tracking:

1. Strategy-side reason tagging framework is tracked in GitHub issue `clickconfirm/nautilus-trader#7`.

### Cancel intent follow-up (CANCEL path, non-MVP)

MVP scope lock for Tasks 2-4: do **not** implement cancel-intent stream persistence in this phase.
Because cancel lifecycle events do not carry strategy tags/info today, persist strategy cancel intent separately as Task 5 follow-up work.

Follow-up approach (Task 5):

1. Emit a custom internal event `OrderActionIntent` on `events.order_intent.<strategy_id>`.
1. Persist it append-only with the same idempotency key (`(trader_id, event_id)`), either:
1. as a separate table `order_action_intent`, or
1. as “intent rows” in the same `order_action` table (distinguished by `event_type`) if you want a single-table query surface.

Correlation:

1. Join cancel intent to cancel lifecycle events by `(trader_id, client_order_id)` and (optionally) `action_id`.

---

## Persistence pipeline (MVP)

### Ingestion

1. Subscribe to `events.order.*` (or per-strategy topics if required by the MessageBus backing).
1. Filter to the selected `OrderEvent` types.
1. Normalize to a primitive row (no cross-thread Cython objects).
1. Set `ts_ingest = now_ns()` at handler receive time.
1. Enqueue using `put_nowait` (no waits, no DB I/O on handler path).

### Write path

1. Batch inserts in a single transaction per batch.
1. Idempotent insert: `INSERT ... ON CONFLICT(trader_id, event_id) DO NOTHING`.
1. Flush boundedness:
1. process at most `max_batch_size` rows per flush tick
1. optional: stop after `flush_time_budget_ms` and resume on the next tick
1. Threaded mode:
1. writer thread owns the SQLite connection
1. `flush()` acts as a durability barrier (waits for queued tasks to be durably handled, subject to timeout)

### Error handling (configurable)

Reuse the fill persistence error policy shape:

1. `buffer_until_full_then_fail` (default): retain for retry with backoff until queue full, then
   disable new ingress or raise immediately if `propagate_errors_to_bus=True`.
1. `log_and_drop`: drop and log (best-effort).
1. `fail_fast`: set persistence failed on the first write error; raise only when propagation is
   enabled.

### Config surface (proposed)

1. `db_path`
1. `topic` (default `events.order.*`)
1. `max_queue_size`
1. `flush_interval_ms`
1. `max_batch_size`
1. `flush_time_budget_ms` (optional)
1. `flush_timeout_ms` (used by `flush()` barrier waits)
1. `stop_timeout_ms`
1. `strict_stop`
1. `on_error`

---

## Interaction with fill persistence

SQLite is single-writer per DB file.

Recommendations:

1. Default to separate DB files (`fills.sqlite`, `orders.sqlite`) to avoid lock contention and to isolate backlogs/failures.
1. If a single DB file is required, use a unified single-writer sink that writes both tables from one connection. Avoid “one DB file, two writers”.

Postgres follow-up avoids these limitations.

---

## Queries (examples)

1. “Show all CANCEL actions for strategy X where `action_state='REJECTED'` in last 10m.”
1. “For each PLACE, compute submit→accept latency: `OrderAccepted.ts_event - OrderSubmitted.ts_event` (and compare to `ts_ingest` deltas).”
1. “Join order lifecycle to fills by `client_order_id` for a full audit trail.”

---

## Open questions status (locked for Tasks 2-4 scope)

1. `OrderInitialized` in MVP: **Yes (locked)**.
1. `OrderActionIntent` in MVP: **No (follow-up, Task 5)**.
1. `action_id` strategy requirement in MVP: **best-effort (nullable, not enforced)**.
1. Optional indexes (`instrument_id`, `venue_order_id`, `action_id`): **defer from MVP; add only when query evidence requires them**.
1. Blocking check for Tasks 2-4: **No remaining blockers**.

---

## Status tracker

Legend: `TODO` | `DOING` | `DONE` | `BLOCKED`

| Item | Status | Notes | Link |
| --- | --- | --- | --- |
| Task 1: Lock MVP scope + mapping | DONE | Locked MVP event set (includes `OrderInitialized`/`OrderAccepted`/`OrderRejected`), locked enum mapping, moved `OrderActionIntent` to follow-up, set `action_id` best-effort, deferred optional indexes. Refs: `98fd07677`, `096697dad`, `771c4642d`, `05d7da641`, `9d89f0b98` | `98fd07677`, `096697dad`, `771c4642d`, `05d7da641`, `9d89f0b98` |
| Task 2: SQLite store for `order_action` | DONE | Added orders schema + `insert_many` idempotent writer; follow-up clarified JSON `null` literal intent, replaced brittle tuple alias with named row type, added empty/mixed batch tests, and aligned test fixtures to use `OrderActionRow` + shared signal default literal. Refs: `c9ed7bd1e`, `20bb92f6e`, `06a0b9c81` | `c9ed7bd1e`, `20bb92f6e`, `06a0b9c81` |
| Task 3: `OrderActionPersistenceActor` | DONE | Added orders actor/config with selected event filtering, enqueue-only hot path, bounded batch flush, threaded durability barrier semantics, strict/non-strict timeout handling, deterministic cleanup/finalization, and backlog drain regression coverage. Refs: base implementation `241adae5f`; follow-up hardening commits through `a541d6403`. | base `241adae5f`; follow-up hardening through `a541d6403` |
| Task 4: Docs update | DONE | Added execution docs for `OrderActionPersistenceActor` usage, topic patterns, Redis wildcard caveat, and example SQL queries including `action_state` and fills joins. Refs: `054f1e652`, `6a6aeeef0`, `3689f3130`, `204bff39d`, `fb2bd7c7e`. | `054f1e652`, `6a6aeeef0`, `3689f3130`, `204bff39d`, `fb2bd7c7e` |
| Task 5: Strategy reason/action_id framework | TODO | Strategy tagging + cancel intent channel | `clickconfirm/nautilus-trader#7` |

## Implementation plan (tasks)

### Task 1: Lock MVP scope + mapping

Status: decisions are locked and this task is complete; this section is retained as the implementation record.

**Files:**
1. Modify: `docs/plans/2026-03-04-order-action-persistence.md`

**Step 1: Confirm event set**

Locked decision (completed): MVP includes:
1. `OrderInitialized`
1. `OrderAccepted`
1. `OrderRejected`

Updated in the “Locked MVP event set” section.

**Step 2: Finalize `action_state` mapping**

Completed: mapping table and enum values are locked and aligned with schema documentation.

---

### Task 2: Add an order actions persistence package (SQLite store)

**Files:**
1. Create: `nautilus_trader/persistence/orders/__init__.py`
1. Create: `nautilus_trader/persistence/orders/schema.py`
1. Create: `nautilus_trader/persistence/orders/sqlite.py`
1. Test: `tests/unit_tests/persistence/test_order_action_sqlite.py`

**Step 1: Define the schema SQL**

In `nautilus_trader/persistence/orders/schema.py`, define the DDL for `order_action` and the baseline indexes.

**Step 2: Implement idempotent inserts**

In `nautilus_trader/persistence/orders/sqlite.py`, implement:
1. schema creation
1. `insert_many(rows)` using `ON CONFLICT(trader_id, event_id) DO NOTHING`

**Step 3: Add unit tests**

In `tests/unit_tests/persistence/test_order_action_sqlite.py`:
1. insert the same `event_id` twice and assert only one row exists
1. insert two events with same `client_order_id` and assert both persist

---

### Task 3: Add an order action persistence actor

**Files:**
1. Create: `nautilus_trader/persistence/orders/config.py`
1. Create: `nautilus_trader/persistence/orders/actor.py`
1. Test: `tests/unit_tests/persistence/test_order_action_persistence_actor.py`

**Step 1: Define config**

Mirror the fills persistence config surface (queue, batching, flush barrier, stop behavior).

**Step 2: Implement actor**

Implement an `Actor` that:
1. subscribes to order event topics
1. filters to the selected `OrderEvent` types
1. normalizes to primitive rows and sets `ts_ingest`
1. enqueues with `put_nowait`
1. flushes in bounded batches on a timer or writer thread

**Step 3: Add unit tests**

Add tests for:
1. filtering to the configured event types
1. threaded `flush()` durability barrier semantics
1. shutdown behavior under backlog (strict vs non-strict)

---

### Task 4: Docs update

**Files:**
1. Modify: `docs/concepts/execution.md` (or appropriate execution/persistence doc)

**Step 1: Add usage snippet**

Document:
1. how to configure and add the actor to a node
1. topic patterns and Redis stream wildcard caveat
1. example queries (including `action_state`)
