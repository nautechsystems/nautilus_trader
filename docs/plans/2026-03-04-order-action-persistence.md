# Order Action Persistence (places/cancels -> SQL) High Level Spec

**Goal:** Persist *order actions* (place + cancel) as immutable, queryable records in a SQL database for audit/debugging, complementing existing execution fill persistence.

**Architecture:** Subscribe to order event topics (`events.order.*`), normalize selected `OrderEvent` types into a canonical `order_action` record, and write via one idempotent persistence path (SQLite MVP; Postgres later). Keep the schema flexible by storing optional/forward-compatible fields in JSON, and only promoting stable, high-value columns.

**Tech stack:** Nautilus MessageBus, `OrderEvent` model events, `Actor` subscriber + buffered writer, SQLite (`sqlite3`) for MVP, optional Postgres follow-up.

---

## Scope

### In scope (MVP)

Persist the following order event types (all published under `events.order.<strategy_id>`):

- `OrderSubmitted` (place intent accepted by engine)
- `OrderPendingCancel` (cancel request pending)
- `OrderCanceled` (cancel completed)
- `OrderCancelRejected` (cancel failed)

Optional for MVP (recommended):

- `OrderRejected` (place failed) for a complete place/cancel audit trail

### Out of scope (for now)

- Full order lifecycle/event-sourcing (every `OrderEvent` type).
- Cross-entity ACID invariants (this is append-only event persistence).
- Multi-node/multi-process SQLite writes (use Postgres/outbox for production scaling).

---

## Data model: canonical `order_action` record

Treat each persisted record as an immutable event.

### Idempotency key

- Canonical idempotency key: `(trader_id, event_id)` (mirrors fill persistence).
- `client_order_id` is a query key, not an idempotency key (many events per order).

### Action semantics

- `event_type` is the concrete `OrderEvent` class name (e.g., `OrderSubmitted`).
- `action_type` is a coarse category derived from `event_type`:
- `PLACE` for submit-related events.
- `CANCEL` for cancel-related events.

---

## Schema (SQLite MVP)

### Design principle: flexible columns + JSON payload

We should not block on finalizing every field now. Persist stable identity/time fields as columns for indexing, and store the rest in `payload_json` / `info_json`.

Key note about "reason":

- Some order events already have a `reason` attribute meaning *rejection/denial reason* (engine/venue-provided).
- The new requirement is a *strategy-supplied reason for the action* (why we placed/canceled).
- To avoid conflation, persist these separately:
- `rejection_reason` (from event fields when present, e.g. `OrderCancelRejected.reason`)
- `action_reason` (strategy-supplied, likely carried via tags/info, see issue below)

### Proposed table: `order_action`

Columns (minimum viable, indexable):

- `trader_id TEXT NOT NULL`
- `event_id TEXT NOT NULL`
- `strategy_id TEXT NOT NULL`
- `account_id TEXT NOT NULL`
- `instrument_id TEXT NOT NULL`
- `client_order_id TEXT NOT NULL`
- `venue_order_id TEXT` (nullable; may be missing early)
- `position_id TEXT` (nullable)
- `action_type TEXT NOT NULL` (`PLACE` or `CANCEL`)
- `event_type TEXT NOT NULL` (e.g. `OrderSubmitted`)
- `action_reason TEXT` (nullable; strategy-supplied, see framework issue)
- `rejection_reason TEXT` (nullable; for reject/deny events)
- `ts_event INTEGER NOT NULL`
- `ts_init INTEGER NOT NULL`
- `reconciliation INTEGER NOT NULL DEFAULT 0`
- `payload_json TEXT NOT NULL DEFAULT '{}'` (full `OrderEvent.to_dict()` or subset)
- `info_json TEXT NOT NULL DEFAULT '{}'` (event.info if available)
- `created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ','now'))`

Primary key:

- `PRIMARY KEY (trader_id, event_id)`

Indexes (start minimal, expand based on real queries):

- `(ts_event)`
- `(strategy_id, ts_event)`
- `(instrument_id, ts_event)`
- `(client_order_id, ts_event)`
- `(action_type, ts_event)`

---

## Persistence pipeline (MVP)

### Ingestion

- Subscribe to `events.order.*`.
- Filter to selected event types (place/cancel set above).
- Normalize to a primitive row (no cross-thread Cython objects).
- Enqueue using `put_nowait` (no waits, no DB I/O on handler path).

### Write path

- Batch inserts in a single transaction per batch.
- Idempotent insert: `ON CONFLICT(trader_id, event_id) DO NOTHING`.
- Support bounded flush behavior (batch size cap + optional time budget).
- Provide threaded `flush()` drain-barrier semantics for deterministic unit tests and shutdown.

### Failure policy

Reuse the same error policy shape as fills:

- `buffer_until_full_then_fail` (default): retain for retry until queue full, then raise.
- `log_and_drop`: drop and log (best-effort).
- `fail_fast`: set writer error and raise.

---

## Interaction with fill persistence

SQLite is single-writer. If running both fills and order-actions persistence against the same DB file:

- Expect lock contention under load (two writer threads, two connections).
- Mitigations for MVP:
- Prefer separate DB files (`fills.sqlite`, `orders.sqlite`).
- Or unify into a single SQLite writer/actor if you require a single DB file.

Postgres follow-up avoids this limitation.

---

## Queries (examples)

- "Show all cancels for strategy X over last N minutes"
- "Show all actions for a client_order_id"
- "Join order actions to fills for a complete audit trail" (by `client_order_id`)

---

## Open questions (to lock before implementation)

- Persist only "action" events (submitted/pending-cancel/canceled/cancel-rejected), or expand to full order lifecycle?
- Where should *strategy action reason* live structurally?
- In `Order.info` as `{"action_reason": "..."}`?
- In explicit command/event fields?
- In tags on the order object?
- Should `action_reason` be indexed (likely yes once stable)?

---

## Status tracker

Legend: `TODO` | `DOING` | `DONE` | `BLOCKED`

| Item | Status | Notes |
| --- | --- | --- |
| Task 1: Lock scope and event types | TODO | Confirm which `OrderEvent` types are in MVP, and define `action_type` mapping |
| Task 2: SQLite schema + writer for `order_action` | TODO | Create schema + insert + idempotency |
| Task 3: `OrderActionPersistenceActor` | TODO | Subscribe to `events.order.*`, filter, enqueue-only hot path, bounded flush |
| Task 4: Docs update | TODO | Document usage + join with fills for audit |
| Task 5: Strategy action-reason framework | TODO | Track via GitHub issue (see “Reason tagging” issue) |
