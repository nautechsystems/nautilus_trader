# Flux MakerV3 Single-Leg Productionization Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Promote PR #5's MakerV3 single-leg quoting + bridge + API from POC examples into a production-grade, config-driven `flux` integration inside Nautilus Trader.

**Architecture:** Split into three layers (Strategy, Bridge, API) with a versioned Redis schema (`flux:v1:*`) and explicit config objects (no scattered `os.getenv`). Keep hot-path strategy callbacks free of blocking I/O and ensure deterministic order lifecycle reconciliation.

**Tech Stack:** Nautilus Trader (engine/strategy), Python, Redis (streams/hashes/lists + pubsub), Flask (existing API), Nautilus MessageBus.

---

## Scope / non-goals

**In scope**

1. Engine/strategy code and Nautilus integration.
2. Flux bridge + Flux API hardening (no UI/fluxboard work).
3. Redis keyspace/schema decisions + docs.
4. Config standardization and removal of hardcoding/POC naming.

**Out of scope (for this plan)**

1. Fluxboard/UI work.
2. Adding new venues/features beyond what is already present; focus on hardening/modularizing existing behavior.

## Production bar (acceptance criteria)

1. No `poc` naming in shipped module paths, strategy IDs, topic prefixes, env vars, docs, or defaults.
2. No `chainsaw` naming in shipped code/docs.
3. No hardcoded instruments/venues/products/strategy names in production modules (only in example wrappers).
4. Config is explicit and validated (fail-fast) with a single, documented configuration contract.
5. Redis schema is versioned, namespaced per strategy instance, documented, and has bounded growth policies.
6. Strategy hot path avoids Redis/network I/O; parameter updates are ingested on a timer/worker path.
7. Strategy reconciles order lifecycle end-to-end (filled/canceled/rejected/expired) and auto-cancels managed orders on stop.
8. Logging is useful for incident response (structured fields, severity, key state transitions) and follows Nautilus logging conventions.
9. Tests cover strategy invariants and key integration seams (not only helper math).

## Repo standards (must follow)

Primary references:

1. [coding_standards.md](/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/docs/developer_guide/coding_standards.md)
2. [python.md](/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/docs/developer_guide/python.md)
3. [strategies.md](/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/docs/concepts/strategies.md)
4. [logging.md](/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/docs/concepts/logging.md)
5. [message_bus.md](/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/docs/concepts/message_bus.md)
6. [live.md](/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/docs/concepts/live.md)
7. [testing.md](/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/docs/developer_guide/testing.md)
8. [architecture.md](/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr/docs/concepts/architecture.md)

## Current PR contents (what exists today)

Branch: `poc/makerv3-singleleg-mono-pr` (worktree: `/home/ubuntu/nautilus_trader/.worktrees/makerv3-mono-pr`)

Files added/changed in PR:

1. `.gitignore` (adds `.worktrees/` ignore and other entries)
2. `docs/plans/2026-03-03-nautilus-makerv3-single-leg-poc.md` (prototype plan artifact)
3. `examples/live/poc/*` (node wiring, contracts, bridge, API, README, smoke script)
4. `nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py` (strategy)
5. `tests/unit_tests/examples/strategies/test_makerv3_single_leg_quoter.py` (tests)

Key production gaps identified in review (high-level):

1. Unsafe/POC operational patterns in docs and runners (example: `eval`-based secret export).
2. Widespread hardcoding of instruments/venues/strategy IDs/topic prefixes.
3. Redis schema is not versioned or safely namespaced; multi-strategy contamination risks exist.
4. Redis growth is unbounded for some structures (trade hash map).
5. Strategy logic has hot-path I/O, incomplete order lifecycle reconciliation, missing stale maker-side MD gating, and dead failure config.

## Target module layout (proposed)

Goal: move reusable “flux integration” out of `examples/live/poc/*` into a first-class package.

Proposed layout:

1. `nautilus_trader/flux/common/`
2. `nautilus_trader/flux/params/`
3. `nautilus_trader/flux/bridge/`
4. `nautilus_trader/flux/api/`
5. `nautilus_trader/flux/strategies/makerv3/`
6. `examples/live/makerv3_single_leg/` (thin wrappers only)

## Redis schema (decision required)

Adopt a versioned keyspace prefix:

1. `flux:v1:...`

Hard requirements:

1. All keys/channels/streams are strategy-scoped unless explicitly global.
2. High-churn collections have bounded retention (streams with trimming or lists with `LTRIM`).
3. A single authoritative store per datum type (avoid map+list duplication where possible).

Proposed keys (first pass):

1. `flux:v1:stream:{strategy_id}:{topic}` (Redis stream)
2. `flux:v1:state:{strategy_id}` (JSON string)
3. `flux:v1:trades:stream:{strategy_id}` (Redis stream, trimmed)
4. `flux:v1:alerts:{strategy_id}` (list or stream, trimmed)
5. `flux:v1:params:{strategy_id}` (Redis hash)
6. `flux:v1:params:global` and `flux:v1:params:{strategy_id}` (pubsub channels)

## Status tracking (master checklist)

Phase 0: Review and plan

- [x] Complete PR review (engine + integration scope)
- [x] Write this productionization master plan

Phase 1: Naming, layout, and de-POC

- [ ] Create `nautilus_trader/flux/` package and move reusable code out of `examples/live/poc`
- [ ] Remove `poc` and `chainsaw` naming from code/docs (keep only example-specific labels in examples)
- [ ] Replace `nautilus_fluxapi.py` / `chainsaw_bridge.py` naming with `flux` naming

Phase 2: Config contract

- [ ] Define a single `FluxConfig` (and sub-configs) with validation
- [ ] Remove scattered `os.getenv`/magic defaults from strategy/bridge/api internals
- [ ] Document configuration fields and safe defaults

Phase 3: Redis schema + docs

- [ ] Implement versioned Redis key builders (`flux:v1`)
- [ ] Fix multi-strategy contamination paths by strategy-scoping all state
- [ ] Implement bounded retention for trades/events
- [ ] Document Redis schema and migration notes

Phase 4: Bridge productionization

- [ ] Extract bridge ingestion into modular handlers (topic -> handler map)
- [ ] Enforce timestamp normalization (ms) at ingest boundary
- [ ] Improve error handling and structured logs with correlation fields

Phase 5: Flux API productionization

- [ ] Split payload building from Redis I/O and adopt batched reads
- [ ] Remove hardcoded contracts/asset assumptions; inject via config/contract catalog
- [ ] Add basic readiness/health endpoints keyed to schema readiness

Phase 6: Strategy productionization

- [ ] Move strategy out of `nautilus_trader/examples/strategies/` into production module path
- [ ] Fix stale market-data gating on both legs; cancel managed orders on staleness
- [ ] Implement quote failure streak tracking and escalation/backoff using existing config knobs
- [ ] Remove Redis polling from hot book callbacks; move to timer/heartbeat path
- [ ] Implement full order lifecycle reconciliation (rejected/canceled/expired) + auto-cancel on stop
- [ ] Replace hardcoded venue/currency assumptions with config + instrument metadata
- [ ] Improve runtime logs (`self.log.*`) and keep msgbus publishes for external consumers

Phase 7: Tests + verification

- [ ] Add strategy-level tests for invariants (quote stack, cancel/replace, staleness, lifecycle events)
- [ ] Add unit tests for redis key builders, param schema validation, and bounded retention logic
- [ ] Add integration-ish tests around bridge handler transforms (pure functions where possible)

Phase 8: Docs and cleanup

- [ ] Add `docs/flux/redis_schema.md`, `docs/flux/params.md`, `docs/flux/bridge.md`, `docs/flux/api.md`
- [ ] Remove or replace `docs/plans/2026-03-03-nautilus-makerv3-single-leg-poc.md` with a durable production doc
- [ ] Keep `/.worktrees/` ignored in `.gitignore` (intentional repo policy for this repo)
- [ ] Keep `.run/` ignored in `.gitignore` (IDE/run configs; do not commit contents)

### Task execution tracker

- [x] Task 1: Create Flux package skeleton (`FluxRedisKeys` + `FluxConfig` + unit tests)
- [x] Task 2: Decide and document Redis schema (`flux:v1`)
- [ ] Task 3: Extract parameter subsystem and remove hot-path polling
- [ ] Task 4: Bridge hardening and handler modularization
- [ ] Task 5: Flux API refactor into app factory + batched Redis reads
- [ ] Task 6: Strategy productionization (core safety/perf work)
- [ ] Task 7: Replace POC runners with thin examples
- [ ] Task 8: Clean PR artifacts and enforce “no POC/chainsaw leakage”

---

## Execution plan (task-by-task)

Notes:

1. Each task below is designed to be executed with a tight diff and clear verification.
2. Prefer creating small, testable pure functions for transformations (bridge payload -> redis row).
3. Avoid changes that “look nice” but do not reduce production risk.

### Task 1: Create Flux package skeleton

**Files:**

- Create: `nautilus_trader/flux/__init__.py`
- Create: `nautilus_trader/flux/common/__init__.py`
- Create: `nautilus_trader/flux/common/keys.py`
- Create: `nautilus_trader/flux/common/config.py`

**Steps:**

1. Add a minimal `FluxRedisKeys` helper in `keys.py` with `flux:v1` prefix and strategy scoping.
2. Add `FluxConfig` dataclasses/structs in `config.py` with explicit required fields.
3. Add unit tests for key builders and config validation.

**Verify:**

```bash
pytest tests/unit_tests -q
```

### Task 2: Decide and document Redis schema (`flux:v1`)

**Files:**

- Create: `docs/flux/redis_schema.md`

**Steps:**

1. Write the schema table: key/channel, type, producer, consumer, retention/TTL, notes.
2. Document migration from current `maker_poc.*` / `maker_poc` prefixes.

**Verify:**

1. `rg -n \"maker_poc\" -S` should be limited to explicit one-time migration mapping references and examples only.

### Task 3: Extract parameter subsystem (hash + pubsub) and remove hot-path polling

**Files:**

- Create: `nautilus_trader/flux/params/manager.py`
- Modify: strategy module (new location) to use `FluxParamsManager` via timer-based polling
- Modify: API module to use batched reads (`HMGET`) not per-key `GET`

**Steps:**

1. Implement `FluxParamsManager.load()` using `HMGET` and coercion with strict unknown-key rejection.
2. Implement `FluxParamsManager.publish_update()` to `flux:v1:params:*` channels.
3. Remove any `pubsub.get_message()` calls from market-data callbacks; poll from a `Clock` timer.

**Verify:**

1. Add tests for: coercion, unknown key rejection, and update application.

### Task 4: Bridge hardening and handler modularization

**Files:**

- Create: `nautilus_trader/flux/bridge/stream_consumer.py`
- Create: `nautilus_trader/flux/bridge/handlers/*.py`
- Modify: move logic out of `examples/live/poc/chainsaw_bridge.py` into production modules

**Steps:**

1. Define handler interface: `(msg) -> writes` or `(msg, redis) -> None`.
2. Ensure every produced row includes `strategy_id` and adheres to `flux:v1` keys.
3. Replace unbounded `trades.map` with a stream/list with retention policy.
4. Normalize timestamps to integer milliseconds at ingest.

**Verify:**

1. Add unit tests for handlers as pure transforms where possible.

### Task 5: Flux API refactor into app factory + batched Redis reads

**Files:**

- Create: `nautilus_trader/flux/api/app.py`
- Create: `nautilus_trader/flux/api/payloads.py`
- Modify: move logic out of `examples/live/poc/nautilus_fluxapi.py`

**Steps:**

1. Create a Flask app factory taking `FluxConfig` + redis client.
2. Separate payload builders from Redis access.
3. Remove hardcoded contracts/assets; inject contract catalog/config.
4. Add readiness endpoint based on presence of required `flux:v1` keys.

**Verify:**

1. Add unit tests for payload builders and key lookups.

### Task 6: Strategy productionization (core safety/perf work)

**Files:**

- Move: `nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py` -> `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`
- Modify: tests to target new module path and add strategy-level tests

**Steps (must-hit safety items):**

1. Stale market-data gating on both legs and cancel managed orders when blocked.
2. Implement quote failure streak tracking and escalation based on existing config knobs.
3. Add order lifecycle handlers: rejected/canceled/expired + full reconciliation of local state.
4. Auto-cancel on stop (cancel all strategy-tagged managed orders).
5. Remove any redis pubsub polling from hot callbacks.
6. Remove venue/product/currency hardcoding; validate config at `on_start`.
7. Add `self.log.*` events for major transitions and exceptions.

**Verify:**

```bash
pytest tests/unit_tests/examples/strategies -q
```

### Task 7: Replace POC runners with thin examples

**Files:**

- Create: `examples/live/makerv3_single_leg/README.md`
- Create: `examples/live/makerv3_single_leg/run_node.py`
- Create: `examples/live/makerv3_single_leg/run_bridge.py`
- Create: `examples/live/makerv3_single_leg/run_api.py`
- Delete or deprecate: `examples/live/poc/*`

**Steps:**

1. Runners should import production modules only.
2. No secrets bootstrap via `eval`. Document safe, explicit secret loading.
3. Examples default to safe mode (`paper` or non-live) unless `--confirm-live` is passed.

**Verify:**

1. `rg -n \"examples/live/poc\" -S` should find no active imports from production modules.

### Task 8: Clean PR artifacts and enforce “no POC/chainsaw leakage”

**Files:**

- Modify: `.gitignore` (keep `/.worktrees/` ignored; ensure `.run/` is ignored)
- Remove: any tracked `.run/*` artifacts (keep directory ignored/untracked)
- Replace: the old POC plan doc with durable production docs

**Steps:**

1. Add a CI-ish grep check (or at least a documented local check) for `poc` and `chainsaw` strings in production paths.
2. Ensure no absolute host paths remain in docs.

**Verify:**

```bash
rg -n \"\\bpoc\\b|maker_poc|\\bchainsaw\\b\" -S nautilus_trader docs examples
```

---

## Addendum: Second-pass deep review details (subagent findings consolidated)

This addendum is intentionally specific. It captures the “production blockers” and the concrete refactor/test work required to close them.

### Production blocking checklist (must be explicitly satisfied)

Strategy (engine)

1. Enforce maker-leg and reference-leg market data freshness gating before quoting.
2. On staleness: cancel managed orders and enter a clearly logged/published blocked state.
3. Implement quote failure circuit breaker using existing `quote_fail_critical_after_*` knobs.
4. Implement full order lifecycle reconciliation: filled, rejected, canceled, expired.
5. Guarantee cancel-on-stop even when cache visibility is imperfect.
6. Remove all network I/O and Redis socket reads from `on_order_book_deltas` hot path.

Bridge (ingestion)

1. No global, unscoped keys for strategy-specific state (FV, trades, alerts, balances).
2. Remove unbounded `trades.map` growth; bounded retention must apply to all authoritative stores.
3. Normalize timestamps at ingest boundary; persist canonical `ts_ms` as integer milliseconds.
4. All handler writes include correlation context: `strategy_id`, `topic`, and (if stream-based) `entry_id`.

API

1. No hardcoded `strategy_id`, assets, contracts, or `POC_REDIS_*` env parsing in production modules.
2. Batch Redis reads (`HMGET`/pipelines) for params and list endpoints; avoid per-key `GET` loops.
3. Explicit readiness/health endpoints that validate dependencies, not “snapshot presence”.
4. Standard response/error envelope and pagination/limit caps.

Config and examples

1. Replace scattered `POC_*` env vars with a single explicit `FluxConfig` contract + validation.
2. Add run modes (`paper`, `testnet`, `live`) and require explicit confirmation for `live`.
3. Remove unsafe secret bootstrap instructions (no `eval`).

Tests

1. Add strategy-level tests for invariants and lifecycle behavior (not only helper math).
2. Add tests for Redis key builders, bounded retention, timestamp normalization.

### Detailed Redis schema (`flux:v1`) and retention policy (bridge + API contract)

Goals:

1. Strategy-scoped by default.
2. Versioned keys (`flux:v1`) to enable future evolution.
3. Bounded retention for high churn.

Recommended namespace conventions:

1. Output keys: `flux:v1:{domain}:{strategy_id}:...`
2. Inbound streams (bridge input): `flux:v1:in:stream:{environment}:{strategy_id}:{topic}`

Canonical output keys (recommended):

1. `flux:v1:state:{strategy_id}` (string JSON; latest only)
2. `flux:v1:events:{strategy_id}` (stream or list; bounded)
3. `flux:v1:trades:stream:{strategy_id}` (stream; bounded by `MAXLEN`)
4. `flux:v1:trades:meta:{strategy_id}` (hash; optional mirror; bounded cleanup must be implemented if used)
5. `flux:v1:alerts:{strategy_id}` (stream or list; bounded)
6. `flux:v1:balances:snapshot:{strategy_id}` (string JSON; latest only)
7. `flux:v1:balances:rows:{strategy_id}` (hash; keyed by deterministic `exchange:asset:account`)
8. `flux:v1:market:last:{strategy_id}:{exchange}:{base}_{quote}` (string JSON; latest only)
9. `flux:v1:fv:{strategy_id}` (stream or list; bounded; avoid a naked global snapshot)
10. `flux:v1:params:{strategy_id}` (hash)
11. `flux:v1:params:global` and `flux:v1:params:{strategy_id}` (pubsub channels)

Retention policy defaults (tune later, but must exist):

1. `events`: keep last 300 to 2000 entries
2. `alerts`: keep last 200 to 1000 entries
3. `trades`: keep last 1000 to 5000 entries
4. `fv`: keep last 200 to 1000 entries, or store latest only if consumers do not need history

Migration policy:

1. Apply a one-time cutover from `maker_poc.*` / `maker_poc` producers to `flux:v1:in:stream:{environment}:{strategy_id}:{topic}`.
2. Production modules ship as a single clean build that reads and writes only `flux:v1:*` keys/channels.
3. Runtime legacy-read paths and feature-flagged dual modes are out of scope by policy.

### FluxConfig contract (explicit configuration model)

Recommended module:

1. `nautilus_trader/flux/common/config.py`

Required top-level fields (proposal):

1. `mode`: `paper | testnet | live`
2. `confirm_live`: bool (required for `mode=live`)
3. `identity`: namespace, schema_version, `strategy_id`, `strategy_instance_id`, `trader_id`, `external_strategy_id`
4. `redis`: host, port, db, optional username/password, connect/read timeouts
5. `venues`: execution_venue, reference_venue, execution_symbol, reference_symbol
6. `execution`: enable_execution, product_type, testnet, demo, reconciliation settings
7. `msgbus`: stream prefix and prefix flags (`use_trader_prefix`, `use_trader_id`, `use_instance_id`, `stream_per_topic`)
8. `runtime_params`: all runtime strategy knobs (ladder, safety, bot_on)

Validation rules:

1. `mode=live` requires `confirm_live=true` or `--confirm-live`.
2. `enable_execution=true` requires credentials and an explicit run mode.
3. Strategy identifiers must be identifier-safe and must not embed venue/product hardcoding in code.

Example config (TOML) path to create:

1. `examples/live/makerv3_single_leg/config/makerv3_single_leg.toml`

### API hardening (refactor boundaries)

Target layout:

1. `nautilus_trader/flux/api/storage.py` for Redis access and batching
2. `nautilus_trader/flux/api/payloads.py` for schema, validation, and canonical envelopes
3. `nautilus_trader/flux/api/app.py` as Flask app factory + middleware + DI

Required properties:

1. Batch Redis reads for params and feeds.
2. Remove strategy/contract hardcoding; inject via config/catalog provider.
3. Add `healthz` and `readyz` with dependency checks (Redis connectivity, keyspace readiness).
4. Standard response envelope: include `api_version`, `request_id`, and timestamp.
5. Enforce pagination and response size caps.
6. Explicit auth mode and CORS policy (config-driven), even if initially disabled by default.

### Bridge hardening (handler modularization)

Required changes:

1. Split bridge into a stream consumer + handler modules per topic.
2. Ensure every handler writes only strategy-scoped keys and includes `strategy_id` in persisted rows.
3. Remove `trades.map` leak by making a bounded stream/list authoritative and cleaning any mirrors.
4. Normalize and persist `ts_ms` in every row type at ingest.

### Strategy refactor (componentization + perf)

Required changes:

1. Implement the quote failure circuit breaker (`quote_fail_critical_after_*`) as a real state machine.
2. Implement full order lifecycle reconciliation (reject/cancel/expire) and idempotency.
3. Enforce maker+ref market data freshness gates; blocked state must be explicit and observable.
4. Move runtime param ingestion out of `on_order_book_deltas` and into a timer/worker; callback reads only last-known in-memory params.
5. Strengthen cancel-on-stop so it does not depend on `_managed_orders()` being visible.

Suggested extraction targets under `nautilus_trader/flux/strategies/makerv3/`:

1. `quote_math.py` for pure ladder planning and actions
2. `market_health.py` for staleness/block policies
3. `lifecycle.py` for order event reconciliation and failure tracking
4. `params.py` for async-safe param ingestion

### Test plan (post-refactor)

Strategy tests should be moved under:

1. `tests/unit_tests/flux/strategies/makerv3/`

Must-have test cases:

1. Lifecycle: `on_start` idempotency, `on_stop` cancel guarantee, param subscription behavior.
2. Quote invariants: max orders per side, improving-only replacement, tick alignment, monotonic ladders.
3. Staleness matrix: maker stale, ref stale, both stale; ensure cancel + blocked state + resume policy.
4. Order reconciliation: rejected/canceled/expired paths and idempotency on duplicate events.
5. Params: schema validation, unknown key rejection, version monotonicity, no “cancel storm” on non-structural updates.

### Verification gates (repeatable)

Textual policy checks:

```bash
rg -n \"\\bpoc\\b|POC_|maker_poc|\\bchainsaw\\b\" -S nautilus_trader docs examples
```

Focused unit tests:

```bash
python -m pytest tests/unit_tests -q
```

## Decisions

1. Redis schema namespace for production modules is fixed to `flux:v1` with strategy-scoped keys by default; key builders allow namespace/schema injection only for controlled testing.
2. Config contract starts with explicit typed config structs in `nautilus_trader/flux/common/config.py`; unsupported schema versions fail fast at construction time.
3. Retention defaults are mandatory for high-churn streams: `events` 1000, `alerts` 500, `trades` 3000, `fv` 500 (with bounded tuning ranges documented in `docs/flux/redis_schema.md`).
4. Migration policy is hard cutover: one clean production build with `flux:v1:*` reads/writes only and no runtime legacy-read path.

## Progress log

1. 2026-03-04T00:47:15Z | Task 1 - Flux package skeleton (`FluxRedisKeys` + `FluxConfig` + tests) | SHAs: `8570583d3`, `11249bc3b61a` | Notes: Implemented with TDD and review loops; added strict schema/version and Redis validation; Task 1 spec review ✅ and code-quality review ✅.
2. 2026-03-04T00:54:07Z | Task 2 - Redis schema decision + durable documentation | SHAs: `919df6857876f3ca64559936491a92797408e076`, `d916df85b` | Notes: Added `docs/flux/redis_schema.md` with canonical keys/channels, retention policy, `ts_ms` contract, and explicit one-time legacy mapping under hard cutover policy (no runtime legacy reads); Task 2 spec review ✅ and code-quality review ✅.
