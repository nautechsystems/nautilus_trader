<!-- DOCID: docs/fluxboard/tokenmm_contract@v1 -->

# TokenMM HTTP Contract (`tokenmm:v1`)

This document freezes the HTTP contract for the TokenMM surface.
It is implementation-facing and explicitly excludes order-view.

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
        "description": "Target base quantity per quote/hedge cycle."
      }
    }
  }
]
```

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
      "description": "Target base quantity per quote/hedge cycle."
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
      "qty": 0.015
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
  "has_more": false
}
```

### `DELETE /api/v1/alerts`

Semantics:

1. Clears all alerts for the resolved strategy/profile.
2. Request body is empty.

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
  "remaining": 0,
  "server_ts_ms": 1772607924122
}
```
