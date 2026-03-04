# Flux API Contract (`flux:v1`)

This document defines the production API contract implemented by `nautilus_trader/flux/api/app.py`.

## Scope and invariants

1. API is created via `create_flux_api_app(FluxConfig, redis_client, ...)`.
2. All key lookups are strategy-scoped and schema-scoped (`flux:v1:*`).
3. API responses use a consistent envelope with explicit `ok`, `api_version`, `request_id`, and `timestamp_ms`.
4. Readiness and health checks are dependency-based (Redis availability + required key checks).
5. No runtime compatibility mode exists for legacy non-`flux:v1` keyspaces.

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
| `/api/v1/params` | `GET` | Strategy parameter snapshot |
| `/api/v1/params` | `POST`, `PATCH` | Strategy parameter update (query-specified strategy) |
| `/api/v1/signals` | `GET` | Combined strategy state/signals payload |
| `/api/v1/strategies` | `GET` | Strategy payload list (single strategy scope today) |
| `/api/v1/strategies/{strategy_id}/parameters` | `GET` | Path-scoped parameter snapshot |
| `/api/v1/strategies/{strategy_id}/parameters` | `POST`, `PATCH` | Path-scoped parameter update |
| `/api/v1/balances` | `GET` | Balances rows |
| `/api/v1/trades` | `GET` | Trades rows |
| `/api/v1/trades/delta` | `GET` | Trades rows filtered by `after` timestamp |
| `/api/v1/alerts` | `GET` | Alerts rows |

Pagination/limits:

1. `limit` is clamped to `1..200` for row-list endpoints.
2. Trades delta uses `after` parsed to `ts_ms`.

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
2. Redis connectivity/errors return `503 store_unavailable`.
3. Invalid params payloads return `400 invalid_params_update` or `400 missing_payload`.
4. Unexpected exceptions return `500 internal_error`.

The API does not silently substitute legacy keys or default unknown strategy identifiers.

## Verification

```bash
scripts/ci/check-flux-leakage.sh
```
