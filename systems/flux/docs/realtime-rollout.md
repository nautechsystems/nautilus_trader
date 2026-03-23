# Flux Realtime Rollout Controls

This document describes the additive `contract_version=2` rollout controls implemented by
`systems/flux/flux/api/socketio.py` and `systems/flux/flux/api/app.py`.

## Scope

1. Legacy Socket.IO event names remain the default path.
2. The standard path is opt-in through:
   - HTTP snapshot request with `contract_version=2`
   - Socket.IO `subscribe` event carrying the returned lineage metadata
3. The backend `contract_version=2` path currently supports `signal` and `trades` surfaces.
4. The current frontend `Signal` and `Trades` panels use `subscribe` / `realtime_event` / `unsubscribe`
   in flag-on mode; legacy event listeners remain only for explicit local flag-off rollback.
5. `alerts`, `balances`, and `marketData` currently use the frontend realtime standard over the shared
   compatibility bridge, which still consumes legacy `market_update` invalidation traffic.
6. Signal standard recovery is intentionally `invalidate_only` under the current polling transport, while
   trades additionally supports bounded delta replay for contiguous gaps before falling back to
   `recovery_required`.
7. Signals only advertises realtime lineage for canonical profile-scoped snapshots whose request-selected
   strategy set exactly matches the subscribable stream identity for that profile.
8. Trades only advertises realtime lineage for the canonical unfiltered first-page descending snapshot.
9. Canonical trades snapshots must also resolve to a real subscribable descriptor for that normalized profile;
   profile-shaped REST fallbacks are not allowed to advertise `data.realtime` if `subscribe` would reject them.
10. Non-canonical trades queries stay REST-only and omit `data.realtime`.

## Active Surface Matrix

| Surface | Current live path | Recovery mode | Cleanup candidate |
| --- | --- | --- | --- |
| `signal` | frontend standard panel with backend `contract_version=2` snapshot lineage and standard Socket.IO subscription in flag-on mode | `invalidate_only` with lineage and explicit `recovery_required` | frontend duplicate-path cleanup yes; backend legacy-event cleanup no until rollback clients and bridge-backed surfaces are resolved |
| `trades` | frontend standard panel with backend `contract_version=2` snapshot lineage and standard Socket.IO subscription in the canonical live view | delta replay for contiguous gaps, otherwise `recovery_required` | frontend duplicate-path cleanup yes; backend legacy-event cleanup no until rollback clients and bridge-backed surfaces are resolved |
| `alerts` | frontend standard controller over compatibility bridge via legacy `market_update` | `invalidate_only` snapshot refresh | frontend duplicate-path cleanup yes; backend `market_update` removal no |
| `balances` | frontend standard controller over compatibility bridge via legacy `market_update` | `invalidate_only` snapshot refresh | frontend duplicate-path cleanup yes; backend `market_update` removal no |
| `marketData` | frontend standard controller over compatibility bridge via legacy `market_update` | `invalidate_only` snapshot refresh | frontend duplicate-path cleanup yes; backend `market_update` removal no |

## Rollout state

The Flask app exposes in-memory rollout controls at `app.extensions["flux_realtime_rollout"]`.

Default shape:

```python
{
    "supported_contract_versions": {2},
    "hard_kill_switch": False,
    "surface_enabled": {
        "signal": True,
        "trades": True,
    },
    "surface_canary_profiles": {
        "signal": None,
        "trades": None,
    },
}
```

Semantics:

1. `hard_kill_switch`
   - rejects new standard subscribes with `reason=backend_kill_switch`
   - ejects connected standard subscribers with `recovery_required`
   - does not affect legacy event names
2. `supported_contract_versions`
   - unsupported requests are rejected with `reason=unsupported_contract_version`
   - missing `surface_query_key`, `stream_id`, or `snapshot_revision` is rejected with `reason=missing_snapshot_lineage`
3. `surface_enabled[surface] = False`
   - rejects new subscribes with `reason=capability_unavailable`
   - ejects connected subscribers with `reason=capability_withdrawn`
4. `surface_canary_profiles[surface]`
   - `None` means allow all profiles
   - empty set means deny all profiles
   - non-empty set means allow only the listed normalized profiles
   - rejected subscribes use `reason=canary_denied`
   - mid-session denials use `reason=capability_withdrawn`

## Deterministic precedence

Subscribe-time decisions are evaluated in this order:

1. `hard_kill_switch`
2. `supported_contract_versions`
3. supported surface validation
4. `surface_enabled`
5. `surface_canary_profiles`

Mid-session withdrawal decisions are evaluated in this order:

1. `hard_kill_switch`
2. `surface_enabled`
3. `surface_canary_profiles`

This fails closed for the standard path while keeping legacy clients healthy.

## Heartbeat and liveness

Current liveness policy is derived from the polling interval:

1. transport mode: `polling_only`
2. heartbeat cadence: one heartbeat on any emitter poll that has no surface-relevant delta
3. heartbeat interval: `poll_interval_s * 1000` ms, clamped to at least `250ms`
4. tolerated heartbeat jitter: `250ms`
5. missed heartbeat threshold before stale: `2`
6. exposed capability string:
   - `heartbeat_or_data_within_<threshold>ms`

With the default `0.75s` poll interval this yields:

1. `heartbeat_interval_ms = 750`
2. liveness threshold `= 1500ms`

Heartbeats do not advance `last_seq`; only data or recovery events advance stream sequence.

Cursor semantics are surface-specific:

1. Standard stream cursors are keyed by `(profile, surface)`.
2. Legacy `market_update` / `signal_delta` / `trade_update` traffic does not advance standard cursors.
3. Signal standard traffic does not advance trades standard cursors, and trades standard traffic does not advance signal standard cursors.

## Recovery behavior

Current backend recovery behavior is surface-specific.

Implications:

1. `accepted_start_seq` in subscribe ack is the server-accepted surface-specific cursor after lineage validation.
2. Clients must compare their requested cursor with `accepted_start_seq`.
3. Clients must ignore stale subscribe acks from superseded reconnect attempts; only the latest in-flight subscribe attempt for a surface may mutate client state.
4. `surface_query_key`, `stream_id`, and `snapshot_revision` are mandatory subscribe inputs; the server does not silently infer missing lineage.
5. For signals and trades, `data.realtime.last_seq` is the standard surface-specific stream cursor, not a shared profile-wide counter.
6. For trades, `data.realtime.last_seq` is also distinct from the `/api/v1/trades` row cursor.
7. Signals currently use `invalidate_only`; any signal-side `recovery_required` event means the client must discard incremental merge state and fetch a fresh REST snapshot.
8. Signal clients must keep the standard cursor monotonic across `delta_batch`, `heartbeat`, and `invalidate` packets so reconnect resumes never regress during quiet or invalidate-only windows.
9. Trades first attempt bounded delta replay for contiguous gaps; trade scan overflow or unsupported gaps emit `recovery_required` with `reason=trade_gap`.
10. Snapshot responses that started before `manual_refresh_required` must not clear the fail-closed state on the client.

## Observability

The Flask app exposes in-memory counters at `app.extensions["flux_realtime_metrics"]`.

Tracked buckets:

1. `active_standard_subscribers`
   - keyed by `<surface>:v<contract_version>`
2. `standard_subscribe_counts`
   - keyed by accept/reject reason
3. `standard_recovery_required_counts`
   - keyed by recovery reason
4. `legacy_event_counts`
   - keyed by legacy Socket.IO event name

These counters are process-local and intended for rollout smoke tests and integration instrumentation.

Bridge-backed surfaces currently rely on mixed evidence during cleanup review:

1. `legacy_event_counts.market_update` remains the backend-side proof that compatibility traffic exists.
2. Surface-owned Playwright or Vitest rollout checks provide the client-observed request-count ceilings,
   recovery counts, and visible health-state transitions for `alerts`, `balances`, and `marketData`.
3. Cleanup review must not treat the absence of backend `contract_version=2` counters for those bridge
   surfaces as permission to remove `market_update`.

Task 14 also adds deterministic browser cutover evidence for the flag-on Signal and Trades routes:

1. `signal.spec.ts` proves `subscribe` carries signal lineage, legacy `market_update` / `signal_delta` traffic is
   ignored in flag-on steady state, `delta_batch` updates the live panel, reconnect preserves invalidate-only
   recovery, `invalidate` triggers exactly one additional recovery snapshot, and route teardown emits `unsubscribe`.
2. `trades.spec.ts` proves `subscribe` carries trades lineage, legacy `trade_update` traffic is ignored in flag-on
   steady state, `delta_batch` updates the live table, healthy steady state avoids parallel `/api/v1/trades/delta`
   replay, and `capability_withdrawn` stays fail-closed across reconnect while emitting `unsubscribe`.

## Cleanup Rehearsal Gate

The Task 12 frontend cleanup rehearsal is the mixed-surface Playwright soak:

```bash
E2E_BASE_URL=http://127.0.0.1:4173 pnpm exec playwright test -c playwright.smoke.config.ts e2e/realtime-soak.spec.ts
```

The rehearsal runs a fake-socket mixed rollout with:

1. `Signal`, `Trades`, `Alerts`, and `Balances` mounted together on `/dashboard`
2. `MarketData` exercised on `/market-data` in the same run
3. `200` signal rows and `200` market-data rows
4. `50` mixed `market_update` invalidations for dashboard surfaces
5. a trades cursor gap that must recover through exactly one delta replay
6. `50` `market_update` invalidations for `MarketData`

Pass criteria currently recorded by the rehearsal:

1. mounted dashboard rows stay within the committed `<=120` gate
2. `signal`, `alerts`, `balances`, and `marketData` stay within their bounded recovery request ceilings
3. `trades` issues exactly one delta replay request after the injected gap
4. the trades replay request preserves `since_seq`, `stream_id`, and `snapshot_revision`
5. the visible UI shows the recovered alert, balances refresh, trades replay row, and market-data refresh

Current recorded result on 2026-03-23 UTC: pass for the frontend cleanup rehearsal only.

This rehearsal is not the 7-day cleanup review window. It proves bounded mixed-surface behavior and
cleanup-boundary correctness under a deterministic harness. The live minimum canary cohort, active
standard-subscriber thresholds, minimum event-volume thresholds, and allowed legacy-traffic levels still
come from rollout dashboards plus the per-surface cutover packets during the cleanup review window.

Task 12 also requires machine-checked proof that the negotiated transport mode remains `polling_only`.
The current lane evidence for that requirement is the frontend compatibility matrix verification:

```bash
VITEST_FULL=1 pnpm exec vitest run __tests__/realtime/compatibility-matrix.test.tsx
```

That suite passed on 2026-03-23 UTC and explicitly pins `transportMode = polling_only` for the
current standard-path capability matrix.

The first red run of this gate exposed a real production-path gap: the standard `SignalTable` desktop path
was not wiring the shared virtualizer into `DataTable`, which caused dashboard mounted rows to exceed the
budget. Task 12 fixed that regression in `fluxboard/components/domain/signal/SignalTable.tsx`, then reran
the rehearsal green.

## Operational guidance

1. Start with legacy-only clients and confirm legacy event counts remain stable.
2. Enable a small canary profile cohort by setting `surface_canary_profiles`.
3. Verify:
   - standard subscribe acks are accepted
   - active subscriber counts increase only for the intended surface
   - no unexpected `recovery_required` reasons appear
4. Rehearse `hard_kill_switch=True` and confirm:
   - new standard subscribes fail with `backend_kill_switch`
   - connected standard clients receive `recovery_required`
   - legacy clients continue receiving legacy events
5. Use the mixed-surface cleanup rehearsal before removing duplicate frontend live paths.
6. Do not remove backend `market_update`, `signal_delta`, or `trade_update` traffic solely because the
   frontend cleanup rehearsal passed; backend legacy-event removal is only valid for surfaces that no longer
   rely on legacy frontend subscriptions or the compatibility bridge.
