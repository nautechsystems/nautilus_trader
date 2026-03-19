# Flux Realtime Rollout Controls

This document describes the additive `contract_version=2` rollout controls implemented by
`systems/flux/flux/api/socketio.py` and `systems/flux/flux/api/app.py`.

## Scope

1. Legacy Socket.IO event names remain the default path.
2. The standard path is opt-in through:
   - HTTP snapshot request with `contract_version=2`
   - Socket.IO `subscribe` event carrying the returned lineage metadata
3. The standard path currently supports `signal` and `trades` surfaces.
4. The standard path is intentionally `invalidate_only` under the current polling transport.
5. Trades only advertises realtime lineage for the canonical unfiltered first-page descending snapshot.
   Non-canonical trades queries stay REST-only and omit `data.realtime`.

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

## Recovery behavior

Current recovery mode is `invalidate_only`.

Implications:

1. `accepted_start_seq` in subscribe ack is the server-accepted cursor after lineage validation.
2. Clients must compare their requested cursor with `accepted_start_seq`.
3. `surface_query_key`, `stream_id`, and `snapshot_revision` are mandatory subscribe inputs; the server does not silently infer missing lineage.
4. For trades, `data.realtime.last_seq` is the standard stream cursor, not the `/api/v1/trades` row cursor.
5. Any `recovery_required` event means the client must discard incremental merge state and fetch a fresh REST snapshot.
6. Trade scan overflow or cursor gaps emit `recovery_required` with `reason=trade_gap`.

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
