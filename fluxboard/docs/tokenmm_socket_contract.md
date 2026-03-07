<!-- DOCID: apps/fluxboard/docs/tokenmm_socket_contract@v1 -->

# TokenMM Socket.IO Contract (`tokenmm:v1`)

This document freezes the target TokenMM Socket.IO contract for Fluxboard migration.
REST remains authoritative for initial load and recovery.

As of March 7, 2026, this document is the rollout target rather than a claim that every
TokenMM producer already emits the full contract before Tasks 4-6 in
`docs/plans/2026-03-07-tokenmm-risk-and-portfolio-productionization.md` land and the
remaining verification gaps are closed.

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
9. Fluxboard also supports `/tokenm` and `/tokenm/*` route aliases, which map to the same normalized profile.

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

## Quantity Semantics

Socket payloads follow the same unit rules as TokenMM HTTP.

1. Venue/native size is used for execution, position reconciliation, and raw trade rows.
2. Base-exposure size is used for strategy risk, balances, and inventory.
3. When socket payloads expose risk-facing quantity fields, they must use:
   - `position_qty_venue`
   - `position_qty_base`
   - `local_qty_venue`
   - `local_qty_base`
   - `global_qty_base`
   - `order_qty_venue`
   - `order_qty_base`
   - `qty_conversion_status`
   - `qty_conversion_source`
4. Bare `qty` in trade payloads remains venue/native size unless a paired explicit base field is also present.
5. The current `qty_conversion_status` space is:
   - `identity`
   - `exact_multiplier`
   - `price_based`
   - `unsupported`
   - `missing_metadata`
   - `missing_price`
   - `non_integral_venue_qty`

## Shared Portfolio Ownership

For `profile=tokenmm`, socket risk-facing fields must align with the shared portfolio snapshot owned by
`run_portfolio`.

1. Shared `global_qty_base` and shared completeness metadata come from the portfolio snapshot, not from a client-side recomputation.
2. `signal_delta` may carry portfolio-derived fields, but their semantics must match the shared portfolio snapshot exactly.
3. When present, clients must honor:
   - `aggregation_mode`
   - `global_qty_base_complete`
   - `missing_required`
   - `stale_required`
   - `null_qty_required`
4. `global_qty_base` may be present while `global_qty_base_complete = false` in `partial` mode.
5. Compatibility aliases `global_qty` and `global_qty_complete` may remain temporarily, but they must mirror `global_qty_base` and `global_qty_base_complete`.
6. Clients must not treat `global_qty_base` presence alone as proof of completeness.

## Sequence (`seq`) Semantics

1. `seq` comes from one shared stream per normalized profile room (`profile:tokenmm`).
2. The same counter is used across `market_update`, `signal_delta`, and `trade_update`.
3. Server increments `seq` by exactly `1` per emitted event in that room. The server may intentionally skip values to force a client resync when it detects a bounded replay gap (treated as a gap by clients).
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

Optional recovery hint:

1. Server may include `recovery: {required, reason}` in `market_update` when it detects a replay boundary.
2. Clients must treat `seq` gap detection as the definitive resync trigger; the recovery hint is additive.

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
6. Quantity fields that affect risk must use explicit `*_venue` / `*_base` names and include conversion metadata when derived.
7. Shared risk/completeness fields must preserve the exact semantics of the shared portfolio snapshot.

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

Row identity rules:

1. `row_id` is an opaque stable identifier; clients must not parse it.
2. Server prefers producer-provided `row_id`.
3. If missing, server may synthesize a stable fallback using the backing stream `entry_id` (example: `strategy:trade:entry:<entry_id>`).

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

Trade quantity note:

1. `trade.qty` is venue/native size.
2. If the socket contract later includes normalized trade exposure, it must use a convention-consistent explicit `*_venue` / `*_base` pair together with `qty_conversion_status` and `qty_conversion_source`, rather than changing the meaning of `qty`.

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
