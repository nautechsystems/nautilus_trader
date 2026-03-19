# Flux API Contract (`flux:v1`)

This document defines the production API contract implemented by `systems/flux/flux/api/app.py`.

## Scope and invariants

1. API is created via `create_flux_api_app(FluxConfig, redis_client, ...)`.
2. All key lookups are strategy-scoped and schema-scoped (`flux:v1:*`).
3. API responses use a consistent envelope with explicit `ok`, `api_version`, `request_id`, and `timestamp_ms`.
4. Readiness and health checks are dependency-based (Redis availability + required key checks).
5. No runtime compatibility mode exists for legacy non-`flux:v1` keyspaces.
6. Production runner `flux.runners.tokenmm.run_api` binds to `127.0.0.1` by default; external bind
   addresses must be explicitly requested via CLI `--host` or config `[api].host` (CLI wins).
7. Signals legs are keyed by `contract_id` (`{exchange}:{symbol}`) and include `legs_order`.

## Network exposure (security)

1. The production runner module and TokenMM static-serving mode are intended for localhost/internal deployments.
2. The API is unauthenticated by default. Do not expose it directly to the public internet without adding
   authentication/authorization and TLS.
3. Prefer binding to loopback and accessing via VPN/SSH tunnel; if you must bind externally, front it with a secure
   edge (TLS + auth + IP allowlist).
4. `/api/pulse/*` is privileged operational control. Treat it as higher risk than read-only market-data routes.

## Request/response envelope

Success:

1. `ok: true`
2. `api_version: "v1"`
3. `request_id: <string>`
4. `timestamp_ms: <int>`
5. `data: <payload>`

Error:

1. `ok: false`
2. `api_version: "v1"`
3. `request_id: <string>`
4. `timestamp_ms: <int>`
5. `data: null`
6. `error: { code, message, details? }`

## Strategy scoping

1. Every route that reads strategy data resolves strategy ID from query/path.
2. If omitted, strategy ID defaults to `FluxConfig.identity.strategy_id`.
3. Strategy IDs are identifier-validated; invalid values return `400 invalid_strategy_id`.
4. `FluxIdentityConfig` enforces `strategy_instance_id == strategy_id`; API scoping is keyed only by `strategy_id` (no dual-identifier mode).

## Legs keying

1. Signal payload `legs` maps are keyed by `contract_id = "{exchange}:{symbol}"`.
2. `exchange` is normalized lowercase and `symbol` is normalized uppercase before key construction.
3. Each leg row still carries explicit `exchange` and `symbol` fields.

## Endpoints

| Route | Method | Purpose |
| --- | --- | --- |
| `/` | `GET` | Service metadata (`service`, `schema_prefix`) |
| `/api/v1/healthz` | `GET` | Redis availability and readiness snapshot |
| `/api/v1/readyz` | `GET` | Strict readiness gate (all required keys present) |
| `/api/v1/param-schema` | `GET` | Ordered runtime-parameter schema |
| `/api/v1/params` | `GET` | Strategy parameter snapshot array |
| `/api/v1/params` | `POST`, `PATCH` | Bulk/legacy strategy parameter update |
| `/api/v1/signals` | `GET` | Combined strategy state/signals payload |
| `/api/v1/strategies` | `GET` | Strategy payload list (single strategy scope today) |
| `/api/v1/strategies/{strategy_id}/parameters` | `GET` | Path-scoped parameter snapshot |
| `/api/v1/strategies/{strategy_id}/parameters` | `POST`, `PATCH` | Path-scoped parameter update |
| `/api/v1/balances` | `GET` | Balances rows |
| `/api/v1/trades` | `GET` | Trades rows |
| `/api/v1/trades/delta` | `GET` | Trades delta rows (`since_seq` preferred, `after` fallback) |
| `/api/v1/alerts` | `GET` | Alerts rows |
| `/api/v1/alerts` | `DELETE` | Clear alerts for strategy |

## Pulse control-plane endpoints

The TokenMM production runner can also mount a Pulse process-control API under `/api/pulse`. These routes are not part
of the `flux:v1` market-data contract and do not use the standard `ok/api_version/data` envelope.

| Route | Method | Purpose |
| --- | --- | --- |
| `/api/pulse/jobs` | `GET` | List enrolled jobs with grouped status, journald error summary (`count`, `last_seen`, `preview`), and unit metadata |
| `/api/pulse/jobs/{job_id}/start` | `POST` | Start one managed `flux@{job_id}` unit |
| `/api/pulse/jobs/{job_id}/stop` | `POST` | Stop one managed `flux@{job_id}` unit |
| `/api/pulse/jobs/{job_id}/restart` | `POST` | Restart one managed `flux@{job_id}` unit |
| `/api/pulse/jobs/{job_id}/logs` | `GET` | Return recent raw `journalctl` text (`?lines=300` default) |
| `/api/pulse/jobs/group/{group_key}/start` | `POST` | Start every enrolled job in a group |
| `/api/pulse/jobs/group/{group_key}/stop` | `POST` | Stop every enrolled job in a group |
| `/api/pulse/jobs/group/{group_key}/restart` | `POST` | Restart every enrolled job in a group |

Discovery invariants:

1. Pulse discovers jobs from `/etc/flux/*.env` by default.
2. Only env files with `PULSE_ENABLED=1` are listed or controllable.
3. Unit names are derived as `flux@{job_id}` unless `PULSE_UNIT_PREFIX` overrides the prefix.
4. `PULSE_SELF_SERVICE_ID` enables deferred self-stop/self-restart handling for the serving API unit.
5. For TokenMM production, `/api/pulse/*` is the supported service-management surface; `tokenmm_stack.sh` is
   local smoke only.

Pulse payload notes:

1. `/api/pulse/jobs` surfaces `errors.count`, `errors.last_seen`, and `errors.preview` from the current job summary window.
2. `errors.last_seen` is best-effort and populated only when the summary log format exposes a parseable timestamp.
3. `/api/pulse/jobs/{job_id}/logs` remains a raw text response; Pulse UI severity filters are applied client-side on top of that text, not via a separate structured logs API.

Pulse jobs/logs invariants:

1. `GET /api/pulse/jobs` includes per-job `errors` metadata with `count`, `preview`, and `last_seen`.
2. `errors.preview` is the newest matching error-like message extracted from the recent journal window.
3. `errors.last_seen` is populated when the summary journal output carries a parseable timestamp; otherwise it may be `null`.
4. `GET /api/pulse/jobs/{job_id}/logs` remains a raw text `journalctl` response. Severity filtering in the Pulse modal is a UI behavior layered on top of that raw output, not a separate structured logs API contract.

Pagination/limits:

1. `limit` is clamped to `1..200` for row-list endpoints.
2. `offset` is supported for `/api/v1/trades` and `/api/v1/alerts` (default `0`).
3. Trades delta accepts `since_seq`; `after` remains supported as fallback.
4. For paginated list endpoints, `total` is computed from the full strategy-filtered candidate set.

## TokenMM compatibility payloads

`profile` is accepted for TokenMM client compatibility and normalized consistently with Socket.IO, but strategy
scoping is still strategy-driven (default strategy unless an explicit `strategy` query is provided).

Compatibility is request/response-shape compatibility only; storage remains strict `flux:v1` with no legacy keyspace reads.

1. `GET /api/v1/param-schema` returns:
   - `data.params` (schema map)
   - `data.deprecated` (always an object, empty when no deprecated fields)
2. `GET /api/v1/params` returns `data` as an **array** of strategy rows.
3. `PATCH /api/v1/params` supports:
   - bulk payload: `{updates:[{strategy_id, params}], source}`
   - legacy payload: `{params:{...}, source}` (+ optional `?strategy=...`)
   - bulk items require non-empty `strategy_id`; invalid/missing IDs are returned as per-item errors
   - response: `{success, failed, errors}`
4. `GET /api/v1/trades` returns at least:
   - `{rows, total, limit, offset, has_more}`
   - optional: `{next_offset, last_seq, sort}`
5. `GET /api/v1/trades/delta` returns:
   - `{rows, last_seq, reset_required}`
   - when bounded scan cannot safely serve all rows after `since_seq`, returns `reset_required: true`
   - in `since_seq` mode when `reset_required: false`, rows are ordered oldest-to-newest and `last_seq` equals the last returned row
6. `GET /api/v1/alerts` returns stable pagination shape:
   - `{rows, total, limit, offset, has_more}` (+ optional `next_offset`)
7. `DELETE /api/v1/alerts` returns:
   - `{success, strategy_id, deleted, remaining, server_ts_ms}`
8. `GET /api/v1/signals?contract_version=2` adds `data.realtime` only for the canonical profile-scoped
   snapshot shape:
   - normalized `profile` present
   - no explicit `strategy`
   - request-selected strategy set resolves to the same stream identity validated by `subscribe`
9. Canonical signals snapshots keep the legacy payload shape and add `data.realtime` with:
   - `contract_version`
   - `surface`
   - `profile`
   - `surface_query_key`
   - `stream_id`
   - `snapshot_revision`
   - `last_seq`
   - `capabilities`
10. Strategy-scoped or otherwise non-canonical signals queries remain REST-only and omit `data.realtime`.
11. `GET /api/v1/trades?contract_version=2` adds `data.realtime` only for the canonical live-compatible
   snapshot shape:
   - normalized `profile` present
   - no explicit `strategy`
   - no filters
   - first page (`offset=0`)
   - descending/default sort (`ts_ms_desc`)
   - default page size (`limit=50`, explicit or implicit)
12. Non-canonical trades queries remain REST-only and omit `data.realtime`.
13. In `data.realtime`, `last_seq` is the standard stream cursor for subscribe lineage, not the REST row-sequence value.
14. `contract_version` is opt-in only. Unsupported values return `400 unsupported_contract_version`.

## Socket.IO contract (`/socket.io`)

Socket integration is exposed from `create_flux_api_app(...)` and attached to Flask
extensions:

1. `app.extensions["flux_socketio"]` (`flask_socketio.SocketIO`)
2. `app.extensions["flux_socket_server"]` (composed server object with `socketio` + `emitter`)
3. `app.extensions["flux_socketio_server"]` (underlying server instance)
4. `app.extensions["flux_socket_emitter"]` (polling emitter)

Connection/profile rules:

1. Socket path is `/socket.io`.
2. Server is configured for polling-only behavior (`allow_upgrades=False`), so websocket upgrade is disabled.
3. Client should use polling transport and emit `set_profile` after connect.
4. Profile normalization maps `tokenm` and `tokenmm` to `tokenmm`.
5. Room model is `profile:<normalized_profile>`.
6. `set_profile` leaves the previous room and joins the new room for the same client SID.
7. Clearing profile via `set_profile` unset payload returns ack with `profile: ""` and `room: null`.
8. Unsupported profiles return `ok: false` with `error.code: "unsupported_profile"` and no room join.

Legacy event names remain the default path and are unchanged:

Events (names are exact and stable):

1. `market_update`
2. `signal_delta`
3. `trade_update`

Payload invariants:

1. All events include `profile`, `seq`, `server_ts_ms`.
2. `signal_delta` includes `strategy_id` and `patch`.
3. `trade_update` includes `strategy_id`, `op`, `row_id`, `version`, and `trade` (`null` for delete).
4. `market_update` includes `strategies.changed` and `alerts` summary fields.
5. `row_id` is an opaque stable identifier; server may synthesize a fallback using the backing stream `entry_id`
   when producer rows are missing an explicit `row_id`.

Signal patch semantics:

1. Missing patch field means no change.
2. Explicit `null` means delete.
3. `patch.legs` is keyed by `contract_id`; removed legs are emitted as `null`.

Recovery:

1. REST remains authoritative for cold-start and reconnect recovery.
2. Clients must tolerate dropped/stale socket events and rehydrate from:
   - `GET /api/v1/signals`
   - `GET /api/v1/trades/delta`
   - `GET /api/v1/alerts`

### Standard realtime contract (`contract_version=2`)

The standardized contract is additive and opt-in. Legacy clients continue receiving only the legacy event names
unless they explicitly subscribe to the standard path.

HTTP snapshot handshake:

1. Client fetches a canonical live-compatible snapshot with `contract_version=2`.
2. Snapshot response adds `data.realtime = {contract_version, surface, profile, surface_query_key, stream_id, snapshot_revision, last_seq, capabilities}`.
3. Client subscribes with Socket.IO event `subscribe`.

`subscribe` request payload:

1. `contract_version`
2. `surface`
3. `profile`
4. `surface_query_key`
5. `stream_id`
6. `snapshot_revision`
7. `resume_from_seq`

All three lineage fields are mandatory and must be echoed from the snapshot:

1. `surface_query_key` must be non-empty
2. `stream_id` must be non-empty
3. `snapshot_revision` must be non-null

`subscribe` ack payload:

1. `accepted`
2. `contract_version`
3. `surface`
4. `profile`
5. `surface_query_key`
6. `stream_id`
7. `snapshot_revision`
8. `accepted_start_seq`
9. `last_seq`
10. `capabilities`
11. rejection-only: `reason`

Standard live event name:

1. `realtime_event`

Standard live envelope invariants:

1. Every event includes `contract_version`, `surface`, `stream_id`, `profile`, `kind`, `seq`, `snapshot_revision`, and `server_ts_ms`.
2. Supported `kind` values are:
   - `delta_batch`
   - `heartbeat`
   - `recovery_required`
3. `delta_batch` carries machine-readable `payload` content:
   - `signal` surface: `payload.signals[]`, `payload.alerts`, `payload.strategies.changed`
   - `trades` surface: `payload.trades[]`
4. `heartbeat` may carry an empty payload and does not advance `last_seq`.
5. `recovery_required` includes machine-readable `reason`.

Standard rejection and recovery reasons:

1. Subscribe rejection may return:
   - `backend_kill_switch`
   - `unsupported_contract_version`
   - `unsupported_surface`
   - `unsupported_profile`
   - `capability_unavailable`
   - `canary_denied`
   - `missing_snapshot_lineage`
   - `stream_rollover`
   - `snapshot_revision_mismatch`
   - `surface_query_key_mismatch`
2. Mid-session withdrawal emits `recovery_required` with:
   - `backend_kill_switch`
   - `capability_withdrawn`
3. Trade overflow/gap emits `recovery_required` with:
   - `trade_gap`

Current standard capabilities:

1. `capabilities.recovery_mode` is `invalidate_only`.
2. `capabilities.replay_supported` is `false`.
3. `capabilities.transport_mode` is `polling_only`.
4. Clients must resnapshot over REST after any `recovery_required`.

## Readiness policy

Default required keys:

1. `flux:v1:state:{strategy_id}`
2. `flux:v1:params:{strategy_id}`
3. `flux:v1:balances:snapshot:{strategy_id}`
4. `flux:v1:fv:stream:{strategy_id}`

`/api/v1/readyz` returns `503 service_not_ready` until all required keys exist.

## Error policy

Known error classes:

1. Config/contract validation errors return explicit envelope codes.
2. Redis connectivity/errors return `503 store_unavailable` with sanitized messages.
3. Invalid params payloads return `400 invalid_params_update` or `400 missing_payload`.
4. Unexpected exceptions return `500 internal_error` with sanitized messages and safe details.

The API does not silently substitute legacy keys or default unknown strategy identifiers.

## Verification

```bash
tooling/ci/check-flux-leakage.sh
```
