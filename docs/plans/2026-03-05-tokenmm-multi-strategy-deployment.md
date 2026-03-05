# TokenMM Multi-Strategy Deployment & Portfolio Plan

> Scope: review the current Flux/MakerV3 setup in this PR branch and propose a best-practice deployment + workflow for running **5 strategy instances** that **share one portfolio**, with Fluxboard showing **portfolio-level balances** (not strategy-segregated).

## TL;DR (recommendation)

1. **Deploy 1 TradingNode process per strategy instance** (current Flux stream schema strongly assumes this), but run **one shared bridge** (`--all-strategies`) and **one shared Flux API + Fluxboard**.
2. Treat **balances/positions as a portfolio resource** for TokenMM:
   - Implement **API-level aggregation** for `GET /api/v1/balances?profile=tokenmm` when no `strategy=...` is provided.
   - Keep per-strategy balances available via `GET /api/v1/balances?strategy=<id>` for debugging.
3. Make adding a strategy “drop-in” by using a **strategy config directory** and a **stack script** which starts N nodes + 1 bridge + 1 API.

If you need **atomic shared risk/portfolio management across strategies**, move to a **single node hosting multiple strategies** — but that requires a **Flux ingestion redesign** (bridge routing can’t currently separate strategy keyspaces when multiple strategies share one node/message-bus stream prefix).

---

## What we have today (in this repo)

### Process model (examples + deploy script)

- Node runner: `examples/live/makerv3/run_node.py`
  - Builds a `TradingNode` and adds **one** `MakerV3Strategy`.
  - MessageBus is configured to write inbound Redis streams under:
    - `MessageBusConfig.streams_prefix = "{namespace}:{schema_version}:in:stream:{mode}:{identity.strategy_id}"`
    - With `stream_per_topic=True`, inbound streams look like:
      - `flux:v1:in:stream:<env>:<strategy_id>:<topic>`
- Bridge runner: `examples/live/makerv3/run_bridge.py`
  - Consumes one strategy by default, or all strategies with `--all-strategies`.
- API runner: `examples/live/makerv3/run_api.py`
  - Reads/writes Flux schema (`flux:v1:*`) via `nautilus_trader/flux/api/app.py`.
  - Can serve Fluxboard at `/tokenmm/*`.
- Stack launcher: `scripts/deploy/makerv3_stack.sh`
  - Starts: redis (optional) + **one** node + **one** bridge + **one** api.

### Strategy scoping is “hard” today (Flux bridge)

- Flux bridge consumer reads inbound streams whose keys are **already strategy-scoped**:
  - `nautilus_trader/flux/bridge/stream_consumer.py`
  - Output key selection uses the **strategy_id from the stream key**, and intentionally ignores payload `strategy_id`.
- All Flux Redis keys are strategy-scoped today:
  - `nautilus_trader/flux/common/keys.py` (`flux:v1:...:{strategy_id}`)

This is a good default for multi-strategy deployments **when each strategy has its own node/process**.

### Why balances “segregate by strategy_id” today

There are two different segregation mechanisms:

1. **Storage + API are strategy-scoped.**
   - Bridge writes `flux:v1:balances:snapshot:{strategy_id}`.
   - `GET /api/v1/balances` resolves a *single* `strategy_id` (even for `profile=tokenmm`) in `nautilus_trader/flux/api/app.py`.
2. **Each strategy’s balances payload tends to become instrument-scoped.**
   - `nautilus_trader/flux/strategies/makerv3/publisher.py::publish_balances` prefers:
     - `strategy.cache.positions_open(instrument_id=strategy.config.maker_instrument_id)`
     - and only falls back to `positions_open()` (all instruments) if the first query returns empty.
   - With multiple strategies trading different instruments, each strategy’s published balances often contain only *its* instrument positions once any position exists.

So if Fluxboard needs a **single shared TokenMM portfolio view**, we must either:
- **merge** balances across the tokenmm strategy set at the API seam, and/or
- change the publisher to always publish a portfolio-complete positions set, and/or
- introduce a first-class portfolio balances keyspace.

---

## Best-practice guidance from Nautilus (relevant to the node topology question)

### Facts (from repo docs)

- **One node per process** (cannot run multiple nodes concurrently in one process):
  - `docs/concepts/live.md`
  - `docs/concepts/architecture.md`
- For production, you can:
  - **add multiple strategies to a single TradingNode** (shared Portfolio/Risk/Execution engines), or
  - run **additional nodes in separate processes** for isolation/parallelism:
    - `docs/concepts/live.md`
    - `docs/concepts/architecture.md`
- The Portfolio is node-scoped and aggregates positions “across active strategies for the trading node”:
  - `docs/concepts/portfolio.md`
- Multiple instances of the same strategy require unique IDs / order tags:
  - `docs/concepts/strategies.md` (“Multiple strategies” section)

### Answer: “Are we sure deploying on separate nodes is right?”

**It’s right if your primary goal is per-strategy operational isolation + the current Flux strategy-scoped key model.**

It’s *not* sufficient by itself if you require:
- truly shared in-process portfolio state, or
- atomic portfolio-level risk controls spanning all strategies (global caps, cross-instrument netting decisions, etc.).

In that case, the “best Nautilus” setup is a **single TradingNode with multiple strategies**.
But because Flux ingestion is currently stream-key-scoped, that would require a deliberate Flux redesign to keep
per-strategy surfaces in Redis/Fluxboard.

---

## Strategy + portfolio model (proposed)

### Terms

- **Flux Strategy ID**: the identifier used in Flux Redis keys (`flux:v1:*:{strategy_id}`).
- **Nautilus StrategyId**: the Strategy’s internal ID used for ownership, routing, and client order id tagging.
- **TokenMM portfolio**: the exchange account(s) backing the TokenMM operation; conceptually *one* portfolio for the 5 strategies.

### Naming conventions (recommended)

For each strategy instance:

- `identity.strategy_id` / `identity.strategy_instance_id` (Flux) should be unique and stable, e.g.:
  - `bybit_binance_PLUMEUSDT_makerv3`
- `strategy.external_strategy_id` should equal `identity.strategy_id` (avoid drift; Flux publishes/observability uses this).
- `strategy.strategy_id` (Nautilus StrategyId) should be unique and human-meaningful:
  - e.g. `MAKERV3-PLUMEUSDT` or `MAKERV3-001` (but prefer symbol tagging once there are multiple instances).

---

## Workflow: adding a new strategy instance (target UX)

### The “drop-in file” contract

1. Create a new TOML config file (copy a template) under a directory, e.g.:
   - `examples/live/makerv3/config/strategies.d/<strategy_id>.toml`
2. The file must set:
   - `[identity].strategy_id == [identity].strategy_instance_id == [identity].external_strategy_id`
   - unique `[strategy].strategy_id` (Nautilus StrategyId)
   - `maker_instrument_id` + `reference_instrument_id`
   - venue credentials via env indirection (as today)
3. Start/restart stack; the new strategy is discovered by:
   - Flux bridge (`--all-strategies`) via inbound stream scans
   - Flux API via params-key discovery (`/api/v1/params?profile=tokenmm`)

### Deployment orchestration change (recommended)

Create a TokenMM stack runner which:

- Starts N nodes (one per config file)
- Starts 1 bridge in `--all-strategies` mode
- Starts 1 API (+ Fluxboard)

Implementation likely lives in one of:

- New: `scripts/deploy/tokenmm_stack.sh`
- Or extend: `scripts/deploy/makerv3_stack.sh` with a `MAKERV3_STRATEGY_CONFIG_DIR` mode

---

## Fluxboard balances: make it portfolio-scoped (recommended design)

### Goal

`/tokenmm/balances` should show **the TokenMM portfolio**, not “balances for whichever strategy happened to be selected/resolved”.

### Recommended near-term fix (minimal churn): API aggregation for `profile=tokenmm`

Modify `nautilus_trader/flux/api/app.py` `GET /api/v1/balances` behavior:

- If `strategy` query param is present: keep exact behavior (per-strategy balances).
- Else if `profile=tokenmm`: load the full tokenmm strategy id set (same logic as `/api/v1/params`) and **merge** balances:
  - Cash/account balance rows: **dedupe** (key by `exchange + asset + account`) and pick the most recent `ts_ms`.
  - Position rows: **aggregate** net by `exchange + instrument_id` (or other stable instrument key).

This is robust even when each strategy node only publishes positions for its own instrument (the “instrument-scoped payload” issue).

### Optional follow-up: publisher completeness

Consider changing `nautilus_trader/flux/strategies/makerv3/publisher.py::publish_balances` to always include `positions_open()` (all instruments),
so any single strategy snapshot can be portfolio-complete *when the node cache is portfolio-complete*.

### Longer-term (cleanest conceptual model): explicit portfolio balances keyspace

Introduce a portfolio id (e.g. `tokenmm`) and store balances at:

- `flux:v1:portfolio:balances:snapshot:tokenmm`

…with one authoritative producer (dedicated portfolio publisher, or API-side aggregation persisted).

---

## Implementation plan (task breakdown)

### Task 1: Decide topology + document decision

**Deliverable:** decision record inside this file (and optionally a short ADR doc).

**Decision inputs:**
- Need atomic portfolio-level risk? (push toward single node)
- Need per-strategy Flux keyspaces without redesign? (push toward separate nodes)
- Acceptable blast radius of one process? (single node couples failures)

### Task 2: Add multi-strategy stack workflow

**Files:**
- Create: `scripts/deploy/tokenmm_stack.sh` (or extend `scripts/deploy/makerv3_stack.sh`)
- Create: `examples/live/makerv3/config/strategies.d/README.md` (directory contract + naming rules)
- Create: `examples/live/makerv3/config/strategies.d/<template>.toml` (template)

**Verification:**
- `scripts/deploy/tokenmm_stack.sh start` starts N nodes + bridge + api, and `status` reports each.
- `scripts/deploy/tokenmm_stack.sh health` passes:
  - `GET /api/v1/healthz`
  - `GET /tokenmm`
  - Socket.IO polling handshake

### Task 3: Implement portfolio-scoped balances for TokenMM

**Files:**
- Modify: `nautilus_trader/flux/api/app.py` (balances route profile-aware aggregation)
- Modify: `nautilus_trader/flux/api/payloads.py` (helpers to merge/dedupe balances rows + recompute totals)
- Test: `tests/unit_tests/flux/api/test_app.py` (new unit test covering aggregation)

**Verification:**
- `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1 pytest -q tests/unit_tests/flux/api --confcutdir=tests/unit_tests/flux/api`

### Task 4 (optional): Make balances publisher portfolio-complete

**Files:**
- Modify: `nautilus_trader/flux/strategies/makerv3/publisher.py` (publish all positions, not maker-instrument-only)
- Test: `tests/unit_tests/flux/strategies/makerv3/` (add/adjust targeted test if available)

### Task 5: Tighten “TokenMM strategy set” allowlisting (recommended if multiple unrelated strategies exist)

Right now `profile=tokenmm` can discover strategies by scanning for params keys. If Redis contains unrelated Flux strategies,
TokenMM UI may accidentally include them.

**Goal:** Make the TokenMM strategy set explicit and stable (still optionally allowing discovery in dev).

**Files (proposal):**
- Modify: `examples/live/makerv3/run_api.py` (read explicit allowlist / mapping from config)
- Modify: `nautilus_trader/flux/api/app.py` (inject `profile_strategy_map` / allowlist into `create_flux_api_app`)
- Test: `tests/unit_tests/flux/api/test_app.py` (profile allowlist/mapping coverage)

**Proposed config contract:**
- Add to TOML:
  - `[api].profile_strategy_ids_tokenmm = ["strategy_a", "strategy_b", "..."]`
- Behavior:
  - If list is present and non-empty, TokenMM uses *only* these IDs.
  - Else fallback to current “discover from params keys” behavior.

### Task 6: Decide multi-strategy surface expectations in Fluxboard (signals/trades/alerts)

Fluxboard currently calls:
- `GET /api/v1/params?profile=tokenmm` (already fans out to many strategies)
- `GET /api/v1/balances?profile=tokenmm` (should become portfolio-scoped via Task 3)
- `GET /api/v1/signals?profile=tokenmm` (today resolves to a single strategy)
- Socket.IO profile emitter (today resolves to a single strategy)

Decide which of these should be:
- **Per-strategy** (operator selects one strategy; API returns one), vs
- **Multi-strategy** (API returns/streams N strategies).

If TokenMM operators need to monitor all 5 strategies in one view, plan follow-ups:
- Extend `/api/v1/signals` to optionally fan out under `profile=tokenmm` (similar to `/api/v1/params`).
- Extend Socket.IO emitter to emit per-strategy deltas for the TokenMM allowlisted strategy set.

### Task 7: Ops guardrails for “multiple nodes on one account”

If we deploy one node per strategy while sharing an exchange account/portfolio:

- Ensure each strategy has a unique `StrategyId` / `order_id_tag` (prevents collisions).
- Keep cancellation boundaries strategy-owned (avoid cross-strategy cancel blasts).
- Review execution engine filtering settings depending on desired coupling:
  - `filter_unclaimed_external_orders`
  - `filter_position_reports`
  - (see `docs/concepts/live.md`)
- When querying orders to cancel, exclude `PENDING_CANCEL` to avoid duplicate cancels / state explosion:
  - see `docs/concepts/execution.md`

### Open questions (need explicit answers)

1. Do the 5 strategies share **one exchange account** (true shared portfolio), or separate accounts?
2. Do we need **portfolio-level risk limits** enforced atomically across strategies (global caps), or is per-strategy risk acceptable?
3. Should Fluxboard’s TokenMM “Signals” view show **all strategies** by default, or one selected strategy?
4. Do we want a first-class `portfolio_id` concept in Flux schema now, or keep portfolio aggregation in the API for TokenMM?
