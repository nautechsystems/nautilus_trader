# MakerV3 Strategy Refactor — External Review Summary (2026-03-04)

## Scope

This change set productionizes the flux MakerV3 strategy by completing the strategy-only refactor plan in
`docs/plans/2026-03-04-flux-makerv3-strategy-refactor.md`:

- safety hardening (cancel boundaries, stale-loop suppression, deterministic state transitions),
- runtime params correctness (canonical schema, identity wiring, bounded updates),
- observability/perf upgrades (quote-cycle envelopes, alert rate limiting, hot-path tightening),
- modularization and canonical strategy surface migration to `makerv3`,
- example strategy deduplication into a thin wrapper.

## Key outcomes

### 1) Safety and runtime correctness

- Managed-order cancellation defaults to strategy-owned orders only.
- Optional instrument-wide cancel remains explicit, guarded, and event-accounted.
- Stale-data handling is deduped/cooldown-gated and avoids cancel bursts.
- Runtime params are registry-driven, bounded, and wired through manager/factory paths.
- Runtime strategy identity is consistent across payloads and params ownership.

### 2) Canonical `makerv3` surface

- Canonical implementation now lives in `systems/flux/flux/strategies/makerv3/strategy.py`.
- Canonical exports are `MakerV3Strategy` and `MakerV3StrategyConfig` from:
  - `systems/flux/flux/strategies/makerv3/__init__.py`
  - `systems/flux/flux/strategies/__init__.py`
- Legacy module `systems/flux/flux/strategies/makerv3/single_leg_quoter.py` has been removed.
- Canonical strategy topics are now `flux.makerv3.*` only.

### 3) Modularization

The strategy has been split into focused modules:

- `quote_engine.py` (quote-cycle refresh + stale-data gating),
- `market_data.py` (order book deltas/BBO + FV publish triggers),
- `pricing.py` (ladder math, tick/price helpers, skew edge adjustment),
- `rebalancing.py` (side rebalance planning),
- `inventory.py` (inventory extraction + skew computation + TTL cache),
- `managed_orders.py` (managed order collection/reconcile/cancel invariants),
- `runtime_params.py` (runtime schema/wiring + bounded updates),
- `publisher.py` (publish helpers for state/events/alerts/balances),
- `wire.py` (quote-cycle envelope builders),
- `constants.py` (topic + reason/event constants).

### 4) Observability and performance

- Structured quote-cycle envelopes include `run_id`, `quote_cycle_id`, `quote_cycle_event`, and `reason_code`.
- Quote-cycle events emitted for skipped, blocked, and completed cycles with counts/context.
- Actionable alerts are cooldown/transition gated to suppress noise.
- Hot-path avoids per-delta string churn and reduces repeated managed-order scans.

### 5) Example strategy deduplication

- `nautilus_trader/examples/strategies/makerv3.py` is now a thin wrapper over canonical
  strategy exports with no embedded strategy logic.

## Additional hardening completed during review

- Guarded instrument-wide cancel-all exceptions without aborting cancel flows.
- Timer now enforces stale market-data blocks/cancels even when deltas go silent (feed stall protection).
- Managed-order tracking is no longer cleared on transient empty cache snapshots (prevents orphaned open orders).
- Pricing and rebalancing helpers now reject non-finite numerics (NaN/Infinity) early.
- Restart safety: `on_start` resets failure/blocked latches and rejects identical maker/reference instrument IDs.
- Made runtime `qty` application atomic and reject non-positive updates to avoid stale effective qty.
- Aligned params manager factory defaults with in-strategy runtime defaults to prevent first-refresh drift.
- Tightened public module/class/function docstring coverage on new MakerV3 strategy modules and exports.

## Verification summary

All commands below were run with `PYTEST_DISABLE_PLUGIN_AUTOLOAD=1`.

- `pytest tests/unit_tests/flux/strategies/makerv3 -q` → `63 passed`
- `pytest tests/unit_tests/flux/strategies/makerv3 -q` → `83 passed`
- `pytest tests/unit_tests/examples/strategies/test_makerv3_quoter.py -q` → `4 passed`
- `pytest tests/unit_tests/flux/common/test_params.py -q` → `18 passed`
- `pytest tests/unit_tests/flux/params -q` → `25 passed`
- `pytest tests/unit_tests/flux/api/test_app.py -q` → `16 passed`
- `ruff check --select D ...` (MakerV3 strategy modules + exports touched in this refactor) → `All checks passed`

## Remaining explicit plan items

All plan checklist items are now complete:

- fill reconciliation/tracking determinism follow-up completed (managed tracking reconciles on fills without cache timing),
- strategy architecture/invariants/ops-playbook documentation added at `docs/flux/makerv3.md`.
