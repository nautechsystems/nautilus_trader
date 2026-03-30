<!-- DOCID: apps/fluxboard/docs/tokenmm_contract@v1 -->

# TokenMM HTTP Contract (`tokenmm:v1`)

This document freezes the target HTTP contract for the TokenMM surface.
It is implementation-facing and explicitly excludes order-view.
The matching equities surface is documented separately in `fluxboard/docs/equities_contract.md`.

As of March 7, 2026, this is the rollout target contract. It is not a claim that every
TokenMM API, portfolio, and startup-reconciliation surface already matches the contract
before Tasks 4-6 in `docs/plans/2026-03-07-tokenmm-risk-and-portfolio-productionization.md`
and their remaining verification failures are resolved.

Operator validation and rollout gates are maintained in
`docs/runbooks/tokenmm-risk-validation.md`.


## Scope and Route Surface

In scope pages:

1. Dashboard
2. Signals
3. Params
4. Balances
5. Trades blotter
6. Alerts

TokenMM routes:

| Route | Status | Notes |
| --- | --- | --- |
| `/tokenmm` | required | Dashboard landing |
| `/tokenmm/dashboard` | required | Dashboard explicit route |
| `/tokenmm/signal` | required | Signals page |
| `/tokenmm/params` | required | Params page |
| `/tokenmm/balances` | required | Balances page |
| `/tokenmm/trades` | required | Trades page |
| `/tokenmm/alerts` | required | Alerts page |
| `/tokenm` | compat | Legacy alias; redirects to `/tokenmm` |
| `/tokenm/*` | compat | Legacy alias; redirects to `/tokenmm/*` |
| `/tokenmm/order-view` | ui-forbidden | SPA deep-link may resolve at HTTP layer, but TokenMM UI route/nav must not expose order-view |

Order-view is excluded everywhere:

1. No `/tokenmm/order-view` route in TokenMM frontend router
2. No order-view REST endpoints
3. No order-view socket events

## Envelope Contract

All HTTP responses use this envelope:

```json
{
  "ok": true,
  "api_version": "v1",
  "request_id": "9c88f5a7-9e8e-4ec7-aa9d-c28970be5c0f",
  "timestamp_ms": 1772607900123,
  "data": {},
  "error": null
}
```

Error envelope:

```json
{
  "ok": false,
  "api_version": "v1",
  "request_id": "f1f4f3ae-285e-455b-980e-541b09d1234f",
  "timestamp_ms": 1772607901123,
  "data": null,
  "error": {
    "code": "invalid_params_update",
    "message": "Unknown parameter `foo`.",
    "details": {
      "strategy_id": "maker_v3_01"
    }
  }
}
```

## Locked Contract Decisions

1. `contract_id` format is `exchange:symbol` (example `binance:BTCUSDT`).
2. `symbol` must be the canonical venue symbol, including qualifiers needed for uniqueness.
3. If two legs would collide on the same `exchange:symbol`, server config MUST fail fast.
4. `legs` MUST be a map keyed by `contract_id`.
5. Each leg row MUST also include `contract_id`.
6. `legs_order` is optional and provides deterministic ordering when present.
7. `profile` values `tokenm` and `tokenmm` normalize to `tokenmm`.
8. `signals.strategies[].id` and `signals.strategies[].meta.strategy_id` are the same identity.
9. REST is authoritative for first render and recovery.

Required leg fields:

| Field | Type | Required | Notes |
| --- | --- | --- | --- |
| `contract_id` | `string` | yes | Must match the map key |
| `exchange` | `string` | yes | Lowercase recommended |
| `symbol` | `string` | yes | Canonical venue symbol (qualifiers preserved) |
| `bid` | `number \| null` | yes | Best bid |
| `ask` | `number \| null` | yes | Best ask |
| `mid` | `number \| null` | yes | Derived mid |
| `ts_ms` | `integer \| null` | yes | Market timestamp in ms |
| `age_ms` | `integer \| null` | yes | Age at serialization |
| `state` | `string` | yes | Health marker |

## Shared Quote Health Semantics

These semantics are shared across Flux strategy families and should remain stable as new strategies and venues are added.

1. `age_ms` is informational only: it is the time since the last observed quote update at serialization time.
2. `age_ms` alone does not prove feed failure. Quiet books and unchanged quotes may still have large ages.
3. `feed_state`, when present, describes transport/subscription health:
   - `ok`
   - `degraded`
   - `down`
   - `unknown`
4. `quote_state`, when present, describes quote freshness/presence:
   - `fresh`
   - `old`
   - `missing`
5. Old-but-connected quotes must be represented as `feed_state = ok` and `quote_state = old`; they are not feed-down by default.
6. `tradeable` and any future `hedgeable`-style flags are backend policy outputs, not UI heuristics derived from `age_ms`.
7. Operator surfaces should present quote age separately from feed health so “quiet market”, “old quote”, and “broken feed” are not conflated.

## Common HTTP Rules

1. TokenMM REST clients SHOULD send `profile` query on TokenMM endpoints.
2. REST normalization matches socket normalization: `tokenm` and `tokenmm` map to `tokenmm`.
3. `strategy` query is optional; server default strategy is used if omitted.
4. Backend routing is strategy-driven today; `profile` is compatibility metadata unless explicit profile allowlisting is configured.
5. `limit` query is clamped to `1..200` (default `50` where supported).
6. `strategy_id` is the canonical strategy field name across endpoints and socket payloads.
7. In `signals`, `id` is an alias of `meta.strategy_id` for compatibility.
8. Unknown response fields may appear; clients must ignore unknown fields.
9. Examples below show the `data` payload; wrap with the envelope above.

## Quantity Semantics

TokenMM HTTP distinguishes execution size from base-exposure size.

1. Nautilus core `quantity` fields remain venue/native size for orders, fills, and positions.
2. Risk, balances, and portfolio inventory must use explicit base-exposure fields.
3. Risk-facing HTTP payloads must use the following field names when quantity units matter:
   - `position_qty_venue`
   - `position_qty_base`
   - `local_qty_venue`
   - `local_qty_base`
   - `global_qty_base`
   - `order_qty_venue`
   - `order_qty_base`
   - `qty_conversion_status`
   - `qty_conversion_source`
4. `*_venue` means exchange-native contracts / lots / shares.
5. `*_base` means normalized base-asset exposure used for strategy risk and balances.
6. Compatibility-only bare `qty` fields must document their unit explicitly; new risk-facing fields must not rely on an unlabeled `qty`.
7. The current `qty_conversion_status` space is:
   - `identity`
   - `exact_multiplier`
   - `price_based`
   - `unsupported`
   - `missing_metadata`
   - `missing_price`
   - `non_integral_venue_qty`
8. `qty_conversion_source` names the rule used for the conversion.

## Shared Portfolio Ownership

For `profile=tokenmm`, shared portfolio state must come from one backend owner.

1. `run_portfolio` is the canonical source of truth for shared TokenMM portfolio state.
2. Shared portfolio state includes inventory aggregate, contributor diagnostics, merged balances rows, and merged balances totals.
3. `/api/v1/signals?profile=tokenmm` and `/api/v1/balances?profile=tokenmm` must consume that shared portfolio snapshot.
4. Clients must not assume a separate API-side recomputation of shared global risk or shared balances.
5. Per-strategy debug endpoints may still read strategy-scoped snapshots directly.
6. `Balances(profile=tokenmm)` is a rendering of the shared portfolio snapshot, not a second TokenMM risk engine.
7. `Signals(profile=tokenmm)` may render strategy-local and portfolio-global quantities, but it must not derive them from balances except in explicit compatibility fallback mode.

## Shared Portfolio Completeness Semantics

When shared portfolio fields are present, clients must honor explicit completeness metadata.

1. `aggregation_mode` is either `strict` or `partial`.
2. `global_qty_base_complete = true` means all required contributors are fresh and known.
3. `global_qty_base_complete = false` means the shared view is incomplete even if `global_qty_base` is present.
4. `missing_required`, `stale_required`, and `null_qty_required` name the contributors preventing completeness.
5. In `partial` mode, `global_qty_base` is the sum of fresh known contributors.
6. In `strict` mode, `global_qty_base` may be `null` when any required contributor is missing, stale, or unknown.
7. Compatibility aliases `global_qty` and `global_qty_complete` may remain temporarily, but they must mirror `global_qty_base` and `global_qty_base_complete`.

## Endpoint Contracts

### `GET /api/v1/signals?profile=tokenmm`

Request:

```bash
curl -s 'http://127.0.0.1:5022/api/v1/signals?profile=tokenmm&strategy=maker_v3_01'
```

Response `data`:

```json
{
  "server_ts_ms": 1772607900120,
  "strategies": [
    {
      "id": "maker_v3_01",
      "meta": {
        "strategy_id": "maker_v3_01",
        "class": "MakerV3Strategy",
        "strategy_groups": "tokenmm",
        "base_asset": "BTC",
        "quote_asset": "USDT"
      },
      "tradeable": true,
      "blocked": false,
      "managed_orders": 8,
      "legs_order": [
        "binance:BTCUSDT",
        "bybit:BTCUSDT"
      ],
      "legs": {
        "binance:BTCUSDT": {
          "contract_id": "binance:BTCUSDT",
          "exchange": "binance",
          "symbol": "BTCUSDT",
          "bid": 94250.1,
          "ask": 94250.4,
          "mid": 94250.25,
          "ts_ms": 1772607900088,
          "age_ms": 35,
          "state": "ok"
        },
        "bybit:BTCUSDT": {
          "contract_id": "bybit:BTCUSDT",
          "exchange": "bybit",
          "symbol": "BTCUSDT",
          "bid": 94249.9,
          "ask": 94250.5,
          "mid": 94250.2,
          "ts_ms": 1772607900086,
          "age_ms": 37,
          "state": "ok"
        }
      },
      "state": {
        "bot_on": true
      },
      "fv_row": {
        "ts_ms": 1772607899950
      }
    }
  ]
}
```

#### Signal/dashboard fields (best-effort)

1. TokenMM UI expects additional fields to be present when available (backend may omit fields when unknown).
2. Clients MUST ignore unknown fields and tolerate missing optional fields.

Common fields:

| Field | Type | Notes |
| --- | --- | --- |
| `strategy_family` | `string` | Strategy family label derived from metadata. |
| `params` | `object` | Runtime params snapshot (typed values). |
| `balances_ok` | `boolean` | True when balances snapshot is present. |
| `risk_delta` | `number \| null` | Inventory/risk proxy (best-effort). Not the canonical field for local spot inventory when `local_qty_base` is available. |
| `risk_delta_ts_ms` | `integer \| null` | Timestamp for the risk delta value. |
| `decision_edge_bps` | `number \| null` | Current decision edge (basis points). |
| `required_edge_bps` | `number \| null` | Required edge threshold for the chosen case. |
| `edge2_bps` | `number \| null` | `decision_edge_bps - required_edge_bps` when derivable. |
| `spread_net_bps` | `number \| null` | Net spread (best-case selection). |
| `spread_net_case1_bps` | `number \| null` | Net spread for case1 (derived or backend-provided). |
| `spread_net_case2_bps` | `number \| null` | Net spread for case2 (derived or backend-provided). |
| `spread_net_best_case` | `"case1" \| "case2" \| null` | Best-case selector when known. |
| `maker_role_map` | `object` | Role mapping (e.g. `maker_leg`, `ref_leg`, `hedge_leg`) keyed by `contract_id`. |
| `maker_quote_status` | `object \| null` | Compact quote counts/health fields when available. |
| `quote_stacks` | `object \| null` | Per-leg/per-band quote stack summary when available. |
| `pricing_adjustments` | `array` | Pricing/skew adjustments (best-effort). |
| `balance_readiness` | `object \| null` | Backend readiness/health markers for balances inputs. |
| `position_qty_venue` | `number \| null` | Venue/native position size for the maker leg. |
| `position_qty_base` | `number \| null` | Base-asset exposure derived from `position_qty_venue`. |
| `local_qty_venue` | `number \| null` | Local venue/native inventory used for reconciliation/debug. |
| `local_qty_base` | `number \| null` | Local normalized base exposure used for risk/skew. |
| `global_qty_base` | `number \| null` | Shared normalized base exposure from the canonical portfolio snapshot. |
| `global_qty_base_complete` | `boolean \| null` | Completeness bit for `global_qty_base`; false means the shared view is partial or degraded. |
| `aggregation_mode` | `string \| null` | Shared portfolio aggregation mode, currently `strict` or `partial`. |
| `order_qty_venue` | `number \| null` | Venue/native order size after any base-to-venue conversion. |
| `order_qty_base` | `number \| null` | Requested base exposure for order/risk controls. |
| `qty_conversion_status` | `string \| null` | Current status values: `identity`, `exact_multiplier`, `price_based`, `unsupported`, `missing_metadata`, `missing_price`, `non_integral_venue_qty`. |
| `qty_conversion_source` | `string \| null` | Short rule identifier for the conversion path. |

`maker_v3.quote_snapshot` fields are best-effort and may include top-of-book, FV inputs, edge configuration/effective edge numbers, and derived placement prices.

### `GET /api/v1/params?profile=tokenmm`

Request:

```bash
curl -s 'http://127.0.0.1:5022/api/v1/params?profile=tokenmm&strategy=maker_v3_01'
```

Response `data`:

```json
[
  {
    "strategy_id": "maker_v3_01",
    "params": {
      "bot_on": true,
      "qty": 1000.0,
      "max_age_ms": 10000
    },
    "schema": {
      "bot_on": {
        "type": "boolean",
        "description": "Enable quote publishing and management."
      },
      "qty": {
        "type": "number",
        "description": "Compatibility field. Unit must be interpreted together with strategy quantity semantics; risk-facing payloads should publish explicit order_qty_base/order_qty_venue fields."
      }
    }
  }
]
```

Compatibility note:

1. `params.qty` remains a legacy field in this contract.
2. Clients must not infer whether `params.qty` is base exposure or venue/native size without the paired quantity-unit metadata.
3. New risk and balances surfaces must prefer `order_qty_base` and `order_qty_venue`.

### `PATCH /api/v1/params`

Request:

```bash
curl -s -X PATCH 'http://127.0.0.1:5022/api/v1/params?profile=tokenmm&strategy=maker_v3_01' \
  -H 'Content-Type: application/json' \
  -d '{"params":{"bot_on":false,"qty":750.0}}'
```

Response `data`:

```json
{
  "success": [
    {
      "strategy_id": "maker_v3_01",
      "updated": [
        "bot_on",
        "qty"
      ],
      "params": {
        "bot_on": false,
        "qty": 750.0,
        "max_age_ms": 10000
      }
    }
  ],
  "failed": [],
  "errors": []
}
```

### `GET /api/v1/param-schema?profile=tokenmm`

Request:

```bash
curl -s 'http://127.0.0.1:5022/api/v1/param-schema?profile=tokenmm'
```

Response `data`:

```json
{
  "params": {
    "bot_on": {
      "type": "boolean",
      "description": "Enable quote publishing and management."
    },
    "qty": {
      "type": "number",
      "description": "Compatibility field. Quantity unit must be paired with explicit order_qty_base/order_qty_venue semantics."
    },
    "max_age_ms": {
      "type": "integer",
      "description": "Replace managed orders older than this age."
    }
  },
  "deprecated": {}
}
```

### `GET /api/v1/balances`

Balances and inventory rows must use explicit unit-bearing fields when they expose position or order size.

For `profile=tokenmm`, the response must also carry the shared portfolio ownership/completeness fields when available:

- `source = "portfolio_snapshot"`
- `stale_after_ms`
- `aggregation_mode`
- `global_qty_base`
- `global_qty_base_complete`
- `global_qty`
- `global_qty_complete`
- `components`
- `missing_required`
- `stale_required`
- `null_qty_required`

Snapshot freshness gate:

1. `source = "portfolio_snapshot"` is valid only when the shared snapshot is fresh enough: snapshot `server_ts_ms` and inventory `ts_ms` must both be within `stale_after_ms`.
2. If that freshness gate fails, `/api/v1/balances?profile=tokenmm` falls back to the live per-strategy merge path and must not keep advertising `source = "portfolio_snapshot"`.
3. Clients must treat `stale_after_ms` as the freshness budget for the shared snapshot.

Fluxboard risk consumption:

1. `risk_groups` is backend-authored. Fluxboard must not rebuild TokenMM risk grouping from holdings coins.
2. `risk_groups[].rows` is backend-authored drilldown content and order.
3. Balance rows participating in a risk group must carry row-level `risk_key` and `risk_label` fields so Holdings drilldown can use the same semantics as the Risk tab.

Required quantity field names for risk-facing rows:

- `position_qty_venue`
- `position_qty_base`
- `local_qty_venue`
- `local_qty_base`
- `global_qty_base`
- `order_qty_venue`
- `order_qty_base`
- `qty_conversion_status`
- `qty_conversion_source`

Request:

```bash
curl -s 'http://127.0.0.1:5022/api/v1/balances?profile=tokenmm&strategy=maker_v3_01&limit=50'
```

Response `data`:

```json
{
  "rows": [
    {
      "row_id": "maker_v3_01:acc:0",
      "strategy_id": "maker_v3_01",
      "exchange": "binance",
      "asset": "USDT",
      "free": "125000.12",
      "locked": "5000.00",
      "total": "130000.12"
    }
  ],
  "count": 1,
  "server_ts_ms": 1772607916119
}
```

### `GET /api/v1/trades`

Trade row contract:

1. Each row MUST include `row_id`, `ts_ms`, and `version`.
2. `version` is required for deterministic reconnect/dedupe behavior.
3. `row_id` is an opaque stable identifier; server may synthesize a fallback when producer rows are missing one.
4. For TokenMM-facing REST rows, `qty` is operator-facing base quantity.
5. When normalized trade exposure is available, rows MUST also publish:
   - `qty_base`
   - `qty_venue`
   - `qty_conversion_status`
   - `qty_conversion_source`
6. Shared producer bare `qty` remains venue/native size; `qty_base` and `qty_venue` carry the normalized pair.
7. Older raw SQLite rows may not have `*_base` / `*_venue` columns.
8. Legacy Redis trade rows cannot be safely reinterpreted after the fact without producer-supplied normalized fields.
9. Rollout requires a TokenMM trade-stream cutover/reset before enabling base-first `qty` in production.

Request:

```bash
curl -s 'http://127.0.0.1:5022/api/v1/trades?profile=tokenmm&strategy=maker_v3_01&limit=100'
```

Response `data`:

```json
{
  "rows": [
    {
      "row_id": "maker_v3_01:trade:1772607905000:1",
      "version": 1,
      "strategy_id": "maker_v3_01",
      "ts_ms": 1772607905000,
      "exchange": "binance",
      "symbol": "BTCUSDT",
      "side": "BUY",
      "price": 94220.5,
      "qty": 0.015,
      "qty_base": 0.015,
      "qty_venue": 0.015,
      "qty_conversion_status": "identity",
      "qty_conversion_source": "generic:multiplier=1",
      "fee": 0.12
    }
  ],
  "total": 1,
  "limit": 100,
  "offset": 0,
  "has_more": false,
  "last_seq": 1092,
  "sort": "ts_ms_desc"
}
```

### `GET /api/v1/trades/delta`

Delta trade rows use the same contract as `GET /api/v1/trades`, including required `version`.

Request:

```bash
curl -s 'http://127.0.0.1:5022/api/v1/trades/delta?profile=tokenmm&strategy=maker_v3_01&since_seq=1091&limit=100'
```

Response `data`:

```json
{
  "rows": [
    {
      "row_id": "maker_v3_01:trade:1772607905000:1",
      "version": 1,
      "strategy_id": "maker_v3_01",
      "ts_ms": 1772607905000,
      "exchange": "binance",
      "symbol": "BTCUSDT",
      "side": "BUY",
      "price": 94220.5,
      "qty": 0.015,
      "qty_base": 0.015,
      "qty_venue": 0.015,
      "qty_conversion_status": "identity",
      "qty_conversion_source": "generic:multiplier=1"
    }
  ],
  "last_seq": 1092,
  "reset_required": false
}
```

Delta cursor semantics:

1. Primary cursor mode uses `since_seq` (preferred for reconnect-safe replay).
2. When `reset_required=true`, client must do a full reload via `GET /api/v1/trades`.
3. When `reset_required=false` and `since_seq` is used, delta rows are oldest-to-newest and `last_seq` equals the last returned row.
4. `after` is compatibility-only fallback and does not guarantee oldest-to-newest ordering.
5. If production is switching from venue-native bare `qty` to base-first `qty`, perform a TokenMM trade-stream cutover/reset so cached Redis rows do not leak the legacy interpretation into the API.

### `GET /api/v1/alerts`

Request:

```bash
curl -s 'http://127.0.0.1:5022/api/v1/alerts?profile=tokenmm&strategy=maker_v3_01&limit=50'
```

Response `data`:

```json
{
  "rows": [
    {
      "row_id": "maker_v3_01:alert:1772607910000:0",
      "strategy_id": "maker_v3_01",
      "ts_ms": 1772607910000,
      "level": "warning",
      "code": "quote_stale",
      "message": "Leg data stale > 10000ms",
      "details": {
        "contract_id": "bybit:BTCUSDT"
      }
    }
  ],
  "total": 1,
  "limit": 50,
  "offset": 0,
  "has_more": false,
  "capabilities": {
    "feed_mode": "active",
    "clear_mode": "history_only"
  }
}
```

### `DELETE /api/v1/alerts`

Semantics:

1. Clears stream-backed alert history for the resolved strategy/profile.
2. Shared alerts clients must read `capabilities.clear_mode`; when it is `history_only`, currently active resolver-backed rows are not dismissed by this route.
3. Request body is empty.

Request:

```bash
curl -s -X DELETE 'http://127.0.0.1:5022/api/v1/alerts?profile=tokenmm&strategy=maker_v3_01'
```

Response `data`:

```json
{
  "success": true,
  "strategy_id": "maker_v3_01",
  "deleted": 3,
  "remaining": 1,
  "capabilities": {
    "feed_mode": "active",
    "clear_mode": "history_only"
  },
  "server_ts_ms": 1772607924122
}
```
