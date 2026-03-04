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

1. Fluxboard/UI work beyond the TokenMM migration slice (tracked separately in `docs/plans/2026-03-04-fluxboard-tokenmm-minimal-migration.md`).
2. Adding new venues/features beyond what is already present; focus on hardening/modularizing existing behavior.

## Production bar (acceptance criteria)

1. No `poc` naming in shipped production module paths, strategy IDs, topic prefixes, env vars, durable Flux docs,
   or defaults. Allowlisted legacy mapping references in `docs/flux/redis_schema.md` and deprecated example
   wrappers are excluded.
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

1. [coding_standards.md](docs/developer_guide/coding_standards.md)
2. [python.md](docs/developer_guide/python.md)
3. [strategies.md](docs/concepts/strategies.md)
4. [logging.md](docs/concepts/logging.md)
5. [message_bus.md](docs/concepts/message_bus.md)
6. [live.md](docs/concepts/live.md)
7. [testing.md](docs/developer_guide/testing.md)
8. [architecture.md](docs/concepts/architecture.md)

## Current PR contents (what exists today)

Branch: `poc/makerv3-singleleg-mono-pr` (worktree: `.worktrees/makerv3-mono-pr`)

Major surfaces added/changed:

1. `nautilus_trader/flux/common/*` (typed config + Redis key builders)
2. `nautilus_trader/flux/params/*` (parameter manager + pub/sub semantics)
3. `nautilus_trader/flux/bridge/*` (modular handlers + stream consumer + bounded retention)
4. `nautilus_trader/flux/api/*` (app-factory API + payload builders + readiness/health)
5. `nautilus_trader/flux/strategies/makerv3/*` (production strategy; further refactor tracked separately)
6. `docs/flux/*` (durable schema/params/bridge/api docs; schema includes allowlisted legacy mapping)
7. `scripts/ci/check-flux-leakage.sh` + CI/pre-commit wiring (`.github/workflows/build.yml`, `.pre-commit-config.yaml`)
8. `examples/live/makerv3/*` (thin runners + config + README) and deprecated wrappers under `examples/live/poc/*`
9. Unit test coverage under `tests/unit_tests/flux/*` and `tests/unit_tests/examples/*`
10. `fluxboard/*` (Fluxboard Vite app) + `docs/fluxboard/*` (TokenMM contracts + runbook) + `docs/plans/2026-03-04-fluxboard-tokenmm-minimal-migration.md`

Production gaps identified in review (high-level) and status:

1. Unsafe operational patterns in docs/runners (example: `eval`-based secret export): resolved (Task 7).
2. Widespread hardcoding of instruments/venues/strategy IDs/topic prefixes: resolved for production modules; examples remain config-driven (Tasks 1-7).
3. Redis schema not versioned / multi-strategy contamination risk: resolved via `flux:v1` + strict strategy scoping (Tasks 1-4).
4. Unbounded Redis growth paths: resolved via bounded retention defaults + docs (Tasks 2, 4).
5. Strategy hot-path I/O and incomplete lifecycle reconciliation: resolved for production readiness baseline (Task 6); further modularization/refactor is tracked separately.
6. Socket.IO emitter lifecycle/perf/observability gaps for Fluxboard realtime: resolved via explicit profile refcounts, idle wake/sleep behavior, bounded per-profile error backoff/logging, and zero-ref state cleanup in `nautilus_trader/flux/api/socketio.py`.

## Target module layout (proposed)

Goal: move reusable “flux integration” out of `examples/live/poc/*` into a first-class package.

Proposed layout:

1. `nautilus_trader/flux/common/`
2. `nautilus_trader/flux/params/`
3. `nautilus_trader/flux/bridge/`
4. `nautilus_trader/flux/api/`
5. `nautilus_trader/flux/strategies/makerv3/`
6. `examples/live/makerv3/` (thin wrappers only)

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

- [x] Create `nautilus_trader/flux/` package and move reusable code out of `examples/live/poc`
- [x] Remove `poc` and `chainsaw` naming from production code/durable docs (allowlisted migration mapping and deprecated examples are excluded)
- [x] Replace `nautilus_fluxapi.py` / `chainsaw_bridge.py` naming with `flux` naming

Phase 2: Config contract

- [x] Define a single `FluxConfig` (and sub-configs) with validation
- [x] Remove scattered `os.getenv`/magic defaults from strategy/bridge/api internals
- [x] Document configuration fields and safe defaults

Phase 3: Redis schema + docs

- [x] Implement versioned Redis key builders (`flux:v1`)
- [x] Fix multi-strategy contamination paths by strategy-scoping all state
- [x] Implement bounded retention for trades/events
- [x] Document Redis schema and migration notes

Phase 4: Bridge productionization

- [x] Extract bridge ingestion into modular handlers (topic -> handler map)
- [x] Enforce timestamp normalization (ms) at ingest boundary
- [x] Improve error handling and structured logs with correlation fields

Phase 5: Flux API productionization

- [x] Split payload building from Redis I/O and adopt batched reads
- [x] Remove hardcoded contracts/asset assumptions; inject via config/contract catalog
- [x] Add basic readiness/health endpoints keyed to schema readiness

Phase 6: Strategy productionization

- [x] Move strategy out of `nautilus_trader/examples/strategies/` into production module path
- [x] Fix stale market-data gating on both legs; cancel managed orders on staleness
- [x] Implement quote failure streak tracking and escalation/backoff using existing config knobs
- [x] Remove Redis polling from hot book callbacks; move to timer/heartbeat path
- [x] Implement full order lifecycle reconciliation (rejected/canceled/expired) + auto-cancel on stop
- [x] Replace hardcoded venue/currency assumptions with config + instrument metadata
- [x] Improve runtime logs (`self.log.*`) and keep msgbus publishes for external consumers

Phase 7: Tests + verification

- [x] Add strategy-level tests for invariants (quote stack, cancel/replace, staleness, lifecycle events)
- [x] Add unit tests for redis key builders, param schema validation, and bounded retention logic
- [x] Add integration-ish tests around bridge handler transforms (pure functions where possible)

Phase 8: Docs and cleanup

- [x] Add `docs/flux/redis_schema.md`, `docs/flux/params.md`, `docs/flux/bridge.md`, `docs/flux/api.md`
- [x] Archive `docs/plans/2026-03-03-nautilus-makerv3-single-leg-poc.md` as a prototype artifact; treat `docs/flux/*` as durable production docs
- [x] Keep `/.worktrees/` ignored in `.gitignore` (intentional repo policy for this repo)
- [x] Keep `.run/` ignored in `.gitignore` (IDE/run configs; do not commit contents)

### Task execution tracker

- [x] Task 1: Create Flux package skeleton (`FluxRedisKeys` + `FluxConfig` + unit tests)
- [x] Task 2: Decide and document Redis schema (`flux:v1`)
- [x] Task 3: Extract parameter subsystem and remove hot-path polling
- [x] Task 4: Bridge hardening and handler modularization
- [x] Task 5: Flux API refactor into app factory + batched Redis reads
- [x] Task 6: Strategy productionization (core safety/perf work)
- [x] Task 7: Replace POC runners with thin examples
- [x] Task 8: Clean PR artifacts and enforce “no POC/chainsaw leakage”
- [x] Task 9: Follow-up gate (bridge offsets, API legs keying, CI plotly check, config uniqueness, bridge runner scope hardening)
- [x] Task 10: Non-overlap follow-up wave (CI/pre-commit gate wiring, redis-schema allowlist enforcement, API localhost defaults, plotly test guard)
- [x] Task 11: Fluxboard TokenMM migration (Fluxboard app + TokenMM contracts/runbook + compat; tracked in `docs/plans/2026-03-04-fluxboard-tokenmm-minimal-migration.md`)
- [x] Task 12: Socket.IO emitter lifecycle/perf hardening (idle when no profiles, avoid room scans, per-profile backoff/logging, bounded emission)

### Task 12 progress log

- [2026-03-04 09:58 UTC] Task 12: Implemented `FluxSocketEmitter` active-profile refcount bookkeeping (connect/disconnect/set_profile hooks), event-driven idle loop wakeups, per-profile error isolation with bounded backoff+logging, and zero-ref profile state cleanup; added lifecycle/error-isolation tests in `tests/unit_tests/flux/api/test_socketio_tokenmm.py` / evidence: `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/api/test_socketio_tokenmm.py --confcutdir=tests/unit_tests/flux/api` (15 passed) / SHA: `ec1625a45`.

### Follow-up gate tracker (P0 + P1)

- [x] P0: Bridge offset semantics hardened (advance offsets only after decode+handler+write success; no advance on decode/handler/write failures; broad write failure catch)
- [x] P0: API `legs` map keyed by `contract_id = "{exchange}:{symbol}"` (exchange lower + symbol upper), preserving `exchange`/`symbol` fields in row payloads
- [x] P1: Added explicit Plotly availability verification in wheel build action (`uv run --no-sync python -c "import plotly.graph_objects as go"`)
- [x] P1: Enforced `strategy_instance_id == strategy_id` in `FluxIdentityConfig` without changing `FluxRedisKeys` format
- [x] P1: Bridge runner wildcard hardening (`--all-strategies`, mutual exclusion with `--strategy-id`, fail-fast strategy scope validation)

### Task 10 follow-up tracker (non-overlap)

- [x] Added leakage gate wiring in `.pre-commit-config.yaml` and explicit `build.yml` pre-commit job step.
- [x] Added redis-schema migration allowlist markers and enforced banned-name scan in `docs/flux/redis_schema.md` outside allowlist only.
- [x] Changed example API bind defaults to localhost (`127.0.0.1`) in runner/config and documented explicit external-exposure override.
- [x] Guarded Plotly-dependent tearsheet unit module import to skip cleanly when Plotly is not installed.

### Follow-up verification checklist

```bash
scripts/ci/check-flux-leakage.sh
# NOTE: repo-wide grep for legacy naming is expected to match:
# - docs/flux/redis_schema.md inside the allowlisted migration block
# - examples/live/poc/* deprecated wrappers (kept for transition)
# Use scripts/ci/check-flux-leakage.sh as the authoritative production leakage gate.
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3 -m pytest tests/unit_tests/flux/common -q
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3 -m pytest tests/unit_tests/flux/bridge -q
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3 -m pytest tests/unit_tests/flux/api -q
PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 python3 -m pytest tests/unit_tests/examples -q
```

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

- Move: `nautilus_trader/examples/strategies/makerv3.py` -> `nautilus_trader/flux/strategies/makerv3/single_leg_quoter.py`
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

- Create: `examples/live/makerv3/README.md`
- Create: `examples/live/makerv3/run_node.py`
- Create: `examples/live/makerv3/run_bridge.py`
- Create: `examples/live/makerv3/run_api.py`
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

Retention policy defaults and allowed ranges are authoritative in `docs/flux/redis_schema.md` under **High-churn retention defaults**.

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

1. `examples/live/makerv3/config/makerv3.toml`

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
scripts/ci/check-flux-leakage.sh
```

Focused unit tests:

```bash
python -m pytest tests/unit_tests -q
```

## Decisions

1. Redis schema namespace for production modules is fixed to `flux:v1` with strategy-scoped keys by default; key builders allow namespace/schema injection only for controlled testing.
2. Config contract starts with explicit typed config structs in `nautilus_trader/flux/common/config.py`; unsupported schema versions fail fast at construction time.
3. Retention defaults are mandatory for high-churn streams; `docs/flux/redis_schema.md` is the single source of truth for numeric defaults and allowed tuning ranges.
4. Migration policy is hard cutover: one clean production build with `flux:v1:*` reads/writes only and no runtime legacy-read path.
5. Runtime parameter ingestion is centralized in `FluxParamsManager` and runs on timer-driven refresh paths; market-data callbacks must read in-memory params only.
6. Bridge ingestion is modularized as topic handlers plus a stream consumer; handlers emit strategy-scoped `flux:v1` writes with bounded retention and normalized `ts_ms` at ingest.
7. API contract is factory-based (`FluxConfig` + injected Redis client), with centralized envelopes (`api_version`, `request_id`, `timestamp_ms`) and explicit health/readiness schema checks.
8. Production strategy implementation lives under `nautilus_trader/flux/strategies/makerv3/` with two-leg staleness gating, lifecycle reconciliation, and quote-failure circuit breaker fail-stop semantics.
9. Production leakage policy is enforced via `scripts/ci/check-flux-leakage.sh`, which fails on POC/chainsaw naming in production Flux paths and on absolute host paths in durable Flux docs.
10. Follow-up gate policy keeps Redis key format unchanged while enforcing config-level identity uniqueness (`strategy_instance_id == strategy_id`) and explicitly forbids edits under `nautilus_trader/flux/strategies/*` in this wave to avoid worker-collision risk.
11. Task 10 non-overlap wave boundaries remain strict: no edits under `nautilus_trader/flux/strategies/*` and no changes to untracked plan docs; `docs/flux/redis_schema.md` migration references are allowed only within explicit `leakage-allowlist` markers and are leakage-gated everywhere else.

## Progress log

1. 2026-03-04T00:47:15Z | Task 1 - Flux package skeleton (`FluxRedisKeys` + `FluxConfig` + tests) | SHAs: `8570583d3`, `11249bc3b61a` | Notes: Implemented with TDD and review loops; added strict schema/version and Redis validation; Task 1 spec review ✅ and code-quality review ✅.
2. 2026-03-04T00:54:07Z | Task 2 - Redis schema decision + durable documentation | SHAs: `919df6857876f3ca64559936491a92797408e076`, `d916df85b` | Notes: Added `docs/flux/redis_schema.md` with canonical keys/channels, retention policy, `ts_ms` contract, and explicit one-time legacy mapping under hard cutover policy (no runtime legacy reads); Task 2 spec review ✅ and code-quality review ✅.
3. 2026-03-04T01:18:14Z | Task 3 - Params subsystem extraction + hot-path polling removal | SHAs: `915772bd9160e5dcf2981b67a99f58f443b72653`, `6bf0fbc70084cb79f4c15af99e3f197bb2c4695d` | Notes: Added `FluxParamsManager` (`HMGET` load, strict unknown-key rejection, update+publish), moved strategy param refresh to timer path, removed market-data callback network polling, and switched API params read/write path to `flux:v1` hash/channel semantics; Task 3 spec review ✅ and code-quality review ✅.
4. 2026-03-04T01:33:29Z | Task 4 - Bridge modularization + schema hardening | SHAs: `1ee44e2be39cce1688d886a5f66f8265252ebcbe`, `7c53ac9bf983689a872519a7cef2aa98dab06f8a` | Notes: Moved bridge ingestion into `nautilus_trader/flux/bridge/*` with topic handlers + consumer, replaced monolithic runner with thin wrapper, enforced strategy-scoped `flux:v1` outputs with bounded stream retention, normalized `ts_ms`, and added consumer boundary tests; Task 4 spec review ✅ and code-quality review ✅.
5. 2026-03-04T01:52:58Z | Task 5 - API package refactor + envelope/readiness hardening | SHAs: `7b9c2deb8425171b956e5887cb9246ca7bd0d54d`, `8d93d0d5f2241e2678d3b1380f0d445f6c6fe345` | Notes: Moved API logic into `nautilus_trader/flux/api/*` with DI app factory, store/payload separation, batched feed reads, schema-based readiness/health endpoints, explicit validation/error envelopes, and thin example runner wiring; Task 5 spec review ✅ and code-quality review ✅.
6. 2026-03-04T02:13:41Z | Task 6 - Strategy productionization safety/perf refactor | SHAs: `811339f6e4023296e5e11498c7fab28c259dfdd0`, `13295ccf0b91b74012a42276e92e164854b89231` | Notes: Added production strategy module under `nautilus_trader/flux/strategies/makerv3/`, implemented two-leg staleness gating + cancel, quote-failure circuit breaker fail-stop, lifecycle reconciliation for reject/cancel/expire, and stronger cancel-on-stop tracking semantics with new strategy-level tests; Task 6 spec review ✅ and code-quality review ✅.
7. 2026-03-04T02:26:03Z | Task 7 - Replace POC runners with thin examples | SHAs: `6758d180f09eb607ee198667d23082c8b4e39ea2` | Notes: Added `examples/live/makerv3/*` (node/bridge/api runners + README + config), converted `examples/live/poc/*` runners into thin deprecated wrappers, removed unsafe secret bootstrap patterns, and kept run modes explicit; Task 7 spec review ✅ and code-quality review ✅.
8. 2026-03-04T03:04:17Z | Task 8 - PR cleanup + no POC/chainsaw leakage enforcement | SHAs: `aa57547d5`, `689ec0b39`, `67d2fa1b9`, `32a2dcd97`, `c50489073`, `7ea168551` | Notes: Renamed production bus payload type to `FluxBusPayload`, removed POC envelope compatibility from bridge, added durable Flux docs (`params.md`, `bridge.md`, `api.md`), replaced archived prototype plan doc with durable pointer, preserved `/.worktrees/` and `.run/` ignores, and hardened `scripts/ci/check-flux-leakage.sh` through spec/quality fix loops (case-insensitive leakage terms, `POC_*`/`*_poc` detection, generalized host-path checks, and URL-safe Windows path matching); Task 8 spec review ✅ and code-quality review ✅.
9. 2026-03-04T03:08:07Z | Task 9 (P0) - Bridge offset semantics hardening | SHAs: `fb4f99e3b` | Notes: Moved offset advancement to post-write success only, retained offsets on decode/handler/write failure paths, broadened write exception catch in run loop, and added run-loop regression tests for offset behavior.
10. 2026-03-04T03:11:57Z | Task 9 (P0) - API legs contract_id keying fix | SHAs: `a90941dab` | Notes: Added contract-id-based legs keying (`{exchange}:{symbol}` normalization), removed same-exchange symbol collision path, and added regression tests for same-exchange multi-symbol contracts.
11. 2026-03-04T03:12:00Z | Task 9 (P1) - Plotly CI import gate | SHAs: `f2ada0c7f` | Notes: Added explicit `plotly.graph_objects` import verification step to common wheel-build action.
12. 2026-03-04T03:12:04Z | Task 9 (P1) - Identity uniqueness policy | SHAs: `5182a4e65` | Notes: Enforced `strategy_instance_id == strategy_id` in `FluxIdentityConfig`, updated common config/key tests, and documented no-schema-change uniqueness policy.
13. 2026-03-04T03:12:09Z | Task 9 (P1) - Bridge runner wildcard hardening | SHAs: `176f0685c` | Notes: Added `--all-strategies` support with strict scope validation (mutual exclusion with `--strategy-id`, fail-fast when scope missing) plus runner unit tests and README updates.
14. 2026-03-04T03:24:38Z | Task 9 (quality loop) - Bridge batch failure semantics + CI isolation hardening | SHAs: `461001804` | Notes: Hardened per-stream batch processing to stop at first failed entry and avoid offset advancement past failures; fixed example default config to satisfy new identity policy; added API-runner config smoke test; moved Plotly verification into isolated temporary virtualenv with wheel install.
15. 2026-03-04T03:27:50Z | Task 9 (quality loop) - CI wheel URI portability | SHAs: `4d12202aa` | Notes: Replaced manual `file://` wheel URI construction with Python `Path(...).resolve().as_uri()` in common wheel-build action to avoid platform path formatting pitfalls.
16. 2026-03-04T03:52:05Z | Task 10 - CI/pre-commit leakage gate wiring | SHAs: `4f4b5134e` | Notes: Added local pre-commit hook `check-flux-leakage` and explicit `build.yml` pre-commit job step to run `bash scripts/ci/check-flux-leakage.sh`.
17. 2026-03-04T03:52:08Z | Task 10 - Redis schema allowlist enforcement | SHAs: `c18d7aebe` | Notes: Wrapped `maker_poc` migration references in explicit allowlist markers in `docs/flux/redis_schema.md` and updated leakage script to enforce banned-name checks on that doc outside allowlist only.
18. 2026-03-04T03:52:15Z | Task 10 - Example API localhost default + docs/tests | SHAs: `9eb815633` | Notes: Defaulted example API bind host to `127.0.0.1` in runner/config, updated runner/API docs for explicit external exposure override, and extended run_api tests for host-resolution behavior.
19. 2026-03-04T03:52:26Z | Task 10 - Plotly import guard for analysis tests | SHAs: `3dd928c8b` | Notes: Replaced unconditional top-level Plotly import in tearsheet unit tests with guarded import handling so collection stays stable when Plotly is absent.
20. 2026-03-04T04:01:42Z | Task 10 (quality loop) - guard robustness fixes | SHAs: `db7296ddb` | Notes: Switched tearsheet module to collect-then-skip (`HAS_PLOTLY` + `pytest.mark.skipif`) to avoid file-only exit-code-5 behavior when Plotly is missing, and hardened leakage marker counting in `scripts/ci/check-flux-leakage.sh` to emit explicit diagnostics under strict shell settings.
