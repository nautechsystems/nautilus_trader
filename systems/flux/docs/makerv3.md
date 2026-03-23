# Flux MakerV3 Strategy (`makerv3`)

This document describes the canonical Flux MakerV3 quoting strategy implementation and its production invariants.

Code location: `systems/flux/flux/strategies/makerv3/`.

## Overview

MakerV3 is a single-instrument quoting strategy which places and maintains a ladder of post-only maker orders on a configured
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

- `systems/flux/flux/strategies/makerv3/__init__.py` exports `MakerV3Strategy`, `MakerV3StrategyConfig`.
- `systems/flux/flux/strategies/__init__.py` re-exports the same canonical surfaces.

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

- `systems/flux/flux/common/params.py` (`MAKERV3_RUNTIME_PARAM_REGISTRY`)
- `systems/flux/flux/strategies/makerv3/runtime_params.py`

Operational expectations:

- Raw/reference FV remains the market-derived anchor used by the strategy.
- Signed skew and offsets do not mutate that raw/reference FV estimator; they adjust quoted FV / quote placement relative to the reference market.
- Signed skew convention is canonical across strategy, API, and UI:
  - positive values raise our quoted FV / quote richer,
  - negative values lower our quoted FV / quote cheaper,
  - short inventory relative to target should therefore produce positive skew,
  - `linear_offset_bps`, `global_skew_bps`, `local_skew_bps`, and `total_skew_bps` all use that convention.
- Unknown keys are rejected (fail-fast).
- Updates are coerced and applied atomically.
- Hot-path-sensitive params (for example depth) are bounded to HFT-safe limits.

Wiring:

- The strategy can be provided a manager instance via `set_params_manager(...)`.
- Or it can be provided a lazy factory via `set_params_manager_factory(...)` (preferred for runners that construct Redis
  clients at runtime).

Operator examples:

- `global_qty < des_qty_global` and `local_qty < des_qty_local` should produce positive `global_skew_bps` and `local_skew_bps`, so total skew is positive.
- `global_qty > des_qty_global` and `local_qty > des_qty_local` should produce negative skew components, so total skew is negative.
- `linear_offset_bps = +5` means we quote as if FV is `+5 bps` higher relative to the reference market.
- Signal should display the arithmetic contract `linear + global + local = total`.

Rollout checklist:

1. Pause or gate affected live TokenMM strategies before deploying the skew-semantics change.
2. In staging or replay, confirm short inventory displays positive skew and long inventory displays negative skew.
3. Confirm Signal tooltips read naturally for operators:
   - quoted FV shift
   - linear/global/local/total breakdown
   - actual bid edge / ask edge
4. Capture one before/after payload sample or screenshot for Lan sign-off.

## Topics and observability

MakerV3 publishes structured JSON payloads to canonical topics:

- `flux.makerv3.state`: state snapshots (including managed-order counts and optional pricing debug).
- `flux.makerv3.event`: structured events including quote-cycle envelopes.
- `flux.makerv3.alert`: actionable operator alerts (cooldown/transition gated).
- `flux.makerv3.market_bbo`: top-of-book snapshots (change-driven + heartbeat).
- `flux.makerv3.fv`: fair-value snapshots (midpoint of maker/reference when available).
- `flux.makerv3.order_intent`: per-order place/cancel intent payloads used to enrich persistent
  `order_action` and `execution_fill` rows.
- `flux.makerv3.trade`: order fill notices for downstream monitoring/analytics, including decision
  correlation fields when available. Bare `qty` remains the legacy venue-native execution size on
  this shared topic; explicit `qty_base`, `qty_venue`, `qty_conversion_status`, and
  `qty_conversion_source` fields carry normalized quantity context for consumers that need it.

Trade payload quantity contract:

- `qty` remains the shared venue-native fill quantity for backward compatibility on the producer topic.
- `qty_venue` duplicates that explicit venue-native quantity for consumers that want a named field.
- `qty_base` carries normalized base-asset exposure when instrument metadata allows conversion.
- `qty_conversion_status` and `qty_conversion_source` explain whether `qty_base` is exact, degraded, or unavailable.

MakerV3 telemetry is persisted across four surfaces:

- `quote_cycle` for every decision pass, including no-order cycles.
- `order_action` for actual lifecycle events enriched from `flux.makerv3.order_intent`.
- `execution_fill` for fills enriched with the same correlation metadata plus IB gateway
  send/receipt timestamps when available.
- `execution_markout` for derived 30s/60s/120s live-forward markouts vs `fv_market_mid`
  on TokenMM nodes.

Generic execution-pipeline timing is emitted on `events.execution.timing` for all live strategies
which traverse the standard `Strategy -> RiskEngine -> ExecutionEngine -> LiveExecutionClient`
path. `order_action` and `execution_fill` subscribe to that stream and persist:

- `ts_command_init_ns`
- `ts_risk_recv_ns`, `ts_risk_forward_ns`
- `ts_exec_recv_ns`, `ts_exec_forward_ns`
- `ts_client_submit_ns`
- `ts_adapter_submit_start_ns`

This gives MakerV3 a v1 latency breakdown without inventing a strategy-specific transport layer.
The strategy-specific surfaces remain:

- `quote_cycle` for no-order decisions and pricing audit
- `order_intent` for per-order reason/correlation metadata

Operational guidance:

- Keep high-volume quote-cycle, state, and pricing diagnostics on these structured topics.
- Use process logs for lifecycle events, guardrails, failures, and operator actions rather than repeating hot-path telemetry in text logs.
- See `docs/runbooks/makerv3-markouts.md` for the markouts operator workflow, join keys,
  and current scope limits.

Quote-cycle events use an envelope with:

- `run_id`: stable per-run identifier
- `quote_cycle_id`: `{run_id}:{quote_cycle_seq}`
- `quote_cycle_seq`: numeric cycle sequence
- `quote_cycle_event`: `skipped|blocked|completed`
- `reason_code`: machine-readable reason for operator/debugging
- `trigger_source`, `trigger_instrument_id`, `trigger_md_ts_event_ns`, `trigger_md_ts_init_ns`
- `ts_cycle_start_ns`, `ts_cycle_end_ns`
- `state_from`, `state_to`
- `cancel_count`, `place_count`
- optional `decision_context_json` for blocked/completed or action-taking cycles

Order-intent payloads carry:

- `intent_type`: `PLACE` or `CANCEL`
- `client_order_id`, `run_id`, `quote_cycle_id`, `reason_code`, `level_index`
- local decision timestamps such as `ts_decision_ns`, `ts_submit_local_ns`,
  `ts_cancel_request_local_ns`
- trigger timestamps `ts_market_data_event_ns` and `ts_market_data_recv_ns`

Clock-domain note:

- `ts_market_data_event_ns` comes from the triggering market-data event and is not assumed to be in
  the same clock domain as local strategy timestamps.
- `ts_market_data_recv_ns`, `ts_decision_ns`, `ts_submit_local_ns`, the generic execution timing
  fields, and the IB adapter callback timestamps are local-clock measurements intended for latency
  analysis.
- `ts_submit_gateway_send_ns` / `ts_cancel_gateway_send_ns` are best-effort local pre-dispatch
  timestamps taken immediately before the `ibapi` call, not confirmed gateway/network send
  receipts.
- Cancel commands currently bypass the generic risk stage on the standard live path, so
  `ts_risk_recv_ns` and `ts_risk_forward_ns` are expected to be `NULL` on cancel rows.

Safe v1 latency cuts for MakerV3 include:

- local tick receive -> decision: `ts_decision_ns - ts_market_data_recv_ns`
- local tick receive -> strategy submit: `ts_submit_local_ns - ts_market_data_recv_ns`
- command init -> risk/exec/client/adapter segments using the persisted `ts_*` execution fields
- local tick receive -> IB pre-dispatch: `ts_submit_gateway_send_ns - ts_market_data_recv_ns`
- IB pre-dispatch -> callback receipt: `ts_open_order_recv_ns - ts_submit_gateway_send_ns` or
  `ts_order_status_recv_ns - ts_submit_gateway_send_ns`
- IB pre-dispatch -> fill receipt: `ts_exec_details_recv_ns - ts_submit_gateway_send_ns`

Not safe in v1 without explicit clock normalization:

- `ts_market_data_event_ns` minus any local-clock field
- `execution_fill.ts_event` minus any local-clock field when treating `ts_event` as venue time

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
