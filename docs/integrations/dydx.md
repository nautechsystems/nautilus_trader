# dYdX

dYdX is one of the largest decentralized cryptocurrency exchanges for crypto derivative products.
This integration supports live market data ingestion and order execution with dYdX v4, running on
its own Cosmos SDK application-specific blockchain (dYdX Chain) with CometBFT consensus. The order
book and matching engine run on-chain as part of the validator process. Orders are submitted as
Cosmos transactions via gRPC and settled each block. An Indexer service exposes REST and WebSocket
APIs for market data and account state.

## Installation

:::note
No additional installation extras are required. The adapter is implemented in Rust and
compiled into the core `nautilus_trader` package automatically during the build.
:::

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/dydx/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/dydx/examples/)

## Overview

This adapter is implemented in Rust with Python bindings via PyO3. It provides direct integration
with dYdX's Indexer API (REST/WebSocket) for market data and gRPC for Cosmos SDK transaction
submission, without requiring external client libraries.

### Product support

| Product Type      | Data Feed | Trading | Notes                                                      |
| ----------------- | --------- | ------- | ---------------------------------------------------------- |
| Perpetual Futures | ✓         | ✓       | All perpetuals are USDC‑settled.                           |
| Spot              | -         | -       | dYdX offers spot on Solana; not supported by this adapter. |
| Options           | -         | -       | *Not available on dYdX*.                                   |

:::note
This adapter supports perpetual futures only. All markets are quoted in USD and settled in USDC.
:::

## Chain architecture

Unlike centralized exchanges (CEXs) that expose a single REST/WebSocket API, dYdX v4 runs on its
own **Cosmos SDK application-specific blockchain**. This means every trade is a Cosmos transaction
that goes through consensus, and the adapter must manage sequences, gas, and block-height-based
expiration.

### Transport layers

The adapter communicates through three independent transport layers:

```
                         ┌─────────────────────────────────────────────┐
                         │              dYdX v4 Chain                  │
                         │                                             │
 ┌──────────┐  HTTP      │   ┌──────────────────────┐                  │
 │          │───────────►│   │  Indexer (read-only) │                  │
 │          │  WebSocket │   │  - REST API          │                  │
 │ Nautilus │───────────►│   │  - Streaming API     │                  │
 │ Adapter  │            │   └──────────────────────┘                  │
 │          │  gRPC      │   ┌──────────────────────┐                  │
 │          │───────────►│   │  Validator (write)   │                  │
 └──────────┘            │   │  - Cosmos Tx submit  │                  │
                         │   │  - Sequence mgmt     │                  │
                         │   └──────────────────────┘                  │
                         └─────────────────────────────────────────────┘
```

| Layer     | Target    | Direction | Purpose                                              |
| --------- | --------- | --------- | ---------------------------------------------------- |
| HTTP      | Indexer   | Read‑only | Instrument metadata, historical data, account state. |
| WebSocket | Indexer   | Read‑only | Real‑time market data, order/fill/position updates.  |
| gRPC      | Validator | Write     | Order placement, cancellation, and batch operations. |

### Block-based settlement

Trades settle on block commit, and short-term orders expire by block height rather than wall-clock
time. The adapter tracks block heights and timestamps from the WebSocket feed over a rolling
100-block window and estimates `seconds_per_block` from it, then uses that estimate to convert
time-based order expiry into block-height offsets.

Until five block samples have been collected, the estimate falls back to **500 ms** per block.
Observed mainnet block times run closer to one second, so the fallback understates the short-term
window and routes borderline orders to the long-term path rather than the reverse.

## Architecture

The dYdX v4 adapter includes multiple components which can be used together or separately:

- `DydxHttpClient`: HTTP client for Indexer REST API queries.
- `DydxWebSocketClient`: WebSocket client for real-time market data and account updates.
- `DydxGrpcClient`: gRPC client for Cosmos SDK transaction submission.
- `InstrumentCache`: Instrument parsing and loading, shared by the HTTP, WebSocket, and execution clients.
- `DydxDataClient`: Market data feed manager.
- `DydxExecutionClient`: Account management and trade execution gateway.
- `DydxDataClientFactory`: Factory for dYdX v4 data clients (used by the trading node builder).
- `DydxExecutionClientFactory`: Factory for dYdX v4 execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to work with these lower level components directly.
:::

:::warning[First-time account activation]
A dYdX v4 trading account (sub-account 0) is created only after the wallet's first deposit or trade.
Until then, gRPC and Indexer account queries return not-found, so `DydxExecutionClient.connect()`
fails while initializing the transaction sequence.

Before starting a `LiveNode`, send any positive amount of USDC or other supported collateral
from the same wallet on the same network (mainnet/testnet). Once the transaction has finalised
(a few blocks), restart the node and the client will connect cleanly.
:::

## Troubleshooting

### gRPC `NotFound` on connect

**Cause:** The wallet/sub-account has never been funded and therefore does not yet exist on-chain.

**Fix:**

1. Deposit any positive amount of USDC to sub-account 0 on the correct network.
2. Wait for finality (roughly 30 seconds on mainnet, longer on testnet).
3. Restart the `LiveNode`; the connection should now succeed.

:::tip
In unattended deployments, wrap the `connect()` call in an exponential-backoff loop so the
client retries until the deposit appears.
:::

## Symbology

dYdX uses specific symbol conventions for perpetual futures contracts.

### Symbol format

Format: `{Base}-USD-PERP`

All perpetuals on dYdX are:

- Quoted in USD
- Settled in USDC
- Use the `.DYDX` venue suffix in Nautilus

Examples:

- `BTC-USD-PERP.DYDX` - Bitcoin perpetual futures
- `ETH-USD-PERP.DYDX` - Ethereum perpetual futures
- `SOL-USD-PERP.DYDX` - Solana perpetual futures

To subscribe in your strategy:

```python
InstrumentId.from_str("BTC-USD-PERP.DYDX")
InstrumentId.from_str("ETH-USD-PERP.DYDX")
```

:::info
The dYdX Indexer ticker for a perpetual is `{Base}-USD` (for example `BTC-USD`). The adapter appends
the `-PERP` suffix for consistency with other adapters and to leave room for other product types.
:::

## Orders capability

dYdX supports perpetual futures trading with a full set of order types and execution
features. The adapter automatically classifies each order as short‑term, long‑term, or conditional
from its type, time-in-force, and expiry, so no manual tagging is needed.

### Order types

| Order Type             | Perpetuals | Notes                                              |
| ---------------------- | ---------- | -------------------------------------------------- |
| `MARKET`               | ✓          | Immediate execution at best available price.       |
| `LIMIT`                | ✓          |                                                    |
| `STOP_MARKET`          | ✓          | Stop‑loss conditional order, always stateful.      |
| `STOP_LIMIT`           | ✓          | Conditional order, always stateful.                |
| `MARKET_IF_TOUCHED`    | ✓          | Take‑profit market order, triggers on price touch. |
| `LIMIT_IF_TOUCHED`     | ✓          | Take‑profit limit order, triggers on price touch.  |
| `TRAILING_STOP_MARKET` | -          | *Not supported*.                                   |
| `TRAILING_STOP_LIMIT`  | -          | *Not supported*.                                   |

### Execution instructions

| Instruction   | Perpetuals | Notes                                                                                                                                                                                          |
| ------------- | ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `post_only`   | ✓          | Supported on LIMIT, STOP_LIMIT, and LIMIT_IF_TOUCHED orders. A post‑only order priced to cross the spread is **accepted then immediately canceled** by the venue (not rejected with a reason). |
| `reduce_only` | ✓          | Accepted by the chain **only on orders that execute immediately** (IOC). Anything else is rejected on‑chain with `code=9003`, `Reduce-only is currently disabled for non-IOC orders`.          |

How the adapter handles the flag depends on the order type:

| Order type                                | `reduce_only` behavior                                                                  |
| ----------------------------------------- | --------------------------------------------------------------------------------------- |
| `LIMIT`, `STOP_LIMIT`, `LIMIT_IF_TOUCHED` | Forwarded with your time in force. Use `IOC` or the chain rejects it.                   |
| `MARKET`                                  | Dropped. The order fills like an ordinary market order and can open or flip a position. |
| `STOP_MARKET`, `MARKET_IF_TOUCHED`        | Forwarded, but these carry no time in force, so the chain always rejects them.          |

Set `reduce_only` only on the first group, and only together with `IOC`.

### Time in force options

| Time in force | Perpetuals | Notes                                                                                                                                                                |
| ------------- | ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `GTC`         | ✓          | Good Till Canceled.                                                                                                                                                  |
| `GTD`         | ✓          | Good Till Date. The venue reports expiry as a cancel event; the adapter maps this to `OrderExpired` (not `OrderCanceled`) when the order's `expire_time` has passed. |
| `IOC`         | ✓          | Immediate or Cancel.                                                                                                                                                 |
| `FOK`         | -          | *Deprecated by dYdX v4*. The chain rejects FOK orders with `code=48`; the adapter generates `OrderDenied` locally and does not broadcast.                            |
| `DAY`         | -          | *Not supported*. The adapter generates `OrderDenied` locally and does not broadcast.                                                                                 |

### Advanced order features

| Feature            | Perpetuals | Notes                                                                                                                                                                                     |
| ------------------ | ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Order modification | -          | Not supported. dYdX supports short‑term order [replacement](https://docs.dydx.xyz/concepts/trading/limit-orderbook#replacements) (same ID, higher GTB); not yet exposed as `ModifyOrder`. |
| Bracket/OCO orders | -          | *Not supported*.                                                                                                                                                                          |
| Iceberg orders     | -          | *Not supported*.                                                                                                                                                                          |

### Batch operations

| Operation    | Perpetuals | Notes                                                                                                                  |
| ------------ | ---------- | ---------------------------------------------------------------------------------------------------------------------- |
| Batch submit | ✓          | Supported for long‑term `LIMIT` orders. Short‑term orders are submitted individually.                                  |
| Batch modify | -          | *Not supported*.                                                                                                       |
| Batch cancel | ✓          | Partitioned: short‑term orders use `MsgBatchCancel` (single gRPC call), long‑term orders use batched `MsgCancelOrder`. |

### Position management

| Feature          | Perpetuals | Notes                                                           |
| ---------------- | ---------- | --------------------------------------------------------------- |
| Query positions  | ✓          | Real‑time position updates.                                     |
| Position mode    | -          | Netting only (see below).                                       |
| Leverage control | -          | Set by each market's margin fractions; no per‑account override. |
| Margin mode      | -          | Cross margin only.                                              |

:::note
dYdX nets positions (one position per instrument) at the venue level, so the adapter operates in
`NETTING` mode only.
:::

### Order querying

| Feature              | Perpetuals | Notes                          |
| -------------------- | ---------- | ------------------------------ |
| Query open orders    | ✓          | List all active orders.        |
| Query order history  | ✓          | Historical order data.         |
| Order status updates | ✓          | Real‑time order state changes. |
| Trade history        | ✓          | Execution and fill reports.    |

### Contingent orders

| Feature            | Perpetuals | Notes                                            |
| ------------------ | ---------- | ------------------------------------------------ |
| Order lists        | -          | *Not supported*.                                 |
| OCO orders         | -          | *Not supported*.                                 |
| Bracket orders     | -          | *Not supported*.                                 |
| Conditional orders | ✓          | Stop, take‑profit market, and take‑profit limit. |

### Equity tier limit

dYdX caps how many **stateful orders** (long‑term and conditional) a subaccount may hold open at
once, based on the subaccount's net collateral. Short‑term orders are exempt from the cap.
Submitting past the cap is rejected on‑chain with `code=10001` and a log message of the form
`Opening order would exceed equity tier limit of N`. Cancel existing stateful orders before placing
more, or split strategies across subaccounts.

| Net collateral      | Maximum open stateful orders |
| ------------------- | ---------------------------- |
| Under $20           | 0                            |
| $20 to $100         | 10                           |
| $100 to $1,000      | 20                           |
| $1,000 to $10,000   | 40                           |
| $10,000 to $100,000 | 100                          |
| $100,000 and above  | 200                          |

The tiers are governance-adjustable. Query the live values from a node's
`/dydxprotocol/clob/equity_tier` endpoint, or see
[equity tier limits](https://docs.dydx.xyz/concepts/trading/limits/equity-tier-limits).

### MIT and LIT round-tripping

dYdX's protocol uses a single `TAKE_PROFIT` order type with a price (`subticks`) and trigger
price; whether it behaves as market‑on‑trigger or limit‑on‑trigger is implicit in the price. The
adapter submits Nautilus `MARKET_IF_TOUCHED` as a take‑profit with the price set to the 5%
pay‑through worst‑case, and `LIMIT_IF_TOUCHED` as a take‑profit at the user's limit price. Both
forms are returned by the Indexer as `"type":"TAKE_PROFIT"`.

On reconciliation, the adapter recovers the original Nautilus order type from how far the reported
price sits from the trigger price. A drift of **2% or more** means the price came from the 5%
pay‑through buffer, so the order is reconciled as `MARKET_IF_TOUCHED`; anything closer is treated as
a user‑chosen limit and reconciled as `LIMIT_IF_TOUCHED`. The 2% threshold separates the pay‑through
band from typical take‑profit limit offsets, which sit well under 1%.

### Liquidation and ADL (deleveraging) handling

dYdX v4 applies two sequential risk mechanisms:

1. **Liquidation** runs when an account drops below its maintenance margin.
   Positions close against the insurance fund within a bounded spread from the
   oracle price.
2. **Deleveraging (ADL)** activates when either liquidation cannot fully
   restore collateralisation, or when a large oracle jump drives an account
   negative in a single step. Deleveraging closes the undercollateralised
   position against randomly selected offsetting accounts.

The Indexer exposes the classification via the `type` field on each `Fill` record:

| `type`        | Meaning                                            |
| ------------- | -------------------------------------------------- |
| `LIMIT`       | Normal fill.                                       |
| `LIQUIDATED`  | Taker side of a liquidation (undercollateralised). |
| `LIQUIDATION` | Maker side of a liquidation (insurance fund).      |
| `DELEVERAGED` | Taker side of a deleveraging (ADL closure).        |
| `OFFSETTING`  | Maker side of a deleveraging (offsetting account). |

Any other value the venue introduces is decoded as an unknown fill type and handled like a normal
fill, so a new classification never drops the fill.

The adapter logs a warning with instrument, side, size, and price for each
liquidation / deleveraging fill, then emits the `FillReport` through the
normal path. A position the venue reports as `LIQUIDATED` is treated as closed,
which closes out the corresponding position report.

Upstream references:

- [Liquidations](https://docs.dydx.xyz/concepts/trading/liquidations)
- [Contract loss mechanisms (deleveraging)](https://help.dydx.trade/en/articles/166973-contract-loss-mechanisms-on-dydx-chain)

### Order classification

dYdX classifies every order into one of three on‑chain categories. The adapter
automatically determines the category based on time-in-force and expiry, so no manual
configuration is required.

| Category    | Placement | Expiry          | Typical use                                       |
| ----------- | --------- | --------------- | ------------------------------------------------- |
| Short‑term  | In‑memory | Block height    | IOC, or orders expiring within 40 blocks.         |
| Long‑term   | On‑chain  | Timestamp (UTC) | GTC/GTD with expiry beyond the short‑term window. |
| Conditional | On‑chain  | Timestamp (UTC) | Stop‑loss and take‑profit triggers.               |

At the protocol level, **all dYdX orders are limit orders**. The `MARKET` order type
is a Nautilus convenience that the adapter implements as an aggressive IOC limit order
priced well through the book. This means market orders follow the same
`Submitted > Accepted > Filled` lifecycle as limit orders (an `OrderAccepted` event is
expected before the fill).

See the [dYdX order documentation](https://docs.dydx.xyz/concepts/trading/orders)
for full protocol-level details on short-term vs stateful order mechanics.

#### Short-term orders

Short-term orders live **in validator memory only** and expire by block height. The protocol's
`ShortBlockWindow` caps their lifetime at **40 blocks** past the current height. They are the
fastest order type on dYdX because they skip on-chain storage.

**Properties**:

- **IOC (and the deprecated FOK) are always short-term**, regardless of other parameters
- **GTD orders** are automatically classified as short-term when the expiry falls within the
  dynamic short-term window (`40 blocks × seconds_per_block`)
- Use Good-Til-Block (GTB) for replay protection instead of Cosmos SDK sequences
- Can be broadcast **concurrently** (no semaphore, cached sequence)
- Expire silently without generating cancel events
- Cannot be batched in a single transaction (one `MsgPlaceOrder` per tx)

#### Long-term orders

Long-term (stateful) orders are **stored on-chain** and expire by UTC timestamp. They generate
explicit cancel events when they expire or are cancelled.

**Properties**:

- **GTC** orders default to 90-day expiration (protocol limit is 95 days)
- **GTD** orders use the user-provided expiry timestamp
- Require proper Cosmos SDK sequence management (serialized via semaphore)
- Must be broadcast **serially** with incrementing sequence numbers
- Can be batched in a single transaction

#### Conditional orders

Conditional orders (stop-loss, take-profit) are **always stored on-chain** and triggered by
price conditions on the validator.

**Properties**:

- Always use timestamp-based expiry (default 90 days for GTC, protocol limit 95 days)
- Always use the long-term broadcast path (serialized with semaphore)
- Include `StopMarket`, `StopLimit`, `TakeProfitMarket`, and `TakeProfitLimit`

#### Automatic routing

The adapter determines order lifetime automatically from the estimated block time:

```
max_short_term_secs = 40 blocks (ShortBlockWindow) × seconds_per_block
```

If the order's time until expiry is within `max_short_term_secs`, it is routed as short-term.
Otherwise, it is routed as long-term. No manual configuration is needed.

#### MARKET order implementation

dYdX has no native market order type. The adapter implements `MARKET` orders as aggressive
**IOC limit orders** priced at:

- **Buy**: `oracle_price × (1 + 0.05)` (5% above oracle)
- **Sell**: `oracle_price × (1 - 0.05)` (5% below oracle)

This 5% slippage buffer (`DEFAULT_MARKET_ORDER_SLIPPAGE = 0.05`) sets the worst-case price
(the "pay-through price"). Because the order is IOC, unfilled slippage is not consumed. The
buffer is intentionally wide to maximize fill probability across volatile conditions.

### Client order ID encoding

dYdX requires `u32` client IDs on-chain, but Nautilus uses string-based `ClientOrderId` values
(e.g., `O-20260220-031943-001-000-51`). The adapter encodes these bidirectionally so that orders
can be reconciled across restarts without persisted state.

For the standard O-format (`O-YYYYMMDD-HHMMSS-TTT-SSS-CCC`), the encoding is deterministic:

| dYdX field        | Bits | Contents                                           |
| ----------------- | ---- | -------------------------------------------------- |
| `client_id`       | 32   | `[trader:10][strategy:10][count:12]` (unique key). |
| `client_metadata` | 32   | Seconds since 2020-01-01 UTC (timestamp).          |

Because the encoding is deterministic, the adapter can decode any reconciled order back to its
original `ClientOrderId` string without needing a database or mapping file.

A `ClientOrderId` that is a plain number is also deterministic: the number becomes `client_id` and
`client_metadata` is set to a fixed marker, so it decodes across restarts as well. Any other format
falls back to sequential allocation with an in-memory reverse map, and those IDs can only be decoded
within the same session.

#### Restart collision prevention

On restart, Nautilus resets the internal order counter based on the number of reconciled orders,
which may be lower than the highest counter value used in the previous session (e.g., if some
orders have expired from the API response). This can cause a new order to produce the same
`client_id` as a previous session's order, resulting in a duplicate venue order UUID.

The adapter prevents this by registering every `client_id` seen during reconciliation. If a new
O-format encoding produces a `client_id` that was already used, the encoder logs a warning and
falls back to sequential allocation. Sequential allocation also skips any registered values.

:::note
This protection is automatic and requires no user configuration. The warning log
`[ENCODER] client_id ... collides with reconciled order` is informational. The order will
still be submitted successfully with an alternative ID.
:::

## Broadcasting and retry strategy

### Short-term broadcast

Short-term orders use Good-Til-Block (GTB) for replay protection. The chain's `ClobDecorator`
ante handler skips Cosmos SDK sequence checking for short-term messages, so:

- **No semaphore**: broadcasts are fully concurrent
- **Cached sequence**: no increment or allocation needed
- **No retry**: if the broadcast fails, it fails immediately
- Benign cancel errors are treated as success (see below)

### Long-term broadcast

Long-term and conditional orders require proper Cosmos SDK sequence management:

- **Semaphore** with 1 permit serializes all long-term broadcasts
- **Exponential backoff**: 500ms -> 1s -> 2s -> 4s (max 5 retries)
- **10-second total budget** prevents indefinite retry loops
- On sequence mismatch, the sequence is **resynced from chain** before retry
- Transient gRPC failures (unavailable, deadline exceeded, resource exhausted) also resync before
  retry, so repeated timeouts cannot drift the local sequence ahead of the chain

### Sequence mismatch detection

| Error code | Source             | Meaning                                          |
| ---------- | ------------------ | ------------------------------------------------ |
| `code=32`  | Cosmos SDK         | Account sequence mismatch                        |
| `code=104` | dYdX authenticator | Signature verification failed (sequence‑related) |

Both trigger automatic resync + retry via the `RetryManager`.

### Benign cancel errors

These errors during short-term cancel operations are treated as **success**:

| Error code  | Meaning                                                           |
| ----------- | ----------------------------------------------------------------- |
| `code=19`   | Transaction already in mempool cache (duplicate tx)               |
| `code=9`    | Cancel already exists in memclob with >= GoodTilBlock             |
| `code=3006` | Order to cancel does not exist (already filled/expired/cancelled) |

### Batch cancel partitioning

When cancelling multiple orders, the adapter partitions them by lifetime:

1. **Short-term orders**: single `MsgBatchCancel` via `broadcast_short_term()`
2. **Long-term orders**: batched `MsgCancelOrder` messages via `broadcast_with_retry()`

This ensures each group uses the appropriate broadcast strategy.

## Funding rates

dYdX perpetual futures use a fixed 1-hour funding interval. The adapter sets `interval`
to `60` (minutes) on all `FundingRateUpdate` objects for both WebSocket and historical
funding data.

## Rate limiting

### gRPC rate limiting

The adapter rate-limits gRPC `broadcast_tx` calls to prevent `ResourceExhausted` (429) errors
from validator nodes.

| Setting                      | Default | Description                                                           |
| ---------------------------- | ------- | --------------------------------------------------------------------- |
| `grpc_rate_limit_per_second` | `4`     | Maximum gRPC broadcast requests per second. Set to `None` to disable. |

This is a config-struct field, not a parameter of the Python `DydxExecClientConfig` constructor.

### Provider limits

Known rate limits for public gRPC providers:

| Provider  | Limit                |
| --------- | -------------------- |
| Polkachu  | 300 req/min (~5/s)   |
| KingNodes | 250 req/min (~4.2/s) |
| AutoStake | 4 req/s              |

The default of 4 req/s is conservative and works across all public providers.

### Multiple gRPC URL fallback

The adapter connects to the first reachable node in a list of gRPC URLs, falling back to the next
one when a connection fails. This matters on a DEX, where individual public nodes go down without
notice. The execution config resolves that list in order:

1. `grpc_urls`, when non-empty.
2. `grpc_endpoint`, as a single-URL list. Setting only this field gives up the fallback.
3. The default public validator nodes for the selected network.

Both fields are config-struct fields and are not parameters of the Python `DydxExecClientConfig`
constructor, so Python configs always get the network defaults with their built-in fallback.

## Price and size quantization

dYdX uses integer-based quantization for prices and sizes. The adapter handles all conversions
automatically via `OrderMessageBuilder`, but understanding the parameters helps with debugging.

### Market parameters

| Parameter                     | Description                                             |
| ----------------------------- | ------------------------------------------------------- |
| `atomic_resolution`           | Exponent for converting human‑readable size to quantums |
| `quantum_conversion_exponent` | Exponent for converting quantums to tokens              |
| `step_base_quantums`          | Minimum order size step in quantums                     |
| `subticks_per_tick`           | Price granularity within each tick                      |

### Market order pricing

Orders submitted without an explicit price use the oracle price with a 5% slippage buffer (the
"pay-through price"). This covers `MARKET`, `STOP_MARKET`, and `MARKET_IF_TOUCHED`:

- **Buy**: `oracle_price × 1.05`
- **Sell**: `oracle_price × 0.95`

Order pricing reads the oracle price from the instrument cache, which the Indexer populates when the
client connects and does not refresh afterwards, so the pay-through band stays anchored to the
oracle price observed at connect time. This is a separate path from the live oracle prices the
execution client tracks off the markets channel, which it uses to value account state and positions
rather than to price orders.

### Automatic handling

All price and size quantization is handled automatically by `OrderMessageBuilder`.
No manual conversion is needed when submitting orders through Nautilus.

## Data subscriptions

The adapter supports the following data subscriptions:

| Data type            | Subscription | Historical request | Notes                                            |
| -------------------- | ------------ | ------------------ | ------------------------------------------------ |
| Trade ticks          | ✓            | ✓                  |                                                  |
| Quote ticks          | ✓            | -                  | Synthesized from order book top‑of‑book.         |
| Order book deltas    | ✓            | -                  | L2 depth only.                                   |
| Order book snapshots | -            | ✓                  | One‑time snapshot via HTTP request.              |
| Bars                 | ✓            | ✓                  | See supported resolutions below.                 |
| Mark prices          | ✓            | -                  | Via markets channel.                             |
| Index prices         | ✓            | -                  | Via markets channel.                             |
| Funding rates        | ✓            | ✓                  | Real‑time via markets channel, history via HTTP. |
| Instrument status    | ✓            | -                  | Via markets channel.                             |

### Supported bar resolutions

| Resolution | dYdX candle |
| ---------- | ----------- |
| 1-MINUTE   | `1MIN`      |
| 5-MINUTE   | `5MINS`     |
| 15-MINUTE  | `15MINS`    |
| 30-MINUTE  | `30MINS`    |
| 1-HOUR     | `1HOUR`     |
| 4-HOUR     | `4HOURS`    |
| 1-DAY      | `1DAY`      |

## Subaccounts

dYdX supports multiple subaccounts per wallet address, allowing segregation of trading strategies
and risk management within a single wallet.

### Concepts

- Each wallet address can have multiple numbered subaccounts (0, 1, 2, ..., 127). Numbers 128 and
  above are the venue's isolated-margin child subaccounts, which this adapter does not support.
- Subaccount 0 is the **default** and is automatically created on first deposit.
- Each subaccount maintains its own:
  - Positions
  - Open orders
  - Collateral balance
  - Margin requirements

### Configuration

Specify the subaccount number in the execution client config:

```python
from nautilus_trader.adapters.dydx import DydxExecClientConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


exec_config = DydxExecClientConfig(
    trader_id=TraderId.from_str("TRADER-001"),
    account_id=AccountId.from_str("DYDX-001"),
    subaccount_number=0,
)
```

:::note
Most users will use subaccount `0` (the default). Advanced users can configure multiple execution
clients for different subaccounts to implement strategy segregation or risk isolation.
:::

## Testnet setup

The dYdX testnet (`dydx-testnet-4`) is a full replica of mainnet for testing strategies
without risking real funds. All default testnet endpoints are resolved automatically when
`network=DydxNetwork.TESTNET`.

### 1. Create a testnet wallet

**Option A: Via the dYdX testnet web app (easiest)**

1. Go to [v4.testnet.dydx.exchange](https://v4.testnet.dydx.exchange)
2. Connect with MetaMask, Keplr, Phantom, or WalletConnect
3. A dYdX account is generated automatically
4. Export your secret phrase: click your address (top-right) and select "Export secret phrase"

**Option B: Use an existing secp256k1 private key**

Any 32-byte hex-encoded secp256k1 private key will work. The adapter derives the `dydx1...`
address from the key automatically using Cosmos bech32 encoding.

### 2. Fund the testnet account

A subaccount must be funded before the adapter can connect (see [First-time account activation](#architecture)).

**Via the testnet web app:**

Click the deposit/recharge button on [v4.testnet.dydx.exchange](https://v4.testnet.dydx.exchange)
to receive testnet USDC automatically.

**Via the faucet API directly:**

```bash
# Fund subaccount 0 with 2000 USDC
curl -X POST https://faucet.v4testnet.dydx.exchange/faucet/tokens \
  -H "Content-Type: application/json" \
  -d '{"address": "dydx1...", "subaccountNumber": 0, "amount": 2000}'

# Fund native tokens (for gas fees)
curl -X POST https://faucet.v4testnet.dydx.exchange/faucet/native-token \
  -H "Content-Type: application/json" \
  -d '{"address": "dydx1..."}'
```

### 3. Set environment variables

```bash
export DYDX_TESTNET_WALLET_ADDRESS="dydx1..."
export DYDX_TESTNET_PRIVATE_KEY="0x..."  # hex-encoded, 0x prefix optional
```

### 4. Configure the trading node

Set `network=DydxNetwork.TESTNET` on both data and execution clients:

```python
from nautilus_trader.adapters.dydx import DydxDataClientConfig
from nautilus_trader.adapters.dydx import DydxExecClientConfig
from nautilus_trader.adapters.dydx import DydxNetwork
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


data_config = DydxDataClientConfig(network=DydxNetwork.TESTNET)

exec_config = DydxExecClientConfig(
    trader_id=TraderId.from_str("TRADER-001"),
    account_id=AccountId.from_str("DYDX-001"),
    network=DydxNetwork.TESTNET,
    wallet_address=None,  # Falls back to DYDX_TESTNET_WALLET_ADDRESS
    private_key=None,  # Falls back to DYDX_TESTNET_PRIVATE_KEY
    subaccount_number=0,
)
```

### Testnet endpoints

The Python constructors select the default testnet endpoints automatically and do not expose
endpoint overrides.

| Service   | Default URL                                          |
| --------- | ---------------------------------------------------- |
| HTTP      | `https://indexer.v4testnet.dydx.exchange`            |
| WebSocket | `wss://indexer.v4testnet.dydx.exchange/v4/ws`        |
| gRPC      | `https://test-dydx-grpc.kingnodes.com:443` (primary) |
| Faucet    | `https://faucet.v4testnet.dydx.exchange`             |
| Web app   | `https://v4.testnet.dydx.exchange`                   |

### Mainnet endpoints

The Python constructors select the default mainnet endpoints automatically and do not expose
endpoint overrides.

| Service   | Default URL                                         |
| --------- | --------------------------------------------------- |
| HTTP      | `https://indexer.dydx.trade`                        |
| WebSocket | `wss://indexer.dydx.trade/v4/ws`                    |
| gRPC      | `https://dydx-ops-grpc.kingnodes.com:443` (primary) |

## Configuration

Configure the dYdX adapter through the trading node configuration. Execution clients support
environment variable fallbacks for credentials. Data clients use public endpoints and do not require
wallet credentials.

### Data client configuration options

| Option      | Default   | Description                                     |
| ----------- | --------- | ----------------------------------------------- |
| `network`   | `MAINNET` | `DydxNetwork.MAINNET` or `DydxNetwork.TESTNET`. |
| `proxy_url` | `None`    | Optional proxy URL for HTTP and WebSocket use.  |

### Execution client configuration options

| Option              | Default   | Description                                                                       |
| ------------------- | --------- | --------------------------------------------------------------------------------- |
| `trader_id`         | Required  | Nautilus trader ID for the client.                                                |
| `account_id`        | Required  | Nautilus account ID for the client.                                               |
| `network`           | `MAINNET` | `DydxNetwork.MAINNET` or `DydxNetwork.TESTNET`.                                   |
| `private_key`       | `None`    | Hex‑encoded signing key; falls back to the network‑specific environment variable. |
| `wallet_address`    | `None`    | dYdX wallet address; falls back to the network‑specific environment variable.     |
| `subaccount_number` | `0`       | Subaccount number from `0` through `127`.                                         |
| `proxy_url`         | `None`    | Optional proxy URL for HTTP and WebSocket use.                                    |

### Basic setup

Register `DydxDataClientConfig` with `DydxDataClientFactory` and `DydxExecClientConfig` with
`DydxExecutionClientFactory` on the node builder. The
[Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/dydx/)
show the complete `LiveNode.builder(...)` wiring for both clients.

### API credentials

Credentials can be passed directly via the Python config (`wallet_address`, `private_key`) or
resolved automatically from environment variables based on the configured `network`.

#### Environment variables

| Variable                      | Network | Description                                    |
| ----------------------------- | ------- | ---------------------------------------------- |
| `DYDX_WALLET_ADDRESS`         | Mainnet | Bech32-encoded wallet address (`dydx1...`).    |
| `DYDX_PRIVATE_KEY`            | Mainnet | Hex‑encoded secp256k1 private key for signing. |
| `DYDX_TESTNET_WALLET_ADDRESS` | Testnet | Testnet wallet address (`dydx1...`).           |
| `DYDX_TESTNET_PRIVATE_KEY`    | Testnet | Testnet private key.                           |

#### Resolution priority

1. Value passed in the Python config (if non-empty)
2. Environment variable selected by `network`

### Permissioned key trading

#### What are API Trading Keys

API Trading Keys let you delegate trading to a separate signing key without sharing your main
wallet's seed phrase. The API key can place trades using all available margin in the owner's
cross-margin account, but cannot withdraw funds or transfer assets.

#### Creating an API key

1. In the dYdX web app, navigate to **More > API Trading Keys**
2. Click **Generate New API Key**
3. Save the **API Wallet Address** and **Private Key** (shown once, not stored by dYdX)
4. Click **Authorize API Key** (this registers the key on-chain as an authenticator)
5. The key is now active and can be used for trading

See the [dYdX permissioned keys documentation](https://docs.dydx.xyz/interaction/permissioned-keys)
for the authenticator model, and the
[front-end walkthrough](https://help.dydx.trade/en/articles/267486-api-trading-keys-creating-a-new-key-on-the-front-end)
for creating and managing keys in the web app.

#### Adapter configuration

Set the API key's private key as `DYDX_PRIVATE_KEY` and the
owner's wallet address as `DYDX_WALLET_ADDRESS`. The adapter detects the mismatch during connect
and automatically queries the chain for matching authenticator IDs.

```python
from nautilus_trader.adapters.dydx import DydxExecClientConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


config = DydxExecClientConfig(
    trader_id=TraderId.from_str("TRADER-001"),
    account_id=AccountId.from_str("DYDX-001"),
    wallet_address="dydx1owner...",  # Owner account (holds margin)
    private_key="0xapikey...",  # API Trading Key private key
)
```

The public Python config does not accept manual authenticator IDs.

:::note
API Trading Keys only work with **cross-margin** accounts and cross markets. Isolated margin
is not supported.
:::

## Order books

Order books can be maintained at full depth or top-of-book quotes depending on the subscription.
The venue does not provide quotes directly. Instead, the adapter subscribes to order book deltas
and synthesizes quotes for the `DataEngine` when there is a top-of-book price or size change.
Only L2 (MBP) book type is supported.

## Contributing

:::info
For additional features or to contribute to the dYdX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
