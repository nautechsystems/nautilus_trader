# Binance

Founded in 2017, Binance is one of the largest cryptocurrency exchanges in terms
of daily trading volume, and open interest of crypto assets and crypto
derivative products.

NautilusTrader provides Binance integration in both Python and Rust. The Rust
adapter supports all product types listed below and includes additional
features (noted inline). The Python adapter supports the same product types.

Supported products:

- **Binance Spot** (including Binance US)
- **Binance USDT-Margined Futures** (perpetuals and delivery contracts)
- **Binance Coin-Margined Futures** (perpetuals and delivery contracts)

## Examples

- [Python live examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/binance/)
- [Rust spot examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/binance/examples/spot/)
- [Rust futures examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/binance/examples/futures/)

## Overview

The Binance adapter includes multiple components that can be used together or separately:

- `BinanceHttpClient`: Low-level HTTP API connectivity.
- `BinanceWebSocketClient`: Low-level WebSocket API connectivity.
- `BinanceInstrumentProvider`: Instrument parsing and loading.
- `BinanceSpotDataClient` / `BinanceFuturesDataClient`: Market data feed manager.
- `BinanceSpotExecutionClient` / `BinanceFuturesExecutionClient`: Account management and trade execution gateway.
- `BinanceLiveDataClientFactory`: Factory for Binance data clients (used by the trading node builder).
- `BinanceLiveExecClientFactory`: Factory for Binance execution clients (used by the trading node builder).

:::note
Most users configure a live trading node (as below) and do not interact with
these lower-level components directly.
:::

### Product support

| Product Type                            | Supported | Notes                              |
|-----------------------------------------|-----------|------------------------------------|
| Spot Markets (incl. Binance US)         | ✓         |                                    |
| Margin Accounts (Cross & Isolated)      | -         | *Not implemented.* Planned for v2. |
| USDT-Margined Futures (PERP & Delivery) | ✓         |                                    |
| Coin‑Margined Futures                   | ✓         |                                    |

:::note
Margin account features (borrow, repay, isolated margin management) are not implemented.
The Python adapter will not add margin support. Full margin trading support is planned for v2.
:::

## Data types

The integration includes several custom data types:

- `BinanceTicker`: 24-hour ticker data including price and statistical information.
- `BinanceBar`: Bar data with additional volume metrics for historical and real-time use.
- `BinanceFuturesMarkPriceUpdate`: Mark price updates for Binance Futures.

See the Binance [API Reference](/docs/python-api-latest/adapters/binance.html) for full definitions.

## Symbology

Native Binance symbols are used where possible for spot and futures contracts.
Because NautilusTrader supports multi-venue trading, it must distinguish between
`BTCUSDT` the spot pair and `BTCUSDT` the perpetual futures contract (Binance
uses the same symbol for both).

Nautilus appends the `-PERP` suffix to all perpetual symbols. For example,
the Binance Futures `BTCUSDT` perpetual contract becomes `BTCUSDT-PERP`
within Nautilus.

## Order capability

The following tables detail order types, execution instructions, and
time-in-force options across Binance account types.

### Order types

| Order Type             | Spot | Margin | USDT Futures | Coin Futures | Notes                              |
|------------------------|------|--------|--------------|--------------|------------------------------------|
| `MARKET`               | ✓    | -      | ✓            | ✓            | Quote quantity support: Spot only. |
| `LIMIT`                | ✓    | -      | ✓            | ✓            |                         |
| `STOP_MARKET`          | -    | -      | ✓            | ✓            | Futures only.           |
| `STOP_LIMIT`           | ✓    | -      | ✓            | ✓            |                         |
| `MARKET_IF_TOUCHED`    | -    | -      | ✓            | ✓            | Futures only.           |
| `LIMIT_IF_TOUCHED`     | ✓    | -      | ✓            | ✓            |                         |
| `TRAILING_STOP_MARKET` | -    | -      | ✓            | ✓            | Futures only.           |

### Execution instructions

| Instruction   | Spot | Margin | USDT Futures | Coin Futures | Notes                                 |
|---------------|------|--------|--------------|--------------|---------------------------------------|
| `post_only`   | ✓    | -      | ✓            | ✓            | See restrictions below.               |
| `reduce_only` | -    | -      | ✓            | ✓            | Futures only; disabled in Hedge Mode. |

#### Post-only restrictions

Only *limit* order types support `post_only`.

| Order Type               | Spot | Margin | USDT Futures | Coin Futures | Notes                                               |
|--------------------------|------|--------|--------------|--------------|-----------------------------------------------------|
| `LIMIT`                  | ✓    | -      | ✓            | ✓            | Uses `LIMIT_MAKER` for Spot, `GTX` TIF for Futures. |
| `STOP_LIMIT`             | -    | -      | ✓            | ✓            | Futures only.                                       |

### Time in force

| Time in force | Spot | Margin | USDT Futures | Coin Futures | Notes                                      |
|---------------|------|--------|--------------|--------------|--------------------------------------------|
| `GTC`         | ✓    | -      | ✓            | ✓            | Good Till Canceled.                        |
| `GTD`         | ✓*   | -      | ✓            | ✓            | *Converted to GTC for Spot with warning.   |
| `FOK`         | ✓    | -      | ✓            | ✓            | Fill or Kill.                              |
| `IOC`         | ✓    | -      | ✓            | ✓            | Immediate or Cancel.                       |

### Advanced order features

| Feature            | Spot | Margin | USDT Futures | Coin Futures | Notes                                        |
|--------------------|------|--------|--------------|--------------|----------------------------------------------|
| Order Modification | ✓    | -      | ✓            | ✓            | Price and quantity for `LIMIT` orders only.  |
| Bracket/OCO Orders | -    | -      | -            | -            | *Planned*. Currently denied at submission.   |
| Iceberg Orders     | ✓    | -      | ✓            | ✓            | Large orders split into visible portions.    |

### Batch operations

| Operation          | Spot | Margin | USDT Futures | Coin Futures | Notes                                        |
|--------------------|------|--------|--------------|--------------|----------------------------------------------|
| Batch Submit       | ✓    | -      | ✓            | ✓            | Orders submitted individually (no batch API call). |
| Batch Modify       | -    | -      | -            | -            | Not implemented.                             |
| Batch Cancel       | -*   | -      | ✓            | ✓            | *Spot falls back to individual cancels.      |

#### Cancel all orders behavior

When calling `cancel_all_orders()` from a strategy, the adapter includes
orders in both open and inflight (SUBMITTED) states so that the adapter also
cancels orders not yet acknowledged by Binance.

**Multi-strategy safety**: When multiple strategies trade the same instrument,
the adapter compares orders owned by the requesting strategy against all orders
for that instrument. If the strategy owns all orders, a single cancel-all API
call is used. Otherwise, per-strategy cancels are sent (batch for regular
orders, individual for algo orders) to avoid affecting other strategies.

**Futures algo orders**: Conditional order types (`STOP_MARKET`, `STOP_LIMIT`,
`TAKE_PROFIT`, `TAKE_PROFIT_MARKET`, `TRAILING_STOP_MARKET`) require a
different cancel endpoint. The adapter routes these through the correct
endpoint automatically. Once an algo order triggers and becomes a regular
order, it uses the standard cancel endpoint.

**Endpoints used**:

| Account Type | Regular Orders                  | Algo Orders (batch)              | Algo Orders (individual)    |
|--------------|---------------------------------|----------------------------------|-----------------------------|
| Spot/Margin  | `DELETE /api/v3/openOrders`     | N/A                              | N/A                         |
| USDT Futures | `DELETE /fapi/v1/allOpenOrders` | `DELETE /fapi/v1/algoOpenOrders` | `DELETE /fapi/v1/algoOrder` |
| Coin Futures | `DELETE /dapi/v1/allOpenOrders` | `DELETE /dapi/v1/algoOpenOrders` | `DELETE /dapi/v1/algoOrder` |

### Position management

| Feature             | Spot | Margin | USDT Futures | Coin Futures | Notes                                       |
|---------------------|------|--------|--------------|--------------|---------------------------------------------|
| Query positions     | -    | -      | ✓            | ✓            | Real‑time position updates.                 |
| Position mode       | -    | -      | ✓            | ✓            | One‑Way vs Hedge mode (position IDs).       |
| Leverage control    | -    | -      | ✓            | ✓            | Dynamic leverage adjustment per symbol.     |
| Margin mode         | -    | -      | ✓            | ✓            | Cross vs Isolated margin per symbol.        |

### Risk events

| Feature              | Spot | Margin | USDT Futures | Coin Futures | Notes                                       |
|----------------------|------|--------|--------------|--------------|---------------------------------------------|
| Liquidation handling | -    | -      | ✓            | ✓            | Exchange‑forced position closures.          |
| ADL handling         | -    | -      | ✓            | ✓            | Auto‑Deleveraging events.                   |

Binance Futures can trigger exchange-generated orders in response to risk events:

- **Liquidations**: When insufficient margin exists to maintain a position, Binance forcibly closes it at the bankruptcy price. These orders have client IDs starting with `autoclose-`.
- **ADL (Auto-Deleveraging)**: When the insurance fund is depleted, Binance closes profitable positions to cover losses. These orders use client ID prefix `adl_autoclose`.
- **Settlements (USDT-M)**: Funding/margin settlement orders use client IDs starting with `settlement_autoclose-`.
- **Deliveries (COIN-M)**: Expiring delivery contracts auto-close with client IDs starting with `delivery_autoclose-`.
- **Insurance fund**: Takeover by the insurance fund uses status `NEW_INSURANCE` (deprecated on the public changelog but still observed on the wire).

The adapter detects these special order types via their client ID patterns
(checked before the execution type), then:

1. Logs a warning with order details for monitoring.
2. Generates a `FillReport` with correct fill details and TAKER liquidity side.
3. Generates an `OrderStatusReport` for reconciliation.

Upstream references:

- [USDT-M `ORDER_TRADE_UPDATE`](https://developers.binance.com/docs/derivatives/usds-margined-futures/user-data-streams/Event-Order-Update)
- [COIN-M `ORDER_TRADE_UPDATE`](https://developers.binance.com/docs/derivatives/coin-margined-futures/user-data-streams/Event-Order-Update)

The execution engine creates external orders from runtime status reports when
the order is not already in cache. This covers first-seen exchange-generated
orders (the typical case for a live liquidation or ADL event). The engine
assigns the order to any strategy that has claimed the instrument via
`external_order_claims`, or to the `EXTERNAL` strategy by default.

#### Commission estimation

When Binance omits the commission fields (`N`/`n`) from the fill event, the
Rust adapter estimates commission as `default_taker_fee * qty * price` using
the quote currency. This applies to USD-M linear contracts only. COIN-M
inverse contracts use zero commission as a fallback because the linear
formula does not account for contract size. Configure `default_taker_fee` on
`BinanceExecClientConfig` to match your fee tier (default: 0.0004 / 0.04%).

#### Hedge-mode position IDs

When `use_position_ids` is enabled (default), exchange-generated fill reports
include a `venue_position_id` derived from the instrument and position side
(e.g. `ETHUSDT-PERP.BINANCE-LONG`). Set `use_position_ids` to false on
`BinanceExecClientConfig` for virtual positions with `OmsType.HEDGING`.

:::note
The status report and fill report are emitted bundled as a single
`OrderWithFills` execution report. The engine creates the external order
from the status report and then applies the real fill, preserving the
venue's `trade_id` and `commission`. Any residual quantity not covered by
the bundled fills is closed with an inferred fill from the status report's
`avg_px`.
:::

### Order querying

| Feature             | Spot | Margin | USDT Futures | Coin Futures | Notes                                       |
|---------------------|------|--------|--------------|--------------|---------------------------------------------|
| Query open orders   | ✓    | ✓      | ✓            | ✓            | List all active orders.                     |
| Query order history | ✓    | ✓      | ✓            | ✓            | Historical order data.                      |
| Order status updates| ✓    | ✓      | ✓            | ✓            | Real‑time order state changes.              |
| Trade history       | ✓    | ✓      | ✓            | ✓            | Execution and fill reports.                 |

### Contingent orders

| Feature             | Spot | Margin | USDT Futures | Coin Futures | Notes                                        |
|---------------------|------|--------|--------------|--------------|----------------------------------------------|
| Order lists         | -    | -      | -            | -            | *Not supported*.                             |
| OCO orders          | -    | -      | -            | -            | *Planned*. Currently denied at submission.   |
| Bracket orders      | -    | -      | -            | -            | *Planned*. Currently denied at submission.   |
| Conditional orders  | ✓    | ✓      | ✓            | ✓            | Stop and market‑if‑touched orders.           |

### Order parameters

Customize individual orders by supplying a `params` dictionary when calling
`Strategy.submit_order` (Python) or setting `Params` on a `SubmitOrder`
command (Rust). The Binance execution clients recognize:

| Parameter        | Type   | Account types     | Description |
|------------------|--------|-------------------|-------------|
| `price_match`    | `str`  | USDT/COIN Futures | Set one of Binance's `priceMatch` modes (see Price match section below) to delegate price selection to the exchange. Cannot be combined with `post_only` or iceberg (`display_qty`) instructions. |
| `close_position` | `bool` | USDT/COIN Futures | Close the entire position when the trigger fires (see Close position section below). Only valid for `StopMarket` and `MarketIfTouched` orders. Cannot be combined with `reduce_only`. |

### Price match

Binance Futures supports BBO (Best Bid/Offer) price matching via the
`priceMatch` parameter, which delegates price selection to the exchange. Limit
orders dynamically join the order book at optimal prices without specifying an
exact price level.

When using `price_match`, you submit a limit order with a reference price (for
local risk checks), and Binance determines the actual working price based on
the current market state and price match mode.

#### Valid price match values

| Value         | Behavior                                                       |
|---------------|----------------------------------------------------------------|
| `OPPONENT`    | Join the best price on the opposing side of the book.          |
| `OPPONENT_5`  | Join the opposing side price but allow up to a 5-tick offset.  |
| `OPPONENT_10` | Join the opposing side price but allow up to a 10-tick offset. |
| `OPPONENT_20` | Join the opposing side price but allow up to a 20-tick offset. |
| `QUEUE`       | Join the best price on the same side (stay maker).             |
| `QUEUE_5`     | Join the same‑side queue but offset up to 5 ticks.             |
| `QUEUE_10`    | Join the same‑side queue but offset up to 10 ticks.            |
| `QUEUE_20`    | Join the same‑side queue but offset up to 20 ticks.            |

:::info
For more details, see the [official documentation](https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api).
:::

#### Event sequence

When an order is submitted with `price_match`:

1. Nautilus sends the order to Binance with the `priceMatch` parameter but omits the limit price from the API request.
2. Binance accepts the order and determines the actual working price.
3. Nautilus generates an `OrderAccepted` event.
4. If the Binance-accepted price differs from the reference price, Nautilus generates an `OrderUpdated` event with the actual working price.
5. The order price in the Nautilus cache now matches the Binance-accepted price.

#### Example

```python
order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTCUSDT-PERP.BINANCE"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_int(1),
    price=Price.from_str("65000"),  # Reference price for local risk checks
)

strategy.submit_order(
    order,
    params={"price_match": "QUEUE"},
)
```

:::note
If Binance accepts the order at a different price (e.g. 64,995.50), you
receive an `OrderAccepted` event followed by an `OrderUpdated` event with
the new price.
:::

### Close position

Binance Futures conditional orders support `closePosition`, which closes the entire position
when the trigger fires. Binance resolves the quantity server-side from the current position
size at trigger time.

Unlike `reduce_only`, `closePosition` adapts to position size changes, and Binance
auto-cancels the order when the position is closed by other means.

Pass `close_position` via the `params` dictionary on `StopMarket` or `MarketIfTouched` orders.
Cannot be combined with `reduce_only`.

<Tabs items={['Python', 'Rust']}>
<Tab value="Python">

```python
strategy.submit_order(order, params={"close_position": True})
```

</Tab>
<Tab value="Rust">
```rust
let params = Params::from([("close_position", true.into())]);
let cmd = SubmitOrder::new(order).with_params(params);
```
</Tab>
</Tabs>

:::info
Nautilus omits `quantity` and `reduceOnly` from the API request when `close_position` is set.
The order quantity is used only for local risk checks.
:::

### Trailing stops

For trailing stop market orders on Binance:

- Use `activation_price` (optional) to specify when the trailing mechanism activates.
- When omitted, Binance uses the current market price at submission time.
- Use `trailing_offset` for the callback rate (in basis points).

:::warning
Do not use `trigger_price` for trailing stop orders: it will fail with an
error. Use `activation_price` instead.
:::

## Link & Trade

The NautilusTrader integration ID is automatically prefixed to all
system-generated client order IDs for every order placed through the Binance
Rust adapter. This provides transparent order attribution through Binance's
[Link and Trade](https://developers.binance.com/docs/binance_link/link-and-trade)
program without requiring any user configuration.

The adapter uses a deterministic two-way encoding to compress outgoing
`ClientOrderId` values into a compact format that fits within Binance's
36-character `newClientOrderId` limit, and decodes incoming order events back
to the original ID before they reach strategies. This transformation is fully
transparent: strategies see only their original `ClientOrderId` values at all
times.

:::note
The integration ID prefix applies to all order operations including
submissions, modifications, cancellations, and status queries. Orders placed
before this support was added are handled gracefully through passthrough
decoding.
:::

:::info
This feature is currently available in the Rust adapter only. Users can opt out
by passing a custom `client_order_id` on their orders, or by removing the
encoding calls and recompiling. There is no technical limitation preventing
either approach.
:::

### Decoding client order IDs

When querying Binance directly (REST API, web UI, or your own HTTP code), the
`clientOrderId` field contains the encoded form. Two utility functions recover
the original Nautilus `ClientOrderId`:

```python
from nautilus_trader.adapters.binance import (
    decode_binance_futures_client_order_id,
    decode_binance_spot_client_order_id,
)

# Encoded ID from Binance REST response or web UI
encoded = "x-TD67BGP9-T0A4b1H2vj50H"
original = decode_binance_spot_client_order_id(encoded)
# -> "O-20260305-120000-001-001-100"

# Futures equivalent
encoded_futures = "x-aHRE4BCj-U2xK9mPqR7sT1vW3y"
original_futures = decode_binance_futures_client_order_id(encoded_futures)
```

Strings without the broker prefix pass through unchanged, so these are safe
to call on any `clientOrderId` value.

:::note
The domain-level HTTP clients (`BinanceSpotHttpClient`,
`BinanceFuturesHttpClient`) decode automatically when returning Nautilus
types such as `OrderStatusReport`. Manual decoding is only needed when
working outside the adapter: direct REST queries, the Binance web UI, or
raw venue models.
:::

## Order books

Order books can be maintained at full or partial depths. WebSocket stream
update rates differ between Spot and Futures, with Nautilus using the highest
available rate:

- **Spot**: 100ms
- **Futures**: 0ms (unthrottled)

Only one order book per instrument per trader instance is supported. When
stream subscriptions vary, the Binance data client uses the latest order book
data subscription (deltas or snapshots).

Order book snapshot rebuilds will be triggered on:

- Initial subscription of the order book data.
- Data websocket reconnects.

The sequence of events is as follows:

- Deltas will start buffered.
- Snapshot is requested and awaited.
- Snapshot response is parsed to `OrderBookDeltas`.
- Snapshot deltas are sent to the `DataEngine`.
- Buffered deltas are iterated, dropping those where the sequence number is not greater than the last delta in the snapshot.
- Deltas will stop buffering.
- Remaining deltas are sent to the `DataEngine`.

## Binance data differences

The `ts_event` field on `QuoteTick` differs between Spot and Futures. Spot
does not provide an event timestamp, so the adapter uses `ts_init` (meaning
`ts_event` and `ts_init` are identical).

## Binance specific data

You can subscribe to Binance-specific data streams as they become available.

:::note
Bars, mark prices, index prices, and funding rates can be subscribed to in the
normal way via the Rust adapter. The custom data subscriptions below are for
the Python adapter.
:::

### `BinanceFuturesMarkPriceUpdate`

Subscribe to `BinanceFuturesMarkPriceUpdate` (including funding rate info)
from your actor or strategy:

```python
from nautilus_trader.adapters.binance import BinanceFuturesMarkPriceUpdate
from nautilus_trader.model import DataType
from nautilus_trader.model import ClientId

# In your `on_start` method
self.subscribe_data(
    data_type=DataType(BinanceFuturesMarkPriceUpdate, metadata={"instrument_id": self.instrument.id}),
    client_id=ClientId("BINANCE"),
)
```

Received `BinanceFuturesMarkPriceUpdate` objects are passed to your `on_data`
method. Check the type, as this method handles all custom/generic data.

```python
from nautilus_trader.core import Data

def on_data(self, data: Data):
    # First check the type of data
    if isinstance(data, BinanceFuturesMarkPriceUpdate):
        # Do something with the data
```

## Funding rates

The Rust adapter emits `FundingRateUpdate` as a first-class data type through
`subscribe_funding_rates`. The data comes from the
[Mark Price Stream](https://developers.binance.com/docs/derivatives/usds-margined-futures/websocket-market-streams/Mark-Price-Stream)
WebSocket endpoint, which provides the current funding rate and next funding
time alongside mark and index prices. All three subscriptions
(`subscribe_mark_prices`, `subscribe_index_prices`, `subscribe_funding_rates`)
share a single `@markPrice@1s` stream with ref-counted subscription management.

The Python adapter exposes funding rate data through
`BinanceFuturesMarkPriceUpdate` custom data subscriptions (see
[Binance specific data](#binance-specific-data) below).

The `interval` field on `FundingRateUpdate` is `None` for Binance because the
Mark Price Stream does not include a funding interval field. Binance exposes
`fundingIntervalHours` through the
[Get Funding Rate Info](https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Get-Funding-Rate-Info)
REST endpoint, but the adapter does not consume it.

## Instrument status polling

:::info[Rust adapter only]
This feature is available in the Rust data clients (`LiveNode`). The Python
data clients do not poll for status changes.
:::

The adapter periodically polls Binance `exchangeInfo` to detect changes in
instrument trading status. When a symbol transitions between states (e.g.
Trading to Halt, or Trading to Delivering for a futures contract approaching
expiry), the adapter emits an `InstrumentStatus` event.

The polling interval defaults to 3600 seconds (60 minutes) and is configurable
via `instrument_status_poll_secs` in the data client config. Set to `0` to
disable polling entirely.

On initial connect, the adapter seeds its status cache from the exchange info
response without emitting events. Only subsequent polls that detect a status
change emit `InstrumentStatus` events. If a symbol disappears from exchange
info (e.g. after delisting or contract expiry), the adapter emits
`NotAvailableForTrading`.

### Status mapping

#### Spot

| Binance status     | MarketStatusAction         |
|--------------------|----------------------------|
| Trading            | Trading                    |
| EndOfDay           | Close                      |
| Halt               | Halt                       |
| Break              | Pause                      |
| NonRepresentable   | NotAvailableForTrading     |

#### Futures (USD-M)

| Binance status     | MarketStatusAction         |
|--------------------|----------------------------|
| Trading            | Trading                    |
| PendingTrading     | PreOpen                    |
| PreTrading         | PreOpen                    |
| PostTrading        | PostClose                  |
| EndOfDay           | Close                      |
| Halt               | Halt                       |
| AuctionMatch       | Cross                      |
| Break              | Pause                      |

#### Futures (COIN-M)

| Binance status     | MarketStatusAction         |
|--------------------|----------------------------|
| Trading            | Trading                    |
| PendingTrading     | PreOpen                    |
| PreDelivering      | PreClose                   |
| Delivering         | Close                      |
| Delivered          | Close                      |
| PreSettle          | PreClose                   |
| Settling           | Close                      |
| Close              | Close                      |
| PreDelisting       | PreClose                   |
| Delisting          | Suspend                    |
| Down               | NotAvailableForTrading     |

:::note
Only instruments that are in a tradable state at connect time are tracked.
Symbols that start in a non-trading state (e.g. halted at connect) do not
appear in the instruments cache, so status transitions for them are not
monitored.
:::

## Rate limiting

Binance uses an interval-based rate limiting system where request weight is
tracked per fixed time window (every minute, resetting at :00 seconds). Each
API endpoint has an assigned weight cost, and total weight usage is tracked
per IP address.

### Global weight limits

These are the primary limits shared across all endpoints:

| Account Type | Weight Limit | Interval |
|--------------|--------------|----------|
| Spot/Margin  | 6,000        | 1 minute |
| Futures      | 2,400        | 1 minute |

### Endpoint weight costs

Some endpoints have higher weight costs per request:

| Endpoint                  | Weight | Notes                                  |
|---------------------------|--------|----------------------------------------|
| `/api/v3/order`           | 1      | Spot order placement.                  |
| `/api/v3/allOrders`       | 20     | Spot historical orders (expensive).    |
| `/api/v3/klines`          | 2+     | Scales with `limit` parameter.         |
| `/fapi/v1/order`          | 1      | Futures order placement.               |
| `/fapi/v1/allOrders`      | 20     | Futures historical orders (expensive). |
| `/fapi/v1/commissionRate` | 20     | Futures commission rate query.         |
| `/fapi/v1/klines`         | 5+     | Scales with `limit` parameter.         |

### WebSocket API limits

The WebSocket API (used for user data streams) shares the same weight quota as the REST API:

| Limit Type       | Value  | Notes                                 |
|------------------|--------|---------------------------------------|
| Request weight   | Shared | Counts against REST API weight quota. |
| Handshake        | 5      | Weight cost per connection attempt.   |
| Ping/pong frames | 5/sec  | Maximum ping/pong rate.               |

### Adapter behavior

The adapter uses token bucket rate limiters to approximate Binance's
interval-based limits. This reduces the risk of quota violations while
maintaining throughput for normal operations.

For endpoints with dynamic weight (e.g. `/klines` scales with the `limit`
parameter), the adapter draws a single token per call. Large history requests
may need manual pacing. Monitor the `X-MBX-USED-WEIGHT-*` response headers to
track actual usage.

:::warning
Binance returns HTTP 429 when you exceed the allowed weight. Repeated
violations trigger temporary IP bans (escalating from 2 minutes to 3 days
for repeat offenders).
:::

:::info
For the latest rate limits, query `/api/v3/exchangeInfo` (Spot) or `/fapi/v1/exchangeInfo` (Futures), or see:

- [Spot API Limits](https://developers.binance.com/docs/binance-spot-api-docs/rest-api/limits)
- [Futures API Limits](https://developers.binance.com/docs/derivatives/usds-margined-futures/general-info)

:::

## Configuration

:::note
The configuration tables below describe the **Python adapter**. The Rust adapter
uses `BinanceDataClientConfig` and `BinanceExecClientConfig` with different field
names. See the Rust source at `crates/adapters/binance/src/config.rs` for the
definitive list of Rust config options.
:::

### Data client configuration options

| Option                             | Default   | Description |
|------------------------------------|-----------|-------------|
| `venue`                            | `BINANCE` | Venue identifier used when registering the client. |
| `api_key`                          | `None`    | Binance API key; loaded from environment variables when omitted. |
| `api_secret`                       | `None`    | Binance API secret; loaded from environment variables when omitted. |
| `key_type`                         | `HMAC`    | **Deprecated**: key type is now auto‑detected from the API secret format. Only needed to force `RSA`. |
| `account_type`                     | `SPOT`    | Account type for data endpoints (spot, margin, USDT futures, coin futures). |
| `base_url_http`                    | `None`    | Override for the HTTP REST base URL. |
| `base_url_ws`                      | `None`    | Override for the WebSocket base URL. |
| `proxy_url`                        | `None`    | Optional proxy URL for HTTP and WebSocket transports. |
| `us`                               | `False`   | Route requests to Binance US endpoints when `True`. |
| `environment`                      | `None`    | Binance environment: `LIVE`, `TESTNET`, or `DEMO`. Defaults to `LIVE` when `None`. |
| `testnet`                          | `False`   | **Deprecated**: use `environment=BinanceEnvironment.TESTNET` instead. |
| `update_instruments_interval_mins` | `60`      | Interval (minutes) between instrument catalogue refreshes. |
| `use_agg_trade_ticks`              | `False`   | When `True`, subscribe to aggregated trade ticks instead of raw trades. Futures WebSocket subscriptions always use `@aggTrade` regardless of this flag. |
| `instrument_status_poll_secs`      | `3600`    | *Rust only.* Interval (seconds) between exchange info polls to detect instrument status changes. Set to `0` to disable. |

### Execution client configuration options

| Option                               | Default   | Description |
|--------------------------------------|-----------|-------------|
| `venue`                              | `BINANCE` | Venue identifier used when registering the client. |
| `api_key`                            | `None`    | Binance API key; loaded from environment variables when omitted. |
| `api_secret`                         | `None`    | Binance API secret; loaded from environment variables when omitted. |
| `key_type`                           | `HMAC`    | **Deprecated**: key type is now auto‑detected from the API secret format. Only needed to force `RSA` (data clients only, RSA is not supported for execution). |
| `account_type`                       | `SPOT`    | Account type for order placement (spot, margin, USDT futures, coin futures). |
| `base_url_http`                      | `None`    | Override for the HTTP REST base URL. |
| `base_url_ws`                        | `None`    | Override for the WebSocket API base URL. |
| `base_url_ws_stream`                 | `None`    | Override for the WebSocket stream URL (futures user data event delivery). |
| `proxy_url`                          | `None`    | Optional proxy URL for HTTP and WebSocket transports. |
| `us`                                 | `False`   | Route requests to Binance US endpoints when `True`. |
| `environment`                        | `None`    | Binance environment: `LIVE`, `TESTNET`, or `DEMO`. Defaults to `LIVE` when `None`. |
| `testnet`                            | `False`   | **Deprecated**: use `environment=BinanceEnvironment.TESTNET` instead. |
| `use_gtd`                            | `True`    | When `False`, remaps GTD orders to GTC for local expiry management. |
| `use_reduce_only`                    | `True`    | When `True`, passes through `reduce_only` instructions to Binance. |
| `use_position_ids`                   | `True`    | Enable Binance hedging position IDs; set `False` for virtual hedging. |
| `use_trade_lite`                     | `False`   | Use TRADE_LITE execution events that include derived fees. |
| `treat_expired_as_canceled`          | `False`   | Treat `EXPIRED` execution types as `CANCELED` when `True`. |
| `recv_window_ms`                     | `5,000`   | Receive window (milliseconds) for signed REST requests. |
| `max_retries`                        | `None`    | Maximum retry attempts for order submission/cancel/modify calls. |
| `retry_delay_initial_ms`             | `None`    | Initial delay (milliseconds) between retry attempts. |
| `retry_delay_max_ms`                 | `None`    | Maximum delay (milliseconds) between retry attempts. |
| `futures_leverages`                  | `None`    | Mapping of `BinanceSymbol` to initial leverage for futures accounts. |
| `futures_margin_types`               | `None`    | Mapping of `BinanceSymbol` to futures margin type (isolated/cross). |
| `use_ws_trading`                         | `True`  | Use the WebSocket trading API for order operations (Spot and USD-M Futures). When `False`, HTTP is used. |
| `default_taker_fee`                      | `0.0004` | Default taker fee rate for commission estimation on exchange‑generated fills (liquidation, ADL, settlement). |
| `log_rejected_due_post_only_as_warning` | `True` | Log post‑only rejections as warnings when `True`; otherwise as errors. |

The most common use case is to configure a live `TradingNode` with Binance
data and execution clients. Add a `BINANCE` section to your client
configuration:

```python
from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        BINANCE: {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "account_type": "spot",  # {spot, usdt_futures, coin_futures}
            "base_url_http": None,  # Override with custom endpoint
            "base_url_ws": None,  # Override with custom endpoint
            "us": False,  # If client is for Binance US
        },
    },
    exec_clients={
        BINANCE: {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "account_type": "spot",  # {spot, usdt_futures, coin_futures}
            "base_url_http": None,  # Override with custom endpoint
            "base_url_ws": None,  # Override with custom endpoint
            "us": False,  # If client is for Binance US
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance import BinanceLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
node.add_exec_client_factory(BINANCE, BinanceLiveExecClientFactory)

# Finally build the node
node.build()
```

### Key types

Binance supports three API key types: **Ed25519**, **HMAC-SHA256**, and
**RSA**. The adapter auto-detects the key type from your API secret format, so
no configuration is needed.

**Ed25519 is strongly recommended.** Binance recommends Ed25519 for its
superior performance and security. A future version of NautilusTrader will
require Ed25519 exclusively.

| Key Type | Data Clients | Execution Clients | Status |
|----------|--------------|-------------------|--------|
| Ed25519  | ✓            | ✓                 | **Recommended** |
| HMAC     | ✓            | ✓                 | Deprecated, will be removed in a future version. |
| RSA      | ✓            | -                 | Deprecated, not supported for execution. |

:::tip
Switch to Ed25519 keys now. Generate an Ed25519 keypair and register it with
Binance. See [Generating Ed25519 keys](#generating-ed25519-keys) below.
:::

:::note
Ed25519 keys must be provided in unencrypted PEM format (base64-encoded ASN.1/DER).
The implementation automatically extracts the 32-byte seed from the DER structure.
Encrypted (password-protected) PEM keys are not supported. If your key is encrypted,
decrypt it first: `openssl pkey -in encrypted.pem -out decrypted.pem`
:::

#### Generating Ed25519 keys

**Option 1: OpenSSL (recommended)**

```bash
# Generate private key (PKCS#8 PEM format)
openssl genpkey -algorithm ed25519 -out binance_ed25519_private.pem

# Extract public key
openssl pkey -in binance_ed25519_private.pem -pubout -out binance_ed25519_public.pem
```

**Option 2: Binance Key Generator**

Download the [Binance Asymmetric Key Generator](https://github.com/binance/asymmetric-key-generator) from the releases page and run it to generate a keypair.

**Registering with Binance**

1. Log in to Binance and go to **Profile** → **API Management**
2. Click **Create API** and select **Self-generated**
3. Paste the contents of your public key file (including the `-----BEGIN PUBLIC KEY-----` header/footer)
4. Configure permissions (Enable Spot & Margin Trading, etc.)

**Using with NautilusTrader**

Set the private key as your API secret:

```bash
export BINANCE_API_KEY="your-api-key-from-binance"
export BINANCE_API_SECRET="$(cat binance_ed25519_private.pem)"
```

Or pass the PEM content directly in your configuration.

:::warning
Keep your private key secure. Never share it or commit it to version control.
:::

### API credentials

Pass credentials directly to the configuration objects, or set the appropriate
environment variables (see [Environments](#environments) for per-environment
variables).

:::tip
Use Ed25519 keys for all clients. HMAC keys still work for both data and
execution clients, but Ed25519 offers better performance and will become the
only supported key type in a future version. See [Key types](#key-types).
:::

:::warning
The `BINANCE_ED25519_*` and `BINANCE_*_ED25519_*` environment variables have
been removed for Spot/Margin. For Futures, they are deprecated and will be
removed in a future version. Rename them to `BINANCE_API_KEY` /
`BINANCE_API_SECRET` (Ed25519 keys are now auto-detected).
:::

When the trading node starts, you receive confirmation of whether your
credentials are valid and have trading permissions.

### Account type

Set `account_type` using the `BinanceAccountType` enum:

- `SPOT`
- `USDT_FUTURES` (USDT or BUSD stablecoins as collateral)
- `COIN_FUTURES` (other cryptocurrency as collateral)

:::note
`MARGIN` and `ISOLATED_MARGIN` account types exist in the enum but margin
trading is not implemented. See [Product support](#product-support).
:::

### Base URL overrides

Override the default base URLs for both HTTP REST and WebSocket APIs. This is
useful for configuring API clusters or when Binance has provided specialized
endpoints.

### Binance US

Set `us=True` in the config to use Binance US endpoints (`False` by default).
All functionality available to US accounts behaves identically to standard
Binance.

### Environments

Binance provides three trading environments, each with separate API
credentials and endpoints. The `environment` config option selects which to
use.

| Environment | Config                  | Description                                                            |
|-------------|-------------------------|------------------------------------------------------------------------|
| **Live**    | `environment="LIVE"`    | Production trading with real funds (default).                          |
| **Demo**    | `environment="DEMO"`    | Simulated funds on production infrastructure. Recommended for testing. |
| **Testnet** | `environment="TESTNET"` | Separate test network (Spot only). Limited futures support.            |

#### Live (production)

The default environment for live trading with real funds. Uses your main Binance
account credentials.

```python
config = BinanceExecClientConfig(
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    account_type=BinanceAccountType.SPOT,
    # environment=BinanceEnvironment.LIVE (default)
)
```

| Variable             | Description         |
|----------------------|---------------------|
| `BINANCE_API_KEY`    | Mainnet API key.    |
| `BINANCE_API_SECRET` | Mainnet API secret. |

#### Demo trading

Practice trading with simulated funds on production infrastructure. Demo
accounts use the same Binance login as your live account but trade with
virtual balances. This is the recommended environment for integration testing,
especially for futures.

**How to get demo credentials:**

1. Log in at [binance.com/en/demo-trading](https://www.binance.com/en/demo-trading).
2. Go to **API Management** and create a demo API key.
3. Demo keys work for both Spot and Futures on demo endpoints.

| Endpoint       | URL                          |
|----------------|------------------------------|
| Spot HTTP      | `demo-api.binance.com`       |
| Spot WS        | `demo-stream.binance.com`    |
| USD-M HTTP     | `demo-fapi.binance.com`      |

```python
config = BinanceExecClientConfig(
    api_key="YOUR_DEMO_API_KEY",
    api_secret="YOUR_DEMO_API_SECRET",
    account_type=BinanceAccountType.USDT_FUTURES,
    environment=BinanceEnvironment.DEMO,
)
```

| Variable              | Description                                        |
|-----------------------|----------------------------------------------------|
| `BINANCE_DEMO_API_KEY`    | Demo API key (shared across Spot and Futures). |
| `BINANCE_DEMO_API_SECRET` | Demo API secret.                               |

:::warning
COIN-M Futures are not supported in demo mode.
:::

#### Testnet

A separate test network with its own user accounts, balances, and order books.
Spot testnet is at `testnet.binance.vision`. The futures testnet at
`testnet.binancefuture.com` now redirects to demo; use `environment="DEMO"`
for futures testing instead.

**How to get Spot testnet credentials:**

1. Go to [testnet.binance.vision](https://testnet.binance.vision/).
2. Log in with GitHub.
3. Generate an API key (HMAC, RSA, or Ed25519).

**Futures testnet:** Binance has merged the futures testnet into the demo
environment. If you need to test futures, use `environment="DEMO"` with
demo credentials instead.

```python
config = BinanceExecClientConfig(
    api_key="YOUR_TESTNET_API_KEY",
    api_secret="YOUR_TESTNET_API_SECRET",
    account_type=BinanceAccountType.SPOT,
    environment=BinanceEnvironment.TESTNET,
)
```

| Variable                             | Description                                        |
|--------------------------------------|----------------------------------------------------|
| `BINANCE_TESTNET_API_KEY`            | Spot testnet API key.                              |
| `BINANCE_TESTNET_API_SECRET`         | Spot testnet API secret.                           |
| `BINANCE_FUTURES_TESTNET_API_KEY`    | Futures testnet API key (deprecated, use demo).    |
| `BINANCE_FUTURES_TESTNET_API_SECRET` | Futures testnet API secret (deprecated, use demo). |

:::note
Testnet credentials are completely separate from your live account. Market
data and liquidity differ from production.
:::

:::warning
The `testnet` config option is deprecated and will be removed in a future
version. Use `environment="TESTNET"` instead.
:::

### Aggregated trades

Binance provides aggregated trade data endpoints as an alternative source of
trades. Unlike the default trade endpoints, aggregated trade endpoints can
return all ticks between a `start_time` and `end_time`.

Set `use_agg_trade_ticks=True` to use aggregated trades (`False` by default).

:::note
For Futures (USD-M and COIN-M), the WebSocket trade subscription always uses
`@aggTrade`. Binance only publishes aggregated trades on the Futures WebSocket;
the legacy `@trade` stream was undocumented and has been silenced. The HTTP
`request_trade_ticks` path continues to honour `use_agg_trade_ticks`.
:::

### Commission rate queries

By default, Binance Futures instruments use fee tier tables based on your VIP
level. For market maker accounts with negative maker fees or when precise
rates are required, enable per-symbol commission rate queries:

```python
from nautilus_trader.adapters.binance import BinanceInstrumentProviderConfig

instrument_provider=BinanceInstrumentProviderConfig(
    load_all=True,
    query_commission_rates=True,  # Query accurate rates per symbol
)
```

When enabled, the adapter queries Binance's `/fapi/v1/commissionRate` endpoint
for each symbol in parallel during instrument loading. Useful for:

- Market maker accounts with negative maker fees.
- Accounts with custom fee arrangements.
- Exact commission rates for PnL calculations.

The adapter uses parallel requests with rate limiting (120 requests/minute,
accounting for the endpoint's weight of 20). If a query fails, it falls back
to the fee tier table.

### Parser warnings

Some Binance instruments cannot be parsed into Nautilus objects if they contain
field values beyond what the platform handles. These instruments are skipped
with a warning.

To suppress these warnings:

```python
from nautilus_trader.config import InstrumentProviderConfig

instrument_provider=InstrumentProviderConfig(
    load_all=True,
    log_warnings=False,
)
```

### Futures hedge mode

Binance Futures Hedge mode allows holding both long and short positions on the
same instrument simultaneously.

To use hedge mode:

1. Configure hedge mode on Binance before starting the strategy.
2. Set `use_reduce_only=False` in `BinanceExecClientConfig` (`True` by default).

    ```python
    from nautilus_trader.adapters.binance import BINANCE

    config = TradingNodeConfig(
        ...,  # Omitted
        data_clients={
            BINANCE: BinanceDataClientConfig(
                api_key=None,  # 'BINANCE_API_KEY' env var
                api_secret=None,  # 'BINANCE_API_SECRET' env var
                account_type=BinanceAccountType.USDT_FUTURES,
                base_url_http=None,  # Override with custom endpoint
                base_url_ws=None,  # Override with custom endpoint
            ),
        },
        exec_clients={
            BINANCE: BinanceExecClientConfig(
                api_key=None,  # 'BINANCE_API_KEY' env var
                api_secret=None,  # 'BINANCE_API_SECRET' env var
                account_type=BinanceAccountType.USDT_FUTURES,
                base_url_http=None,  # Override with custom endpoint
                base_url_ws=None,  # Override with custom endpoint
                use_reduce_only=False,  # Must be disabled for Hedge mode
            ),
        }
    )
    ```

3. When submitting an order, use the `LONG` or `SHORT` suffix in `position_id` to indicate position direction.

    ```python
    class EMACrossHedgeMode(Strategy):
        ...,  # Omitted
        def buy(self) -> None:
            order: MarketOrder = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.instrument.make_qty(self.trade_size),
                # time_in_force=TimeInForce.FOK,
            )

            # LONG suffix is recognized as a long position by Binance adapter.
            position_id = PositionId(f"{self.instrument_id}-LONG")
            self.submit_order(order, position_id)

        def sell(self) -> None:
            order: MarketOrder = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=self.instrument.make_qty(self.trade_size),
                # time_in_force=TimeInForce.FOK,
            )
            # SHORT suffix is recognized as a short position by Binance adapter.
            position_id = PositionId(f"{self.instrument_id}-SHORT")
            self.submit_order(order, position_id)
    ```

## Contributing

:::info
To contribute to the Binance adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
