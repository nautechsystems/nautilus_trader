# Flux Redis Schema (`flux:v1`)

This document defines the production Redis contract for Flux integrations.

## Scope and invariants

1. Namespace is fixed to `flux:v1`.
2. Keys are strategy-scoped by default (`{strategy_id}` is required unless explicitly documented global).
3. There is no global unscoped strategy state.
4. Each datum has one authoritative store (no map+list duplication for the same canonical record set).
5. High-churn data is always bounded by retention policy.
6. `ts_ms` is the canonical event timestamp field for Flux payloads (integer Unix milliseconds, UTC).
7. Identity uniqueness policy is configuration-level: `strategy_instance_id` must equal `strategy_id`; Redis keys stay scoped by `strategy_id` with no schema change.

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

`flux:v1:params:{strategy_id}` is intentionally dual-role:
1. Hash operations (`HGET`/`HSET`/`HMGET`) are the authoritative persistent parameter store.
2. Pub/Sub operations (`SUBSCRIBE`/`PUBLISH`) are only change-notification transport.
3. Writers must update the hash and then publish a refresh signal; pub/sub does not persist parameter values.

## High-churn retention defaults

This table is the authoritative source for production high-churn retention defaults and allowed tuning ranges.

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

## Legacy naming allowlist (migration-only)

This document includes a single legacy mapping section for one-time cutover planning. It must quote historical
key names which contain banned legacy prefixes.

Those legacy strings are allowed only inside the marker block below:

1. `<!-- leakage-allowlist:start maker_poc_migration -->`
2. `<!-- leakage-allowlist:end maker_poc_migration -->`

The CI/pre-commit gate `scripts/ci/check-flux-leakage.sh` strips the allowlisted block and fails if legacy naming
appears anywhere else in production Flux paths or durable Flux docs.

Policy:

1. Do not add additional allowlist marker pairs (the gate requires exactly one pair).
2. If further legacy mapping is required, extend the existing allowlist block and keep it minimal.
3. Consider deleting the legacy mapping after cutover is complete.

<!-- leakage-allowlist:start maker_poc_migration -->
## Migration from `maker_poc.*` / `maker_poc`

Legacy names below are a one-time cutover reference only.

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
| `maker_poc.params` | `flux:v1:params:global` pub/sub broadcast only (no hash write) |
| `maker_poc.params.{strategy_id}` | `flux:v1:params:{strategy_id}` dual-role address: `HSET`/`HMSET` hash update, then `PUBLISH` same address |
| `maker_poc` (legacy stream key) | `flux:v1:in:stream:{environment}:{strategy_id}:{topic}` topic fan-out; inbound entries must include `topic` |

### Cutover policy

1. Production bridge/API read and write only `flux:v1:*` keys and channels.
2. There is a single production build; no runtime legacy-read switch or dual-path ingestion.
3. Legacy `maker_poc` names remain documentation-only mapping references for one-time cutover planning.
4. For `flux:v1:in:stream:{environment}:{strategy_id}:{topic}`, `{environment}` is sourced from `FluxConfig.mode`; if `FluxConfig` is not wired, use the process-level `environment` config field. If neither source is available, fail fast at startup.
5. Missing required routing coordinates (`topic`, `strategy_id`, `ts_ms`, resolved `{environment}`) must fail fast: reject/dead-letter the entry and emit an error.
<!-- leakage-allowlist:end maker_poc_migration -->
