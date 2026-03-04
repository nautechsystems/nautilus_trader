# Flux Redis Schema (`flux:v1`)

This document defines the production Redis contract for Flux integrations.

## Scope and invariants

1. Namespace is fixed to `flux:v1`.
2. Keys are strategy-scoped by default (`{strategy_id}` is required unless explicitly documented global).
3. There is no global unscoped strategy state.
4. Each datum has one authoritative store (no map+list duplication for the same canonical record set).
5. High-churn data is always bounded by retention policy.
6. `ts_ms` is the canonical event timestamp field for Flux payloads (integer Unix milliseconds, UTC).

## Naming conventions

1. Canonical output keys: `flux:v1:{domain}:{strategy_id}:...`
2. Canonical input stream: `flux:v1:in:stream:{environment}:{strategy_id}:{topic}`
3. Pub/Sub channels follow the same namespace and are strategy-scoped unless marked global.

## Production schema

| Key / channel | Type | Producer | Consumer | Retention / TTL | Notes |
| --- | --- | --- | --- | --- | --- |
| `flux:v1:in:stream:{environment}:{strategy_id}:{topic}` | Stream | Strategy runtime, adapters, or ingress writers | Flux bridge consumers | `XADD MAXLEN ~ 50_000` default (`10_000-250_000`) | Input convention for bridge ingestion. Every entry must carry `strategy_id` and `ts_ms`. |
| `flux:v1:state:{strategy_id}` | String (JSON snapshot) | Flux bridge | Flux API, ops tools | Latest-only overwrite; optional `EX 86_400` for stale cleanup | Authoritative latest strategy state snapshot. |
| `flux:v1:events:{strategy_id}` | Stream | Flux bridge | Flux API, monitoring | `XADD MAXLEN ~ 5_000` default (`1_000-50_000`) | High-churn operational events. |
| `flux:v1:alerts:{strategy_id}` | Stream | Flux bridge | Flux API, alerting systems | `XADD MAXLEN ~ 2_000` default (`200-20_000`) | Human/actionable alerts, bounded history. |
| `flux:v1:trades:stream:{strategy_id}` | Stream | Flux bridge | Flux API, reporting consumers | `XADD MAXLEN ~ 20_000` default (`2_000-200_000`) | Canonical trade feed for blotter/history queries. |
| `flux:v1:fv:stream:{strategy_id}` | Stream | Flux bridge | Flux API, analytics consumers | `XADD MAXLEN ~ 10_000` default (`1_000-100_000`) | Fair-value update history; replaces unbounded snapshots. |
| `flux:v1:balances:snapshot:{strategy_id}` | String (JSON snapshot) | Flux bridge | Flux API | Latest-only overwrite; `EX 86_400` default (`3_600-604_800`) | Portfolio/account snapshot for fast API reads. |
| `flux:v1:balances:rows:{strategy_id}` | Hash | Flux bridge | Flux API | Key-level `EX 86_400` default (`3_600-604_800`) | Row key format: `{exchange}:{asset}:{account}`. |
| `flux:v1:market:last:{strategy_id}:{exchange}:{base}_{quote}` | String (JSON snapshot) | Flux bridge | Flux API | `EX 120` default (`30-600`) | Last market top-of-book snapshot; short TTL prevents stale reads. |
| `flux:v1:params:{strategy_id}` | Hash | Config writers / control plane | Strategy and bridge param readers | Persistent by default (no TTL) | Authoritative strategy-scoped parameter store. |
| `flux:v1:params:{strategy_id}` | Pub/Sub channel | Config writers / control plane | Strategy and bridge subscribers | Ephemeral transport | Parameter-update signal channel (same address as hash by design). |
| `flux:v1:params:global` | Pub/Sub channel | Config writers / control plane | Strategy and bridge subscribers | Ephemeral transport | Global broadcast for cross-strategy param refresh notices only. |

## High-churn retention defaults

| Dataset | Canonical key | Default | Allowed range |
| --- | --- | --- | --- |
| Events | `flux:v1:events:{strategy_id}` | `MAXLEN ~ 5_000` | `1_000-50_000` |
| Alerts | `flux:v1:alerts:{strategy_id}` | `MAXLEN ~ 2_000` | `200-20_000` |
| Trades | `flux:v1:trades:stream:{strategy_id}` | `MAXLEN ~ 20_000` | `2_000-200_000` |
| Fair value updates | `flux:v1:fv:stream:{strategy_id}` | `MAXLEN ~ 10_000` | `1_000-100_000` |

## Bridge and API contract

1. Bridge normalizes all incoming timestamps to `ts_ms` before writes.
2. Bridge rejects or dead-letters records missing `strategy_id` or parseable timestamp.
3. API reads only canonical `flux:v1:*` output keys/channels and returns `ts_ms` unchanged.
4. API must not infer strategy context from global keys; `strategy_id` is explicit in every lookup path.

## Migration from `maker_poc.*` / `maker_poc`

Legacy names are supported only in explicit compatibility mode.

### Legacy mapping

| Legacy input | Canonical `flux:v1` destination |
| --- | --- |
| `maker_poc.state` | `flux:v1:state:{strategy_id}` |
| `maker_poc.event` | `flux:v1:events:{strategy_id}` |
| `maker_poc.trade` | `flux:v1:trades:stream:{strategy_id}` |
| `maker_poc.alert` | `flux:v1:alerts:{strategy_id}` |
| `maker_poc.fv` | `flux:v1:fv:stream:{strategy_id}` |
| `maker_poc.balances` | `flux:v1:balances:snapshot:{strategy_id}` and `flux:v1:balances:rows:{strategy_id}` |
| `maker_poc.market_bbo` | `flux:v1:market:last:{strategy_id}:{exchange}:{base}_{quote}` |
| `maker_poc` (legacy stream key) | `flux:v1:in:stream:{environment}:{strategy_id}:{topic}` topic fan-out |

### Compatibility mode notes

1. `migration_mode=compat` reads legacy `maker_poc.*` and `maker_poc` inputs.
2. Compatibility mode writes only `flux:v1:*` outputs (no dual-write back to legacy names).
3. Compatibility mode must still enforce `strategy_id` scoping and `ts_ms` canonicalization at ingest.
4. API stays `flux:v1`-only even while compatibility mode is enabled.

### Removal plan

1. Enable compatibility mode per environment for cutover only.
2. Keep compatibility mode until legacy traffic is zero for 14 consecutive days in that environment.
3. Disable compatibility mode by default in the next release after zero-traffic confirmation.
4. Remove compatibility aliases and remaining `maker_poc` references one release after default-disable.
