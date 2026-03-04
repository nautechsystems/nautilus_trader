<!-- DOCID: docs/fluxboard/tokenmm_socket_contract@v1 -->

# TokenMM Socket.IO Contract (`tokenmm:v1`)

This document freezes the TokenMM Socket.IO contract for Fluxboard migration.
REST remains authoritative for initial load and recovery.

## Scope

Required realtime events:

1. `market_update`
2. `signal_delta`
3. `trade_update`

Explicitly excluded:

1. Any order-view event stream
2. Any event named for order-view behavior (`order_update`, `order_view`, etc.)
3. Any TokenMM frontend order-view route/nav exposure (HTTP SPA fallback may still serve `/tokenmm/order-view`)

## Profile Scoping and Connection Contract

1. REST and Socket.IO share the same profile scope.
2. TokenMM clients send `profile=tokenmm` on REST and on socket connect.
3. Socket.IO path is `/socket.io`.
4. Default transport is polling.
5. Client sends `profile` in connection query.
6. Client emits `set_profile` after connect.
7. Server normalizes profile and joins `profile:<normalized_profile>` room.
8. Profile normalization is strict: `tokenm` and `tokenmm` both normalize to `tokenmm`.

JavaScript client example:

```ts
import { io } from "socket.io-client";

const socket = io("http://127.0.0.1:5022", {
  path: "/socket.io",
  transports: ["polling"],
  query: { profile: "tokenmm" }
});

socket.on("connect", () => {
  socket.emit("set_profile", { profile: "tokenmm" });
});
```

`set_profile` payload:

```json
{
  "profile": "tokenmm"
}
```

## Common Event Fields

All TokenMM events must include:

| Field | Type | Required | Notes |
| --- | --- | --- | --- |
| `profile` | `string` | yes | Normalized profile (`tokenmm`) |
| `seq` | `integer` | yes | Monotonic sequence from one per-profile stream |
| `server_ts_ms` | `integer` | yes | Server event timestamp (ms) |

Compatibility rules:

1. Missing fields in patch payloads mean no change.
2. Explicit `null` in patch payloads means delete.
3. Unknown fields are allowed; clients must ignore them.
4. Socket events use `strategy_id`; this is the same identity as HTTP `signals.strategies[].id`.

## Sequence (`seq`) Semantics

1. `seq` comes from one shared stream per normalized profile room (`profile:tokenmm`).
2. The same counter is used across `market_update`, `signal_delta`, and `trade_update`.
3. Server increments `seq` by exactly `1` per emitted event in that room.
4. Client keeps one `last_seq` per normalized profile.
5. Client handling rules:
   - `seq == last_seq + 1`: apply event
   - `seq <= last_seq`: treat as duplicate/stale and ignore
   - `seq > last_seq + 1`: gap detected, trigger REST resync
6. If server restarts and sequence appears reset, treat as a gap and run REST resync.

## Event: `market_update`

Purpose:

1. Lightweight heartbeat
2. Trigger UI refresh when strategy/alert state changed

Payload example:

```json
{
  "profile": "tokenmm",
  "seq": 4102,
  "server_ts_ms": 1772608000123,
  "server_time": "2026-03-04T03:33:20.123Z",
  "strategies": {
    "changed": [
      "maker_v3_01"
    ]
  },
  "alerts": {
    "count": 2,
    "latest_ts_ms": 1772607999900
  }
}
```

## Event: `signal_delta`

Purpose:

1. Patch one strategy payload without full reload
2. Update leg-level data using `contract_id`

Patch rules:

1. `patch.legs` is a map keyed by `contract_id`.
2. Each non-null leg row includes `contract_id`.
3. `patch.legs_order` is optional and controls deterministic ordering.
4. Missing field means no change.
5. Explicit `null` means delete.

Payload example:

```json
{
  "profile": "tokenmm",
  "strategy_id": "maker_v3_01",
  "seq": 921,
  "server_ts_ms": 1772608001123,
  "patch": {
    "tradeable": true,
    "managed_orders": 7,
    "legs_order": [
      "binance:BTCUSDT"
    ],
    "legs": {
      "binance:BTCUSDT": {
        "contract_id": "binance:BTCUSDT",
        "exchange": "binance",
        "symbol": "BTCUSDT",
        "bid": 94255.0,
        "ask": 94255.3,
        "mid": 94255.15,
        "ts_ms": 1772608001101,
        "age_ms": 22,
        "state": "ok"
      },
      "bybit:BTCUSDT": null
    }
  }
}
```

## Event: `trade_update`

Purpose:

1. Append or upsert blotter rows
2. Delete a row when needed

Required fields:

1. `op`
2. `row_id`
3. `version`
4. `seq`
5. `trade` (required for `op=upsert`, nullable for `op=delete`)

Consistency rule:

1. For `op=upsert`, `trade.version` MUST be present and equal to top-level `version`.

Upsert example:

```json
{
  "profile": "tokenmm",
  "strategy_id": "maker_v3_01",
  "seq": 3051,
  "server_ts_ms": 1772608002123,
  "op": "upsert",
  "row_id": "maker_v3_01:trade:1772608002000:0",
  "version": 1,
  "trade": {
    "row_id": "maker_v3_01:trade:1772608002000:0",
    "version": 1,
    "strategy_id": "maker_v3_01",
    "ts_ms": 1772608002000,
    "exchange": "binance",
    "symbol": "BTCUSDT",
    "side": "SELL",
    "price": 94254.8,
    "qty": 0.01
  }
}
```

Delete example:

```json
{
  "profile": "tokenmm",
  "strategy_id": "maker_v3_01",
  "seq": 3052,
  "server_ts_ms": 1772608003123,
  "op": "delete",
  "row_id": "maker_v3_01:trade:1772607999000:0",
  "version": 2,
  "trade": null
}
```

## Reconnect and Recovery Semantics

1. On every reconnect, client MUST emit `set_profile` again.
2. Client MUST assume events can be dropped while disconnected.
3. After reconnect, client MUST resync via REST:
   - `GET /api/v1/signals?profile=tokenmm`
   - `GET /api/v1/trades/delta?profile=tokenmm&after=max(0,last_trade_ts_ms-1)`
   - `GET /api/v1/alerts?profile=tokenmm`
4. Trade cursor fallback is deterministic:
   - persist `(last_trade_ts_ms, last_trade_row_id, last_trade_version)`
   - dedupe by `row_id` with highest `version` winning
   - drop rows where tuple `(ts_ms, row_id, version)` is `<=` persisted cursor tuple
5. If `seq` gap is detected, client MUST run the same REST resync.
6. `trade_update` idempotency key is `row_id` with highest `version` winning.
7. `signal_delta` applies only when `seq` is newer than the last applied `seq`.
