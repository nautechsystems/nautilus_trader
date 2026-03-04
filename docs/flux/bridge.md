# Flux Bridge Contract (`flux:v1`)

This document defines the production bridge ingestion and write contract for Flux.

## Scope and invariants

1. Bridge input stream keys are `flux:v1:in:stream:{environment}:{strategy_id}:{topic}`.
2. Bridge output keys are strategy-scoped under `flux:v1:*`.
3. `ts_ms` (Unix milliseconds, UTC) is mandatory for persisted rows; bridge normalizes timestamps at ingest.
4. High-churn outputs are bounded with stream `MAXLEN` retention.
5. There is no runtime legacy ingest/read path for non-`flux:v1` schemas.

## Inbound stream contract

Accepted stream key shape:

1. Namespace: `flux`
2. Schema version: `v1`
3. Domain: `in:stream`
4. Environment: `paper | testnet | live` (configured)
5. Strategy scope: required `{strategy_id}`
6. Topic: one of `state`, `event`, `trade`, `alert`, `market_bbo`, `fv`, `balances`

The consumer rejects keys that do not match the configured namespace/schema/environment and topic set.

## Payload formats

Bridge accepts either:

1. Direct JSON payload rows/lists.
2. `FluxBusPayload` envelope (`type` contains `FluxBusPayload`) with `topic` and `payload` fields.

`FluxBusPayload` is the only envelope type recognized by production bridge code.

Timestamp extraction order includes (first parseable wins):

1. Stream fields: `ts_ms`, `timestamp`, `ts`, `ts_event`
2. Payload fields: `ts_ms`, `timestamp`, `ts`, `ts_event`, `time`, `datetime`

If no parseable timestamp is found, the entry is rejected.

## Handler outputs

| Topic | Output key(s) | Type | Retention / TTL |
| --- | --- | --- | --- |
| `state` | `flux:v1:state:{strategy_id}` | String JSON snapshot | latest only |
| `event` | `flux:v1:events:{strategy_id}` | Stream JSON rows | `MAXLEN ~ 5_000` |
| `trade` | `flux:v1:trades:stream:{strategy_id}` | Stream JSON rows | `MAXLEN ~ 20_000` |
| `alert` | `flux:v1:alerts:{strategy_id}` | Stream JSON rows | `MAXLEN ~ 2_000` |
| `fv` | `flux:v1:fv:stream:{strategy_id}` | Stream JSON rows | `MAXLEN ~ 10_000` |
| `market_bbo` | `flux:v1:market:last:{strategy_id}:{exchange}:{base}_{quote}` | String JSON snapshot | `EX 120` |
| `balances` | `flux:v1:balances:snapshot:{strategy_id}` + `flux:v1:balances:rows:{strategy_id}` | Snapshot + hash | latest snapshot + full hash replace |

All persisted rows include correlation fields:

1. `strategy_id`
2. `topic`
3. `entry_id`
4. `ts_ms`

## Operational behavior

1. Stream discovery uses Redis `SCAN` over configured topic patterns.
2. Ingest loop uses `XREAD` with per-stream offsets.
3. Write operations are applied atomically via Redis transaction pipeline.
4. Handler exceptions are logged and the failed entry is skipped.
5. Redis read/write failures are logged and retried by the run loop.

## Runner notes

`examples/live/makerv3_single_leg/run_bridge.py` is the thin wrapper that wires:

1. Mode-gated environment selection (`paper/testnet/live`, explicit `--confirm-live` required for `live`).
2. Topic aliasing from `flux.strategy.*` to handler suffix topics.
3. Redis connection and consumer startup.

## Verification

```bash
scripts/ci/check-flux-leakage.sh
```
