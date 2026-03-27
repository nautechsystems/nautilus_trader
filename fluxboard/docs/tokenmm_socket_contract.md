<!-- DOCID: apps/fluxboard/docs/tokenmm_socket_contract@v1 -->

# TokenMM Socket.IO Contract (`tokenmm:v1`)

This document freezes the TokenMM Socket.IO contract for Fluxboard migration.
REST remains authoritative for initial load and bounded recovery.

As of March 26, 2026, canonical `tokenmm.trades` uses the standard realtime contract in steady
state. Non-canonical trades views remain REST-only, and the legacy `trade_update` path is still
retained for rollback and compatibility clients until the remaining cleanup wave lands.

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
4. For TokenMM `trade_update` payloads, bare `qty` is operator-facing base quantity.
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
7. Fluxboard risk drilldown stays API-driven: backend-authored `risk_groups`, `risk_groups[].rows`, and row-level `risk_key` / `risk_label` semantics come from REST and must not be reconstructed from socket-side coin bucketing.

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

## Fluxboard Resync Completion Ownership

Fluxboard's global resync completion contract is owned by [`stores.ts`](../stores.ts).

1. Acknowledgement ownership is surface-aware, not globally hard-coded.
2. `bumpGlobalResync` opens a new resync epoch and `markGlobalResyncApplied` records per-consumer acknowledgement for that epoch.
3. For `tokenmm.trades` and `equities.trades`, the authoritative acknowledgement consumer is `trades` only.
4. `order-view` is explicitly excluded from TokenMM resync ownership because the profile does not expose an order-view route or panel.
5. If a future surface mounts both trades and order-view together, both mounted consumers must acknowledge the same epoch before it clears.
6. One consumer acknowledging the current epoch must not clear the resync for a multi-consumer surface by itself.
7. A stale acknowledgement from an older epoch may remain recorded for that consumer, but it must not clear a newer active epoch.

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
2. Clients must treat `seq` gap detection and explicit `recovery_required` events as the definitive resync triggers; the recovery hint is additive.
3. Unchanged legacy/cursor conditions should emit at most one repeated hint per condition until the condition changes or the profile is re-armed.

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
    "qty": 0.01,
    "qty_base": 0.01,
    "qty_venue": 0.01,
    "qty_conversion_status": "identity",
    "qty_conversion_source": "generic:multiplier=1"
  }
}
```

Trade quantity note:

1. For TokenMM `trade_update` payloads, `trade.qty` is operator-facing base quantity.
2. When normalized trade exposure is available, `trade.qty_base`, `trade.qty_venue`, `trade.qty_conversion_status`, and `trade.qty_conversion_source` must accompany it.
3. Shared producer bare `qty` remains venue/native size; TokenMM socket projection is the layer that flips bare `qty` to the base-first operator contract.
4. Degraded conversion statuses such as `unsupported`, `missing_metadata`, `missing_price`, and `non_integral_venue_qty` are still valid normalized rows when `trade.qty_venue`, `trade.qty_conversion_status`, and `trade.qty_conversion_source` are present.
5. Only retained Redis trade rows missing normalized quantity fields are legacy compatibility rows.
6. Rollout still requires a TokenMM trade-stream cutover/reset before removing the last compatibility warning from production.

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
3. Canonical `tokenmm.trades` reconnects resume the standard subscription from the latest acknowledged surface cursor when fresh lineage is still available.
4. REST recovery is required only when lineage is missing, subscribe fails closed, `recovery_required.reason=trade_gap` arrives, `invalidate` / lineage mismatch occurs, or a seq gap is detected.
5. Non-canonical trades views remain REST snapshot mode and do not establish the standard subscription.
6. One reconnect should yield one bounded recovery sequence; duplicate unchanged recovery hints are additive only and must not create repeated snapshot churn by themselves.
7. Trade cursor fallback is deterministic when REST recovery runs:
   - prefer persisted `last_seq` for reconnect-safe replay
   - persist `(last_trade_ts_ms, last_trade_row_id, last_trade_version)`
   - dedupe by `row_id` with highest `version` winning
   - drop rows where tuple `(ts_ms, row_id, version)` is `<=` persisted cursor tuple
8. If `seq` gap is detected, client MUST run the same bounded REST resync.
9. `trade_update` idempotency key is `row_id` with highest `version` winning.
10. `signal_delta` applies only when `seq` is newer than the last applied `seq`.
