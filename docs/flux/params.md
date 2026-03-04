# Flux Runtime Parameters (`flux:v1`)

This document defines the production runtime-parameter contract used by Flux strategy, bridge, and API components.

## Scope and invariants

1. Authoritative parameter storage is strategy-scoped hash key `flux:v1:params:{strategy_id}`.
2. Parameter change notification uses pub/sub channels `flux:v1:params:{strategy_id}` and `flux:v1:params:global`.
3. The strategy-scoped hash key and strategy-scoped pub/sub channel intentionally share the same Redis address.
4. Unknown parameter names are rejected (fail-fast) by `FluxParamsManager`.
5. Missing hash fields are resolved from in-process defaults; no silent unknown-field fallback is permitted.

## Data model

Parameter schema entries are keyed by parameter name and include at least a `type` field.

Supported types:

1. `boolean`
2. `integer`
3. `number`
4. `string`-like fallback (any type not listed above is treated as decoded text)

The manager coercion path is strict:

1. Invalid `boolean` text raises `ValueError`.
2. Invalid `integer` values raise `ValueError`.
3. Invalid `number` values raise `ValueError`.
4. Unknown parameter names raise `ValueError`.

Boolean accepted values:

1. True: `1`, `true`, `t`, `yes`, `y`, `on`, `enabled`
2. False: `0`, `false`, `f`, `no`, `n`, `off`, `disabled`

## Read/write contract

Read path (`FluxParamsManager.load`):

1. Validate Redis hash keys against schema via `HKEYS`; fail if unknown fields exist.
2. Read all schema fields in one `HMGET` call.
3. Require `HMGET` response length to match requested field count; otherwise raise `RuntimeError`.
4. Coerce each field value according to schema; if value is `None`, fall back to configured defaults.

Write path (`FluxParamsManager.update` + `publish_update`):

1. Coerce update payload strictly by schema.
2. Persist with one `HSET` mapping write to `flux:v1:params:{strategy_id}`.
3. Publish change envelope to both:
   - `flux:v1:params:global`
   - `flux:v1:params:{strategy_id}`
4. Publish envelope shape:
   - `strategy_id`: string
   - `updates`: object map of coerced values
   - `ts_ms`: integer Unix milliseconds (UTC)

Writers MUST update hash first and publish second. Pub/sub is notification transport only and is not authoritative state.

## API-facing behavior

`nautilus_trader/flux/api/app.py` exposes parameter endpoints that use the same schema/defaults and `FluxParamsManager` logic:

1. `GET /api/v1/param-schema`
2. `GET /api/v1/params`
3. `POST|PATCH /api/v1/params`
4. `GET /api/v1/strategies/{strategy_id}/parameters`
5. `POST|PATCH /api/v1/strategies/{strategy_id}/parameters`

All endpoints are strategy-scoped; when the strategy query/path argument is omitted, the default is `FluxConfig.identity.strategy_id`.

## Failure policy

1. Invalid or unknown parameter updates return explicit API validation errors.
2. Invalid stored hashes (for example unknown keys) surface as `params_store_invalid` and are not auto-healed by the API.
3. No legacy keyspace read/write path exists in production modules.

## Verification

Run local leakage and contract guardrails:

```bash
scripts/ci/check-flux-leakage.sh
```
