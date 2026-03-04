# Flux MakerV3 Strategy (`makerv3`)

This document describes the canonical Flux MakerV3 quoting strategy implementation and its production invariants.

Code location: `nautilus_trader/flux/strategies/makerv3/`.

## Overview

MakerV3 is a single-leg quoting strategy which places and maintains a ladder of post-only maker orders on a configured
maker instrument, using a reference instrument for fair-value anchoring. The strategy is config-driven and supports
bounded runtime parameter updates via the Flux params subsystem.

## Architecture (module split)

The strategy is intentionally organized as a thin orchestrator plus focused helper modules:

- `strategy.py`: `MakerV3Strategy` + `MakerV3StrategyConfig` orchestration and Nautilus handler integration.
- `quote_engine.py`: quote-cycle refresh logic and stale-data safety gates.
- `market_data.py`: order book delta handling, BBO tracking, and FV publishing triggers.
- `pricing.py`: pure pricing helpers (tick rounding, ladder construction, unique-price nudging).
- `rebalancing.py`: pure rebalancing planning (cancel/keep decisions per side).
- `managed_orders.py`: managed-order collection/registration/reconciliation and cancellation safety invariants.
- `inventory.py`: inventory extraction + skew calculation and TTL caching.
- `runtime_params.py`: runtime param schema/wiring, coercion, bounded updates, and manager factory hooks.
- `publisher.py`: JSON/event/state/balances/alert publishing helpers.
- `wire.py`: quote-cycle envelope/builders.
- `constants.py`: canonical topic names and reason/event constants.

Canonical exports:

- `nautilus_trader/flux/strategies/makerv3/__init__.py` exports `MakerV3Strategy`, `MakerV3StrategyConfig`.
- `nautilus_trader/flux/strategies/__init__.py` re-exports the same canonical surfaces.

## Safety invariants (must hold)

1. **Cancellation boundary:** the strategy cancels only its own managed orders by default.
   - Instrument-wide `cancel_all_orders(...)` is behind explicit opt-in (`cancel_all_instrument_orders=False` by default).
2. **Stale-data behavior:** stale or unavailable market data triggers one safety cancel + blocked transition per episode.
   - Stale cancel is cooldown-deduped to avoid cancel bursts.
3. **Determinism and idempotency:** repeated quote cycles with unchanged inputs should not generate repeated churn.
4. **Runtime params bounds:** depth/ladder-sensitive params are validated and bounded; unsafe updates are rejected.
5. **Stop behavior:** `on_stop` converges to quiescence by canceling managed orders and is idempotent.

## Runtime parameters

MakerV3 runtime params are backed by the canonical registry:

- `nautilus_trader/flux/common/params.py` (`MAKERV3_RUNTIME_PARAM_REGISTRY`)
- `nautilus_trader/flux/strategies/makerv3/runtime_params.py`

Operational expectations:

- Unknown keys are rejected (fail-fast).
- Updates are coerced and applied atomically.
- Hot-path-sensitive params (for example depth) are bounded to HFT-safe limits.

Wiring:

- The strategy can be provided a manager instance via `set_params_manager(...)`.
- Or it can be provided a lazy factory via `set_params_manager_factory(...)` (preferred for runners that construct Redis
  clients at runtime).

## Topics and observability

MakerV3 publishes structured JSON payloads to canonical topics:

- `flux.makerv3.state`: state snapshots (including managed-order counts and optional pricing debug).
- `flux.makerv3.event`: structured events including quote-cycle envelopes.
- `flux.makerv3.alert`: actionable operator alerts (cooldown/transition gated).
- `flux.makerv3.market_bbo`: top-of-book snapshots (change-driven + heartbeat).
- `flux.makerv3.fv`: fair-value snapshots (midpoint of maker/reference when available).
- `flux.makerv3.trade`: order fill notices (for downstream monitoring/analytics).

Quote-cycle events use an envelope with:

- `run_id`: stable per-run identifier
- `quote_cycle_id`: monotonically increasing per run
- `quote_cycle_event`: `skipped|blocked|completed`
- `reason_code`: machine-readable reason for operator/debugging

## Ops playbook (quick)

1. **Strategy stays blocked (`blocked_maker_md` / `blocked_reference_md`)**
   - Confirm market data subscriptions for both maker and reference instruments.
   - Check `max_age_ms` runtime parameter and venue feed health.
2. **Unexpected cancellation scope**
   - Verify `cancel_all_instrument_orders` is not enabled unless explicitly intended.
3. **Quote churn**
   - Check requote throttle (`INTERNAL_REQUOTE_THROTTLE_MS`) and runtime ladder params.
   - Ensure reference feed is not intermittently stale (would cause repeated block episodes).
4. **Runtime params not taking effect**
   - Confirm a params manager is wired (manager instance or factory) and strategy identity matches Redis keyspace.

