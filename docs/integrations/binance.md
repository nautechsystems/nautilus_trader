# Binance

Founded in 2017, Binance is one of the largest cryptocurrency exchanges in terms
of daily trading volume, and open interest of crypto assets and crypto
derivative products.

NautilusTrader provides Binance integration for live market data and execution. The adapter is
implemented in Rust and exposed to Python through the same public configurations, factories, and
data types.

Supported products:

- **Binance Spot** (including Binance US)
- **Binance USDT-Margined Futures** (crypto and TradFi perpetuals; current and next monthly and
  quarterly delivery contracts)
- **Binance Coin-Margined Futures** (perpetuals and current or next quarterly delivery contracts)

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/binance/)
- [Rust spot examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/binance/examples/spot/)
- [Rust futures examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/binance/examples/futures/)

## Overview

The adapter exposes these public components:

- `BinanceDataClientConfig` and `BinanceExecClientConfig`: Live client configuration.
- `BinanceInstrumentProviderConfig`: Instrument selection, filtering, warning, and fee policy.
- `BinanceDataClientFactory` and `BinanceExecutionClientFactory`: Trading node client factories.
- `load_binance_instruments`: Standalone configured instrument discovery.
- `load_binance_order_book_deltas`: Rust-backed Binance depth CSV loading for order book wrangling.
- `BINANCE`, `BINANCE_CLIENT_ID`, `BINANCE_VENUE`, and the client-order-ID decoders: Public
  identifiers and decoding utilities.

:::note
Most users need only the configs and factories, wired into a live trading node as shown under
[Live node configuration](#live-node-configuration). The remaining components serve standalone
loading and offline decoding.
:::

Low-level HTTP and WebSocket clients, their caches, and product-specific instrument provider
objects are not exposed through the Python API. Use the live configs and factories, or the
standalone instrument loader, instead of depending on those internals.

For standalone discovery, pass the same data-client and provider configuration used by a live
client:

```python
import asyncio

from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceInstrumentProviderConfig
from nautilus_trader.adapters.binance import BinanceProductType
from nautilus_trader.adapters.binance import load_binance_instruments

config = BinanceDataClientConfig(
    product_type=BinanceProductType.USD_M,
    instrument_provider=BinanceInstrumentProviderConfig(
        load_all=False,
        load_ids=["BTCUSDT-PERP.BINANCE"],
    ),
)
instruments = asyncio.run(load_binance_instruments(config))
```

This function supports Spot, USD-M, and COIN-M. It uses the configured environment, URLs, proxy,
receive window, Binance US mode, filters, warning policy, and commission policy. Margin is not a
supported Binance product and is rejected.

For Binance depth CSV data, call the stateless loader directly:

```python
from nautilus_trader.adapters.binance import load_binance_order_book_deltas

df = load_binance_order_book_deltas(path, nrows=1_000_000)
```

The loader preserves the source values and column order. File-open failures and invalid numeric or
side values raise `RuntimeError`.

### Product support

| Product Type                            | Supported | Notes                                     |
| --------------------------------------- | --------- | ----------------------------------------- |
| Spot Markets (incl. Binance US)         | ✓         |                                           |
| Margin Accounts (Cross & Isolated)      | -         | *Not implemented.*                        |
| USDT-Margined Futures (PERP & Delivery) | ✓         | Monthly and quarterly delivery contracts. |
| Coin-Margined Futures (PERP & Delivery) | ✓         | Quarterly delivery contracts.             |

:::note
Margin account features such as borrow, repay, and isolated margin management are not implemented.
:::

:::info
Each Binance client instance handles one product type. The configs use a
singular `product_type` field, and the live factories create one data or
execution client from one config. To run Spot and Futures in the same node,
configure separate clients with distinct IDs such as `BINANCE_SPOT` and
`BINANCE_FUTURES`, then pass the matching `client_id` when a strategy subscribes
or submits orders. See the current Python examples for complete client setup.
:::

## Data types

The integration includes several custom data types:

- `BinanceSpotTicker`: Spot 24-hour ticker data including prices, volumes, and trade statistics.
- `BinanceFuturesTicker`: Futures 24-hour ticker data including price and statistics.
- `BinanceBar`: Bar data with additional volume metrics for historical and real-time use.
- `BinanceFuturesMarkPriceUpdate`: Futures mark data including the estimated settlement price.
- `BinanceFuturesLiquidation`: Futures liquidation events from the `forceOrder` stream.
- `BinanceFuturesOpenInterest`: Current Futures open interest snapshot (request only).
- `BinanceFuturesOpenInterestHist`: Futures open interest history for a period (request only).

See the Binance [API Reference](/docs/python-api-latest/adapters/binance.html) for full definitions.

## Symbology

Native Binance symbols are used where possible for spot and futures contracts.
Because NautilusTrader supports multi-venue trading, it must distinguish between
`BTCUSDT` the spot pair and `BTCUSDT` the perpetual futures contract (Binance
uses the same symbol for both).

Nautilus appends `-PERP` to USD-M perpetual symbols. For example, the Binance
USD-M `BTCUSDT` perpetual becomes `BTCUSDT-PERP`. USD-M `TRADIFI_PERPETUAL`
listings use the same suffix, so `XAUUSDT` becomes `XAUUSDT-PERP`.

The adapter maps `TRADIFI_PERPETUAL` listings to
`PerpetualContract` and derives their asset class from Binance's `underlyingType`:

| Binance `underlyingType`                        | Nautilus asset class |
| ----------------------------------------------- | -------------------- |
| `EQUITY`, `KR_EQUITY`, `HK_EQUITY`, `PREMARKET` | Equity               |
| `COMMODITY`                                     | Commodity            |

Listings with other or missing values are skipped with a warning.

The adapter preserves Binance's native `_PERP` suffix for COIN-M
perpetuals, so `BTCUSD_PERP` remains unchanged.

Delivery symbols keep Binance's `_YYMMDD` suffix. For example,
`BTCUSDT_260925` and `BTCUSD_260925` remain unchanged within Nautilus. USD-M
supports the documented `CURRENT_MONTH`, `NEXT_MONTH`, `CURRENT_QUARTER`, and
`NEXT_QUARTER` contract types. COIN-M supports `CURRENT_QUARTER` and
`NEXT_QUARTER`. Contract availability varies by environment and listing cycle.

USD-M delivery instruments are linear and settle in the margin asset. COIN-M
delivery instruments are inverse, settle in the margin asset (the base
currency), and use Binance's `contractSize` as the instrument multiplier. Both
use `onboardDate` and `deliveryDate` for activation and expiration. See
Binance's official
[USD-M common definitions](https://developers.binance.com/en/docs/products/derivatives-trading-usds-futures/common-definition)
and [COIN-M common definitions](https://developers.binance.com/en/docs/products/derivatives-trading-coin-futures/common-definition).

The Rust Futures data tester accepts a delivery instrument without source edits:

```bash
BINANCE_FUTURES_INSTRUMENT_ID=BTCUSDT_260925.BINANCE \
  cargo run -p nautilus-binance --example binance-futures-data-tester --features examples
```

## Order capability

The following tables detail order types, execution instructions, and
time-in-force options across the supported Binance products.

### Order types

| Order Type             | Spot | USDT Futures | Coin Futures | Notes                                   |
| ---------------------- | ---- | ------------ | ------------ | --------------------------------------- |
| `MARKET`               | ✓    | ✓            | ✓            | Quote quantity support: Spot only.      |
| `LIMIT`                | ✓    | ✓            | ✓            |                                         |
| `STOP_MARKET`          | ✓    | ✓            | ✓            | Spot sends Binance `STOP_LOSS`.         |
| `STOP_LIMIT`           | ✓    | ✓            | ✓            | Spot sends Binance `STOP_LOSS_LIMIT`.   |
| `MARKET_IF_TOUCHED`    | ✓    | ✓            | ✓            | Spot sends Binance `TAKE_PROFIT`.       |
| `LIMIT_IF_TOUCHED`     | ✓    | ✓            | ✓            | Spot sends Binance `TAKE_PROFIT_LIMIT`. |
| `TRAILING_STOP_MARKET` | -    | ✓            | ✓            | Futures only.                           |

Binance Spot publishes a supported order-type set per symbol in `exchangeInfo`. The adapter does
not filter on it, so a conditional Spot order for a type the symbol does not support is rejected by
the venue rather than locally.

### Execution instructions

| Instruction   | Spot | USDT Futures | Coin Futures | Notes                                 |
| ------------- | ---- | ------------ | ------------ | ------------------------------------- |
| `post_only`   | ✓    | ✓            | ✓            | See restrictions below.               |
| `reduce_only` | -    | ✓            | ✓            | Futures only; disabled in Hedge Mode. |

#### Post-only restrictions

Only *limit* order types support `post_only`.

| Order Type   | Spot | USDT Futures | Coin Futures | Notes                                               |
| ------------ | ---- | ------------ | ------------ | --------------------------------------------------- |
| `LIMIT`      | ✓    | ✓            | ✓            | Uses `LIMIT_MAKER` for Spot, `GTX` TIF for Futures. |
| `STOP_LIMIT` | -    | ✓            | ✓            | Futures only.                                       |

### Time in force

| Time in force | Spot | USDT Futures | Coin Futures | Notes                                     |
| ------------- | ---- | ------------ | ------------ | ----------------------------------------- |
| `GTC`         | ✓    | ✓            | ✓            | Good Till Canceled.                       |
| `GTD`         | ✓*   | ✓            | ✓*           | *Non-default local mapping through `GTC`. |
| `FOK`         | ✓    | ✓            | ✓            | Fill or Kill.                             |
| `IOC`         | ✓    | ✓            | ✓            | Immediate or Cancel.                      |

#### GTD policy

[Binance Spot time-in-force values](https://github.com/binance/binance-spot-api-docs/blob/master/enums.md)
are `GTC`, `IOC`, and `FOK`; Spot has no native `GTD` or `goodTillDate`. USD-M supports native
`GTD` for `LIMIT` and the limit forms of `STOP` and `TAKE_PROFIT`. The adapter routes regular
orders through HTTP or WebSocket trading, independent batches through HTTP `batchOrders`, and
conditional algo orders through HTTP `algoOrder`. The current Binance WebSocket algo schema
includes `goodTillDate` but does not include `GTD` in its `timeInForce` enum, so the adapter does
not route GTD algo orders through that endpoint. COIN-M has no native `GTD` value or
`goodTillDate` parameter in its documented order APIs. See the official
[USD-M trade API](https://developers.binance.com/en/docs/catalog/core-trading-derivatives-trading-usd-s-m-futures/api/rest-api/trade)
and [COIN-M common definitions](https://developers.binance.com/en/docs/products/derivatives-trading-coin-futures/common-definition).

USD-M `goodTillDate` is an epoch timestamp in milliseconds, but Binance ignores any sub-second
part. Nautilus rejects an expiry that is not on a whole-second boundary rather than silently
rounding it. The expiry must be strictly greater than the current time plus 600 seconds and
strictly less than `253402300799000`. Native GTD also rejects market and post-only orders and any
order without an expiry.

`use_gtd=True` is the default. It uses native USD-M GTD and rejects native GTD on Spot and COIN-M.
Set `use_gtd=False` only when the submitting strategy has `manage_gtd_expiry=True`. The adapter
then warns and sends `GTC`, while Nautilus cancels the order at its local expiry.

### Advanced order features

| Feature            | Spot | USDT Futures | Coin Futures | Notes                                       |
| ------------------ | ---- | ------------ | ------------ | ------------------------------------------- |
| Order Modification | ✓    | ✓            | ✓            | Price and quantity for `LIMIT` orders only. |
| OCO Orders         | ✓    | -            | -            | Spot OCO submitted via `orderList/oco`.     |
| Bracket Orders     | -    | -            | -            | *Planned*. Currently denied at submission.  |
| Iceberg Orders     | ✓    | -            | -            | Spot `icebergQty` from `display_qty`.       |

### Batch operations

| Operation    | Spot | USDT Futures | Coin Futures | Notes                                   |
| ------------ | ---- | ------------ | ------------ | --------------------------------------- |
| Batch Submit | ✓    | ✓            | ✓            | Spot OCO or Futures `batchOrders`.      |
| Batch Modify | -    | -            | -            | Not implemented.                        |
| Batch Cancel | -*   | ✓            | ✓            | *Spot falls back to individual cancels. |

#### Cancel all orders behavior

By default, `Strategy.cancel_all_orders()` sends individual cancels for orders associated with that
strategy. When `strategy_only=False` is used, the strategy sends a broad `CancelAllOrders` command
to the adapter. The adapter includes orders in both open and inflight (SUBMITTED) states so that it
also cancels orders not yet acknowledged by Binance.

**Multi-strategy safety**: When multiple strategies trade the same instrument,
the adapter compares orders associated with the requesting strategy against all orders
for that instrument. If all orders are associated with the strategy, a single cancel-all API
call is used. Otherwise, per-strategy cancels are sent (batch for regular
orders, individual for algo orders) to avoid affecting other strategies.

**Futures algo orders**: Conditional order types (`STOP_MARKET`, `STOP_LIMIT`,
`TAKE_PROFIT`, `TAKE_PROFIT_MARKET`, `TRAILING_STOP_MARKET`) require a
different cancel endpoint. The adapter routes these through the correct
endpoint automatically. Once an algo order triggers and becomes a regular
order, it uses the standard cancel endpoint.

**Endpoints used**:

| Product      | Regular Orders                  | Algo Orders (batch)              | Algo Orders (individual)    |
| ------------ | ------------------------------- | -------------------------------- | --------------------------- |
| Spot         | `DELETE /api/v3/openOrders`     | N/A                              | N/A                         |
| USDT Futures | `DELETE /fapi/v1/allOpenOrders` | `DELETE /fapi/v1/algoOpenOrders` | `DELETE /fapi/v1/algoOrder` |
| Coin Futures | `DELETE /dapi/v1/allOpenOrders` | `DELETE /dapi/v1/algoOpenOrders` | `DELETE /dapi/v1/algoOrder` |

#### Submit, modify, and cancel retry policy

The execution clients send each submit, modify, or cancel command once. They do not
blindly retry a command after a timeout, network failure, or Binance unknown-status response because
the first request may have reached the matching engine. Retrying could create a duplicate order or
apply a second amendment.

- A definitive local validation error or venue rejection emits the matching rejection event.
- An ambiguous transport result remains inflight and is resolved by the private stream or REST
  reconciliation. The adapter does not emit a false rejection while the venue outcome is unknown.
- A Futures algo cancel may fall back from the pre-trigger algo endpoint to the regular-order
  endpoint. This changes endpoint after the order triggers; it does not resend the same cancel to
  the same endpoint.
- Strategy code must not resubmit a command while its result is ambiguous. Wait for reconciliation
  or query the order by its client order ID.

The configs do not expose retry controls for order commands because resending an ambiguous command
could duplicate an order or amendment.

### Position management

| Feature          | Spot | USDT Futures | Coin Futures | Notes                                   |
| ---------------- | ---- | ------------ | ------------ | --------------------------------------- |
| Query positions  | -    | ✓            | ✓            | Real-time position updates.             |
| Position mode    | -    | ✓            | ✓            | One-Way vs Hedge mode (position IDs).   |
| Leverage control | -    | ✓            | ✓            | Dynamic leverage adjustment per symbol. |
| Margin mode      | -    | ✓            | ✓            | Cross vs Isolated margin per symbol.    |

### Risk events

| Feature              | Spot | USDT Futures | Coin Futures | Notes                              |
| -------------------- | ---- | ------------ | ------------ | ---------------------------------- |
| Liquidation handling | -    | ✓            | ✓            | Exchange-forced position closures. |
| ADL handling         | -    | ✓            | ✓            | Auto-Deleveraging events.          |

Binance Futures can trigger exchange-generated orders in response to risk events:

- **Liquidations**: When insufficient margin exists to maintain a position, Binance forcibly closes
  it at the bankruptcy price. These orders have client IDs starting with `autoclose-`.
- **ADL (Auto-Deleveraging)**: When the insurance fund is depleted, Binance closes profitable
  positions to cover losses. These orders use client ID prefix `adl_autoclose`.
- **Settlements (USD-M)**: Funding and margin settlement orders use client IDs starting with
  `settlement_autoclose-`.
- **Deliveries (COIN-M)**: Expiring delivery contracts auto-close with client IDs starting with
  `delivery_autoclose-`.
- **Insurance fund**: Takeover by the insurance fund uses status `NEW_INSURANCE` (deprecated on the
  public changelog but still observed on the wire).

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
adapter estimates commission as `default_taker_fee * qty * price` using
the quote currency. This applies to USD-M linear contracts only. COIN-M
inverse contracts use zero commission as a fallback because the linear
formula does not account for contract size. Configure `default_taker_fee` on
`BinanceExecClientConfig` to match your fee tier (default: 0.0004 / 0.04%).

#### Hedge-mode position IDs

When `use_position_ids` is enabled (default), exchange-generated fill reports
include a `venue_position_id` derived from the instrument and position side
(e.g. `ETHUSDT-PERP.BINANCE-LONG`). Keep this enabled for Binance dual-side
positions. Set `use_position_ids` to false only for virtual positions with
`OmsType.HEDGING`, where the engine manages position identity.

For Futures accounts in dual-side position mode, set `oms_type=OmsType.HEDGING`. The adapter
defaults to `OmsType.NETTING` for one-way position mode. Leave `use_position_ids` enabled to track
Binance's separate long and short sides.

:::note
The status report and fill report are emitted bundled as a single
`OrderWithFills` execution report. The engine creates the external order
from the status report and then applies the real fill, preserving the
venue's `trade_id` and `commission`. Any residual quantity not covered by
the bundled fills is closed with an inferred fill from the status report's
`avg_px`.
:::

### Order querying

| Feature              | Spot | USDT Futures | Coin Futures | Notes                          |
| -------------------- | ---- | ------------ | ------------ | ------------------------------ |
| Query open orders    | ✓    | ✓            | ✓            | List all active orders.        |
| Query order history  | ✓    | ✓            | ✓            | Historical order data.         |
| Order status updates | ✓    | ✓            | ✓            | Real-time order state changes. |
| Trade history        | ✓    | ✓            | ✓            | Execution and fill reports.    |

### Contingent orders

| Feature            | Spot | USDT Futures | Coin Futures | Notes                                        |
| ------------------ | ---- | ------------ | ------------ | -------------------------------------------- |
| Order lists        | ✓    | ✓            | ✓            | Spot OCO lists; Futures independent batches. |
| OCO orders         | ✓    | -            | -            | Spot only, via `orderList/oco`.              |
| Bracket orders     | -    | -            | -            | *Planned*. Currently denied at submission.   |
| Conditional orders | ✓    | ✓            | ✓            | Stop and market-if-touched orders.           |

### Order parameters

Customize individual orders by supplying a `params` dictionary when calling
`Strategy.submit_order` (Python) or setting `Params` on a `SubmitOrder`
command (Rust). The Binance execution clients recognize:

| Parameter        | Type   | Products          | Purpose                                          | Restrictions                                                                     |
| ---------------- | ------ | ----------------- | ------------------------------------------------ | -------------------------------------------------------------------------------- |
| `price_match`    | `str`  | USDT/COIN Futures | Delegate price selection to Binance.             | `LIMIT` only; not with `post_only`.                                              |
| `close_position` | `bool` | USDT/COIN Futures | Close the whole position when the trigger fires. | `StopMarket` and `MarketIfTouched` only; not with `reduce_only`, not in batches. |

See [Price match](#price-match) and [Close position](#close-position) for the full behavior.

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
| ------------- | -------------------------------------------------------------- |
| `OPPONENT`    | Join the best price on the opposing side of the book.          |
| `OPPONENT_5`  | Join the opposing side price but allow up to a 5-tick offset.  |
| `OPPONENT_10` | Join the opposing side price but allow up to a 10-tick offset. |
| `OPPONENT_20` | Join the opposing side price but allow up to a 20-tick offset. |
| `QUEUE`       | Join the best price on the same side (stay maker).             |
| `QUEUE_5`     | Join the same-side queue but offset up to 5 ticks.             |
| `QUEUE_10`    | Join the same-side queue but offset up to 10 ticks.            |
| `QUEUE_20`    | Join the same-side queue but offset up to 20 ticks.            |

:::info
For more details, see the [official documentation](https://developers.binance.com/docs/derivatives/usds-margined-futures/trade/rest-api).
:::

#### Event sequence

When an order is submitted with `price_match`:

1. Nautilus sends the order with the `priceMatch` parameter and omits the limit price from the API
   request.
2. Binance accepts the order and determines the actual working price.
3. Nautilus generates an `OrderAccepted` event.
4. If the Binance-accepted price differs from the reference price, Nautilus generates an
   `OrderUpdated` event with the actual working price.
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
Cannot be combined with `reduce_only`, and it is rejected for batch order submission.

```rust tab="Rust"
let params = Params::from([("close_position", true.into())]);
let cmd = SubmitOrder::new(order).with_params(params);
```

```python tab="Python"
strategy.submit_order(order, params={"close_position": True})
```

:::info
Nautilus omits `quantity` and `reduceOnly` from the API request when `close_position` is set.
The order quantity is used only for local risk checks.
:::

### Trailing stops

For trailing stop market orders on Binance:

- Use `activation_price` (optional) to specify when the trailing mechanism activates.
- When omitted, Binance uses the current market price at submission time.
- Use `trailing_offset` for the callback rate, with `TrailingOffsetType.BASIS_POINTS`. The adapter
  rejects any other offset type, and rejects a callback rate outside Binance's 0.1% to 10% range.

:::warning
Do not use `trigger_price` for trailing stop orders: it will fail with an
error. Use `activation_price` instead.
:::

## Link & Trade

The NautilusTrader integration ID is automatically prefixed to all
system-generated client order IDs for every order placed through the Binance
adapter. This provides transparent order attribution through Binance's
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

### Decoding client order IDs

When querying Binance directly (REST API, web UI, or your own HTTP code), the
`clientOrderId` field contains the encoded form. Two utility functions recover
the original Nautilus `ClientOrderId`:

```python
from nautilus_trader.adapters.binance import (
    decode_binance_futures_client_order_id,
    decode_binance_spot_client_order_id,
)

# Encoded ID from a Binance REST response or the web UI
encoded = "x-TD67BGP9-T0000000000000"
original = decode_binance_spot_client_order_id(encoded)
# Returns "O-20200101-000000-000-000-0"

# Futures equivalent
encoded_futures = "x-aHRE4BCj-T0000000000000"
original_futures = decode_binance_futures_client_order_id(encoded_futures)
# Returns "O-20200101-000000-000-000-0"
```

Strings without the broker prefix pass through unchanged, so these are safe
to call on any `clientOrderId` value.

:::note
The adapter decodes automatically wherever it returns Nautilus types such as `OrderStatusReport`.
Manual decoding is only needed when working outside the adapter: direct REST queries, the Binance
web UI, or raw venue payloads.
:::

## Order books

Order books can be maintained at full or partial depths. The diff-depth stream and its update
rate differ by product and Spot transport:

- **Spot SBE**: `<symbol>@depth`, 25ms.
- **Spot JSON**: `<symbol>@depth`, at Binance's default update speed for that stream (1000ms).
- **Futures**: `<symbol>@depth@0ms`, unthrottled.

`L1_MBP` subscriptions require depth 1 and use the Spot `bestBidAsk` or `bookTicker`
stream and the Futures `bookTicker` stream. Each update emits the normal `QuoteTick`
and a two-sided `OrderBookDeltas` batch with `F_MBP` flags so a managed L1 book receives
the same top-of-book state. Quote and L1 subscriptions share the venue stream through
reference counting. The client rejects concurrent L1 and L2 subscriptions for the same
instrument.

Explicit order-book snapshot requests are supported separately from subscription
synchronization. Spot accepts depths from 1 through 5000. Futures accepts 5, 10, 20,
50, 100, 500, or 1000.

An order book snapshot rebuild is triggered on the initial order book subscription and on every
data WebSocket reconnect. The rebuild runs in this order:

1. Buffering of incoming deltas starts.
2. The snapshot is requested and awaited.
3. The snapshot response is parsed to `OrderBookDeltas`.
4. The snapshot deltas are sent to the `DataEngine`.
5. Buffered deltas are iterated, dropping those whose sequence number is not greater than the last
   delta in the snapshot.
6. Buffering stops.
7. The remaining deltas are sent to the `DataEngine`.

:::note
This snapshot-and-buffer sequence applies to Futures and Spot `BookDeltas`
subscriptions without an explicit depth. Spot partial-depth subscriptions deliver
self-contained top-N snapshots. See [Spot market data mode](#spot-market-data-mode).
:::

## Quote timestamps

The `ts_event` field on `QuoteTick` differs between transports. Spot SBE uses the
microsecond event timestamp. Spot public JSON `bookTicker` messages can omit an event
timestamp, in which case the adapter uses `ts_init`. Futures uses the transaction time.

## Bars and historical market data

Spot supports one-second klines for subscriptions and historical requests. Real-time
Spot kline subscriptions require `spot_market_data_mode=Json` because Binance does not
publish kline or ticker streams over Spot SBE. Binance Futures rejects second-level
klines because the Futures API does not offer them.

Closed venue klines emit a core `Bar` and a `BinanceBar` custom-data event. `BinanceBar`
retains quote volume, trade count, taker-buy base volume, and taker-buy quote volume.
Historical core bar requests return `Bar`; request `BinanceBar` custom data with
`bar_type` metadata to retain the extended fields in historical responses.

Real-time trade subscriptions use the `<symbol>@aggTrade` stream on Futures, because Binance only
publishes aggregated trades on the Futures WebSocket, and the individual `<symbol>@trade` stream on
Spot.

Historical trade requests without bounds use the recent-trades endpoint. A request with
time bounds uses aggregate trades and accepts at most 1000 records, so the source follows the
request rather than a config option. Spot passes the supplied bounds to `/api/v3/aggTrades`.
Futures accepts either bound within the last 24 hours; when both are supplied, the range must be
shorter than one hour.

Historical core bar requests accept externally aggregated time bars and use the corresponding
venue kline endpoint. Internally aggregated bars are built by the `DataEngine` from raw trade,
quote, or source-bar responses through the `bar_types` request parameter; the Binance data client
does not aggregate them.

## Binance specific data

Bars, mark prices, index prices, and funding rates are subscribed to in the normal way. The custom
data types below expose additional venue-specific fields that the core data types do not carry.

Binance Futures mark-price payloads preserve the venue `P` estimated settlement price in
`BinanceFuturesMarkPriceUpdate`. Nautilus also emits standard mark-price, index-price, and
funding-rate updates from the same stream. The optional USD-M `ap` moving-average field is
parsed at the transport boundary but is not exposed as domain or custom data.

### `BinanceSpotTicker`

Spot 24-hour ticker custom data requires public JSON market-data mode and an
`instrument_id` metadata value:

```python
from nautilus_trader.adapters.binance import BinanceSpotTicker
from nautilus_trader.model import ClientId
from nautilus_trader.model import DataType

self.subscribe_data(
    data_type=DataType(
        BinanceSpotTicker.__name__,
        metadata={"instrument_id": "BTCUSDT.BINANCE"},
    ),
    client_id=ClientId.from_str("BINANCE"),
)
```

The adapter subscribes to the instrument `@ticker` stream. SBE mode rejects this
subscription because Binance Spot SBE does not provide the stream.

### `BinanceFuturesTicker`

Subscribe to 24-hour ticker statistics for a specific Futures instrument:

```python
from nautilus_trader.adapters.binance import BinanceFuturesTicker
from nautilus_trader.model import ClientId
from nautilus_trader.model import DataType

client_id = ClientId.from_str("BINANCE")

self.subscribe_data(
    data_type=DataType(
        BinanceFuturesTicker.__name__,
        metadata={"instrument_id": "BTCUSDT-PERP.BINANCE"},
    ),
    client_id=client_id,
)
```

The adapter subscribes to the instrument `@ticker` stream and emits
`BinanceFuturesTicker` custom data with `metadata={"instrument_id": "<instrument_id>"}`.
Ticker custom data requires `instrument_id`; all-market ticker subscriptions are not
supported.

### `BinanceFuturesMarkPriceUpdate`

Subscribe to `BinanceFuturesMarkPriceUpdate` (including funding rate info)
from your actor or strategy:

```python
from nautilus_trader.adapters.binance import BinanceFuturesMarkPriceUpdate
from nautilus_trader.model import DataType
from nautilus_trader.model import ClientId

# In your `on_start` method
self.subscribe_data(
    data_type=DataType(
        BinanceFuturesMarkPriceUpdate.__name__, metadata={"instrument_id": self.instrument.id}
    ),
    client_id=ClientId("BINANCE"),
)
```

Received `BinanceFuturesMarkPriceUpdate` objects are passed to your `on_data`
method. Check the type, as this method handles all custom/generic data.

```python
def on_data(self, data):
    # First check the type of data
    if isinstance(data, BinanceFuturesMarkPriceUpdate):
        # Do something with the data
```

### `BinanceFuturesLiquidation`

Subscribe to liquidation updates for either:

- a specific instrument (`<symbol>@forceOrder`), or
- all symbols (`!forceOrder@arr`) by omitting `instrument_id`.

```python
from nautilus_trader.adapters.binance import BinanceFuturesLiquidation
from nautilus_trader.model import ClientId
from nautilus_trader.model import DataType

client_id = ClientId.from_str("BINANCE")

# Instrument-specific
self.subscribe_data(
    data_type=DataType(
        BinanceFuturesLiquidation.__name__,
        metadata={"instrument_id": "BTCUSDT-PERP.BINANCE"},
    ),
    client_id=client_id,
)

# All-market (no instrument_id metadata)
self.subscribe_data(
    data_type=DataType(BinanceFuturesLiquidation.__name__),
    client_id=client_id,
)
```

For instrument-specific subscriptions, `CustomData.data_type` includes
`metadata={"instrument_id": "<instrument_id>"}`. For all-market subscriptions,
the data type has no metadata.

When both modes are subscribed concurrently, all-market takes precedence. The
adapter suspends per-symbol liquidation streams while all-market is active, and
restores active per-symbol streams after all-market is unsubscribed.

### Futures open interest

Open interest is request-only; the Futures data client has no open interest subscription. Both
types require `instrument_id` metadata, and `BinanceFuturesOpenInterestHist` also requires a
Binance `period` string such as `"5m"`:

```python
from nautilus_trader.adapters.binance import BinanceFuturesOpenInterest
from nautilus_trader.adapters.binance import BinanceFuturesOpenInterestHist
from nautilus_trader.model import ClientId
from nautilus_trader.model import DataType

client_id = ClientId.from_str("BINANCE")

# Current open interest snapshot
self.request_data(
    data_type=DataType(
        BinanceFuturesOpenInterest.__name__,
        metadata={"instrument_id": "BTCUSDT-PERP.BINANCE"},
    ),
    client_id=client_id,
)

# Historical open interest series
self.request_data(
    data_type=DataType(
        BinanceFuturesOpenInterestHist.__name__,
        metadata={"instrument_id": "BTCUSDT-PERP.BINANCE", "period": "5m"},
    ),
    client_id=client_id,
)
```

`BinanceFuturesOpenInterestHist` returns a batch of points, each carrying the summed open interest
and its notional value for one bucket. COIN-M history is keyed by pair and contract type, which
the adapter derives from the symbol for perpetuals and from the cached instrument definition for
delivery contracts.

## Funding rates

The adapter emits `FundingRateUpdate` as a first-class data type through
`subscribe_funding_rates`. The data comes from the
[Mark Price Stream](https://developers.binance.com/docs/derivatives/usds-margined-futures/websocket-market-streams/Mark-Price-Stream)
WebSocket endpoint, which provides the current funding rate and next funding
time alongside mark and index prices. All three subscriptions
(`subscribe_mark_prices`, `subscribe_index_prices`, `subscribe_funding_rates`)
share a single `@markPrice@1s` stream with ref-counted subscription management.

Historical funding rates are available through `request_funding_rates`, which
queries the
[Get Funding Rate History](https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Get-Funding-Rate-History)
REST endpoint (`GET /fapi/v1/fundingRate` for USD-M, `GET /dapi/v1/fundingRate`
for COIN-M). Each history row maps to a `FundingRateUpdate` with `ts_event` set
to the funding time. The `next_funding_ns` field is `None` for historical rows
because the endpoint does not provide it.

The adapter also exposes the venue payload through `BinanceFuturesMarkPriceUpdate` custom data
subscriptions (see [Binance specific data](#binance-specific-data)).

The `interval` field on `FundingRateUpdate` is `None` for Binance because the
Mark Price Stream and the funding rate history endpoint do not include a
funding interval field. Binance exposes `fundingIntervalHours` through the
[Get Funding Rate Info](https://developers.binance.com/docs/derivatives/usds-margined-futures/market-data/rest-api/Get-Funding-Rate-Info)
REST endpoint, but the adapter does not consume it.

## Instrument status polling

The data clients periodically poll Binance `exchangeInfo` to detect changes in
instrument trading status. When a symbol transitions between states (e.g.
Trading to Halt, or Trading to Delivering for a futures contract approaching
expiry), the adapter emits an `InstrumentStatus` event.

The polling interval defaults to 3,600 seconds (60 minutes) and is configurable
via `instrument_status_poll_secs` in the data client config. Set to `0` to
disable polling entirely.

On initial connect, the adapter seeds its status cache from the exchange info
response without emitting events. Only subsequent polls that detect a status
change emit `InstrumentStatus` events. If a symbol disappears from exchange
info (e.g. after delisting or contract expiry), the adapter emits
`NotAvailableForTrading`.

Status polling does not reload instrument definitions. The separate
`instrument_refresh_interval_secs` task performs a complete filtered catalogue load, atomically
replaces the data-client and WebSocket lookup maps, sends the refreshed instruments to the data
engine, and updates the status snapshot. It also refreshes the execution client precision cache.
The default full refresh interval is 3,600 seconds; set it to `0` to disable it. Disconnect cancels
the task, and reconnect starts one replacement task with a new cancellation token.

### Status mapping

#### Spot

| Binance status   | MarketStatusAction     |
| ---------------- | ---------------------- |
| Trading          | Trading                |
| EndOfDay         | Close                  |
| Halt             | Halt                   |
| Break            | Pause                  |
| CancelOnly       | Halt                   |
| NonRepresentable | NotAvailableForTrading |

Binance US polls the public JSON exchange info instead, which maps `TRADING` to `Trading`, `BREAK`
to `Pause`, and every other value to `NotAvailableForTrading`.

#### Futures (USD-M)

| Binance status    | MarketStatusAction |
| ----------------- | ------------------ |
| Trading           | Trading            |
| PendingTrading    | PreOpen            |
| PreTrading        | PreOpen            |
| PostTrading       | PostClose          |
| EndOfDay          | Close              |
| Halt              | Halt               |
| AuctionMatch      | Cross              |
| Break             | Pause              |
| PreDelivering     | PreClose           |
| Delivering        | Close              |
| Delivered         | Close              |
| PreSettle         | PreClose           |
| Settling          | Close              |
| Close             | Close              |
| TradingHalt       | Halt               |
| TradingCancelOnly | Halt               |

#### Futures (COIN-M)

| Binance status    | MarketStatusAction     |
| ----------------- | ---------------------- |
| Trading           | Trading                |
| PendingTrading    | PreOpen                |
| PreDelivering     | PreClose               |
| Delivering        | Close                  |
| Delivered         | Close                  |
| PreSettle         | PreClose               |
| Settling          | Close                  |
| Close             | Close                  |
| PreDelisting      | PreClose               |
| Delisting         | Suspend                |
| Down              | NotAvailableForTrading |
| TradingHalt       | Halt                   |
| TradingCancelOnly | Halt                   |

Unknown or undocumented Futures status values map to `NotAvailableForTrading`.

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

### Venue weight limits

Binance's own per-IP weight allowance, which the endpoint costs below draw from:

| Product     | Weight limit | Interval |
| ----------- | ------------ | -------- |
| Spot/Margin | 6,000        | 1 minute |
| Futures     | 2,400        | 1 minute |

### Endpoint weight costs

Binance charges these weights per request:

| Endpoint                  | Weight | Notes                                  |
| ------------------------- | ------ | -------------------------------------- |
| `/api/v3/order`           | 1      | Spot order placement.                  |
| `/api/v3/allOrders`       | 20     | Spot historical orders (expensive).    |
| `/api/v3/klines`          | 2+     | Scales with `limit` parameter.         |
| `/fapi/v1/order`          | 1      | Futures order placement.               |
| `/fapi/v1/algoOrder`      | 0      | Uses order-count limits.               |
| `/fapi/v1/allOrders`      | 20     | Futures historical orders (expensive). |
| `/fapi/v1/commissionRate` | 20     | Futures commission rate query.         |
| `/fapi/v1/klines`         | 5+     | Scales with `limit` parameter.         |

USD-M Futures `POST /fapi/v1/algoOrder` consumes `1` from both
`X-MBX-ORDER-COUNT-10S` and `X-MBX-ORDER-COUNT-1M`. Binance charges no IP
request weight for this endpoint; the adapter still queues it through the
global bucket as part of its local pacing model.

### WebSocket API limits

The WebSocket API (used for order entry and user data streams) shares the same weight quota as the
REST API:

| Limit Type       | Value  | Notes                                 |
| ---------------- | ------ | ------------------------------------- |
| Request weight   | Shared | Counts against REST API weight quota. |
| Handshake        | 5      | Weight cost per connection attempt.   |
| Ping/pong frames | 5/sec  | Maximum ping/pong rate.               |

### Adapter pacing

The adapter runs its own token bucket limiters ahead of the venue's accounting. Every HTTP request
draws one token from a per-product request bucket, and every order operation draws an additional
token from the order-count buckets:

| Product                    | Requests     | Order operations             |
| -------------------------- | ------------ | ---------------------------- |
| Spot                       | 1,200/minute | 10/second, 100,000/day       |
| Futures (USD-M and COIN-M) | 2,400/minute | 300/10 seconds, 1,200/minute |

The order-count buckets cover every order operation the product supports, not just placement:
submit, OCO submit, batch submit, algo submit, modify (Spot modifies through cancel-replace), batch
modify, cancel, cancel-all, batch cancel, and algo cancel. Cancel-heavy and modify-heavy strategies
are throttled by these buckets as well. Non-order authenticated requests, such as leverage and
margin-type changes and listen-key keepalives, draw from the request bucket alone.

The request bucket counts calls rather than weight, so it does not mirror the venue's weight
accounting. A run of high-weight or dynamic-weight endpoints (`/api/v3/allOrders` at weight 20, or
`/klines` scaling with `limit`) spends venue weight faster than the local bucket accounts for.
Large history requests may need manual pacing. Monitor the `X-MBX-USED-WEIGHT-*` response headers
to track actual venue usage.

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

### Data client

| Option                             | Default   | Description                                                                    |
| ---------------------------------- | --------- | ------------------------------------------------------------------------------ |
| `product_type`                     | `Spot`    | One of `Spot`, `UsdM`, or `CoinM`.                                             |
| `environment`                      | `Live`    | One of `Live`, `Testnet`, or `Demo`.                                           |
| `base_url_http`                    | `None`    | Optional HTTP endpoint override.                                               |
| `base_url_ws`                      | `None`    | Optional market WebSocket endpoint override.                                   |
| `api_key` / `api_secret`           | `None`    | Required for Spot SBE; optional for public JSON and Futures data.              |
| `spot_market_data_mode`            | `Sbe`     | `Json` keeps the credential-free Global Spot path. Binance US requires `Json`. |
| `instrument_provider`              | default   | Loading, filters, parser-warning, and commission policy.                       |
| `instrument_refresh_interval_secs` | `3,600`   | Full catalogue refresh interval; `0` disables it.                              |
| `instrument_status_poll_secs`      | `3,600`   | Status-only exchange-info poll interval; `0` disables it.                      |
| `proxy_url`                        | `None`    | Proxy applied to HTTP and every market WebSocket connection.                   |
| `recv_window_ms`                   | `5,000`   | Signed HTTP receive window, inclusive range `1..=60000`.                       |
| `us`                               | `False`   | Route a live Spot JSON client to Binance US.                                   |
| `transport_backend`                | `Sockudo` | WebSocket transport backend.                                                   |

### Execution client

| Option                             | Default   | Description                                                             |
| ---------------------------------- | --------- | ----------------------------------------------------------------------- |
| `trader_id` / `account_id`         | Required  | Nautilus execution identity.                                            |
| `product_type`                     | `Spot`    | One of `Spot`, `UsdM`, or `CoinM`.                                      |
| `environment`                      | `Live`    | One of `Live`, `Testnet`, or `Demo`.                                    |
| `base_url_http`                    | `None`    | Optional HTTP endpoint override.                                        |
| `base_url_ws`                      | `None`    | Optional private stream override.                                       |
| `base_url_ws_trading`              | `None`    | Optional Global Spot or USD-M WebSocket trading override.               |
| `use_ws_trading`                   | `True`    | Use Global WebSocket order entry where supported; Binance US uses HTTP. |
| `ws_trading_setup_timeout_ms`      | `10,000`  | WebSocket trading authentication and setup timeout.                     |
| `instrument_provider`              | default   | Loading, filters, parser-warning, and commission policy.                |
| `instrument_refresh_interval_secs` | `3,600`   | Execution precision-cache refresh interval; `0` disables it.            |
| `proxy_url`                        | `None`    | Proxy applied to HTTP, private streams, and WebSocket trading.          |
| `recv_window_ms`                   | `5,000`   | Signed HTTP and WebSocket receive window, inclusive range `1..=60000`.  |
| `us`                               | `False`   | Route a live Spot execution client to Binance US.                       |
| `api_key` / `api_secret`           | `None`    | Global uses Ed25519 WebSocket auth; Binance US uses HMAC HTTP signing.  |
| `use_gtd`                          | `True`    | Use native USD-M GTD; see [GTD policy](#gtd-policy).                    |
| `use_position_ids`                 | `True`    | Expose Futures hedge-side position IDs.                                 |
| `oms_type`                         | `None`    | `None` selects Futures netting; use `Hedging` for dual-side mode.       |
| `default_taker_fee`                | `0.0004`  | Fallback for exchange-generated Futures fills.                          |
| `futures_leverages`                | `None`    | Initial leverage by Futures symbol.                                     |
| `futures_margin_types`             | `None`    | Initial margin type by Futures symbol.                                  |
| `treat_expired_as_canceled`        | `False`   | Map `EXPIRED` execution events to canceled events.                      |
| `use_trade_lite`                   | `False`   | Use the lower-latency USD-M trade-lite fill stream.                     |
| `bnfcr_currency`                   | `USDT`    | Currency used to resolve `BNFCR` balances and fees.                     |
| `transport_backend`                | `Sockudo` | WebSocket transport backend.                                            |

### Live node configuration

Use `BinanceDataClientConfig` with `BinanceDataClientFactory` and `BinanceExecClientConfig` with
`BinanceExecutionClientFactory`. The current Python examples show the complete
`LiveNode.builder(...)` configuration for data and execution clients.

### Futures Credits Trading Mode (BNFCR)

Binance Futures Credits Trading Mode is an EU regulatory mode in which the USD-M
futures wallet, margin, PnL, and fees are denominated in `BNFCR`: an internal credit
unit pegged 1:1 to USD that replaces stablecoin balances. Because `BNFCR` is not a
tradable asset, the adapter maps it to the `bnfcr_currency` execution config option
(default `USDT`) so account balances and commissions reconcile against the stablecoin
the traded contracts settle in. Set `bnfcr_currency` to `USDC` when trading
USDC-margined perpetuals. Any other unrecognized futures asset is registered as a
generic crypto currency rather than failing.

### Spot market data mode

`spot_market_data_mode` on `BinanceDataClientConfig` selects the Spot data
transport. It affects Spot only; Futures is unchanged.

| Mode   | Credentials        | Quotes       |
| ------ | ------------------ | ------------ |
| `Sbe`  | Ed25519 (required) | `bestBidAsk` |
| `Json` | None (public)      | `bookTicker` |

`Sbe` (default) uses Binance Simple Binary Encoding streams and requires Ed25519
keys (see [Key types](#key-types)); the client refuses to connect without them.
`Json` uses public streams with no credentials. Full Spot `BookDeltas`
subscriptions use the `<symbol>@depth` diff-depth stream on the selected transport, with REST
snapshot synchronization. Explicit depth subscriptions use partial-book snapshots
(see [Order books](#order-books)).

:::note
Exposed to Python as `BinanceSpotMarketDataMode` on
`nautilus_trader.adapters.binance`.
:::

### Key types

The adapter signs with **Ed25519** or **HMAC-SHA256**, auto-detecting the key type from your API
secret format, so no configuration is needed. A secret that parses as a PKCS#8 Ed25519 private key
signs with Ed25519; anything else is treated as an HMAC secret.

**Ed25519 is strongly recommended.** Binance recommends Ed25519 for its
superior performance and security. A future version of NautilusTrader will
require Ed25519 exclusively.

| Key Type | Data Clients | Execution Clients | Status                                           |
| -------- | ------------ | ----------------- | ------------------------------------------------ |
| Ed25519  | ✓            | ✓                 | **Recommended**                                  |
| HMAC     | ✓            | ✓                 | Deprecated, will be removed in a future version. |
| RSA      | -            | -                 | Not supported; register an Ed25519 key instead.  |

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

Download the
[Binance Asymmetric Key Generator](https://github.com/binance/asymmetric-key-generator)
from the releases page and run it to generate a keypair.

**Registering with Binance**

1. Log in to Binance and go to **Profile** -> **API Management**.
2. Click **Create API** and select **Self-generated**.
3. Paste the contents of your public key file, including the `-----BEGIN PUBLIC KEY-----`
   header and footer.
4. Configure permissions (Enable Spot & Margin Trading, etc.).

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
been removed for Spot; a client that finds one logs an error and treats the
credential as missing. For Futures they are deprecated, still honored with a
warning, and will be removed in a future version. Rename them to
`BINANCE_API_KEY` / `BINANCE_API_SECRET` (Ed25519 keys are now auto-detected).
:::

When the trading node starts, you receive confirmation of whether your
credentials are valid and have trading permissions.

### Product type

Configs select one supported product with the `product_type` field and
`BinanceProductType` enum:

- `SPOT`
- `USD_M` (USDT, USDC, or BNFCR collateral)
- `COIN_M` (cryptocurrency collateral)

:::note
Margin trading is not implemented. Other enum variants are rejected by the live clients and the
standalone instrument loader. See [Product support](#product-support).
:::

### Base URL overrides

Override the default base URLs for both HTTP REST and WebSocket APIs. This is
useful for configuring API clusters or when Binance has provided specialized
endpoints.

### Binance US

Set `us=True` on the config for first-class Binance US Spot routing. Binance US is
not a custom-URL alias: the switch selects `api.binance.us`, the public JSON stream, HMAC-signed
HTTP execution, and the port 443 listen-key private stream with periodic keepalive.

See the official Binance US [REST API](https://github.com/binance-us/binance-us-api-docs/blob/master/rest-api.md),
[market streams](https://github.com/binance-us/binance-us-api-docs/blob/master/web-socket-streams.md),
and [user data stream](https://github.com/binance-us/binance-us-api-docs/blob/master/web-socket-api.md)
documentation for the venue contracts behind this routing.

The supported combinations are deliberate:

- Data: `product_type=Spot`, `environment=Live`, `spot_market_data_mode=Json`.
- Execution: `product_type=Spot`, `environment=Live`; order entry uses HTTP and private events use
  the listen-key stream.
- Futures, Testnet, Demo, and Spot SBE configurations with `us=True` fail validation.

Binance US public JSON covers live market data, depth snapshots, recent and aggregate trade
history, and kline history. It uses account-wide maker and taker rates. Global Binance keeps its
existing credential-free Spot JSON behavior with `us=False` and
`spot_market_data_mode=Json`.

### Environments

Binance provides three trading environments, each with separate API
credentials and endpoints. The `environment` config option selects which to
use.

| Environment | Config value                 | Description                                         |
| ----------- | ---------------------------- | --------------------------------------------------- |
| **Live**    | `BinanceEnvironment.LIVE`    | Production trading with real funds (default).       |
| **Demo**    | `BinanceEnvironment.DEMO`    | Demo Trading with simulated Spot and Futures funds. |
| **Testnet** | `BinanceEnvironment.TESTNET` | Legacy Spot and Futures test network.               |

#### Live (production)

The default environment for live trading with real funds. Uses your main Binance
account credentials.

```python
config = BinanceExecClientConfig(
    trader_id=TraderId.from_str("TRADER-001"),
    account_id=AccountId.from_str("BINANCE-001"),
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    product_type=BinanceProductType.SPOT,
    # environment=BinanceEnvironment.LIVE (default)
)
```

| Variable             | Description      |
| -------------------- | ---------------- |
| `BINANCE_API_KEY`    | Live API key.    |
| `BINANCE_API_SECRET` | Live API secret. |

#### Demo trading

Practice trading with simulated funds on production infrastructure. Demo
accounts use the same Binance login as your live account but trade with
virtual balances.

**How to get demo credentials:**

1. Log in at [binance.com/en/demo-trading](https://www.binance.com/en/demo-trading).
2. Go to **API Management** and create a demo API key.
3. Demo keys work for Spot and Futures demo endpoints.

| Endpoint    | URL                        |
| ----------- | -------------------------- |
| Spot HTTP   | `demo-api.binance.com`     |
| Spot WS     | `demo-stream.binance.com`  |
| USD-M HTTP  | `demo-fapi.binance.com`    |
| USD-M WS    | `demo-fstream.binance.com` |
| COIN-M HTTP | `demo-dapi.binance.com`    |
| COIN-M WS   | `demo-dstream.binance.com` |

```python
config = BinanceExecClientConfig(
    trader_id=TraderId.from_str("TRADER-001"),
    account_id=AccountId.from_str("BINANCE-001"),
    api_key="YOUR_DEMO_API_KEY",
    api_secret="YOUR_DEMO_API_SECRET",
    product_type=BinanceProductType.SPOT,
    environment=BinanceEnvironment.DEMO,
)
```

| Variable                  | Description      |
| ------------------------- | ---------------- |
| `BINANCE_DEMO_API_KEY`    | Demo API key.    |
| `BINANCE_DEMO_API_SECRET` | Demo API secret. |

#### Testnet

A legacy test network with its own user accounts, balances, and order books.
Prefer `environment=BinanceEnvironment.DEMO` for new simulated trading
setups. Spot testnet remains at `testnet.binance.vision`; futures testnet
endpoints may route through the Demo Trading infrastructure.

**How to get Spot testnet credentials:**

1. Go to [testnet.binance.vision](https://testnet.binance.vision/).
2. Log in with GitHub.
3. Generate an Ed25519 or HMAC API key (the adapter does not support RSA keys).

**Futures testnet:** Existing configs with `BinanceEnvironment.TESTNET`
continue to work, but new Futures testing should use `BinanceEnvironment.DEMO`.

```python
config = BinanceExecClientConfig(
    trader_id=TraderId.from_str("TRADER-001"),
    account_id=AccountId.from_str("BINANCE-001"),
    api_key="YOUR_TESTNET_API_KEY",
    api_secret="YOUR_TESTNET_API_SECRET",
    product_type=BinanceProductType.SPOT,
    environment=BinanceEnvironment.TESTNET,
)
```

| Variable                             | Description                 |
| ------------------------------------ | --------------------------- |
| `BINANCE_TESTNET_API_KEY`            | Spot testnet API key.       |
| `BINANCE_TESTNET_API_SECRET`         | Spot testnet API secret.    |
| `BINANCE_FUTURES_TESTNET_API_KEY`    | Futures testnet API key.    |
| `BINANCE_FUTURES_TESTNET_API_SECRET` | Futures testnet API secret. |

:::note
Testnet credentials are completely separate from your live account. Market
data and liquidity differ from production.
:::

### Commission rate queries

The instrument provider controls both selection and fee policy:

```python
from nautilus_trader.adapters.binance import BinanceInstrumentProviderConfig

instrument_provider = BinanceInstrumentProviderConfig(
    load_all=False,
    load_ids=["BTCUSDT.BINANCE", "ETHUSDT.BINANCE"],
    filters={"quotes": ["USDT"], "bases": ["BTC", "ETH"]},
    log_warnings=True,
    query_commission_rates=True,
)
```

`load_all=False` selects only `load_ids`; venue filters then apply as an intersection. Supported
filters are `symbols`, `bases`, and `quotes`, plus `contract_types` for Futures. Values are a string
or non-empty list of strings, and matching is case-insensitive. The adapter rejects
`filter_callable`; use the supported declarative filters.

Every parsed instrument receives maker and taker fees:

- Spot uses the account-wide rate when credentials are present, otherwise 0.1% maker and taker.
- Futures uses the account VIP tier when credentials are present, otherwise VIP 0.
- `query_commission_rates=True` opts Global Spot and Futures into rate-limited exact per-symbol
  queries. A failed or invalid query falls back to the account or tier rate for that symbol.
- Binance US uses its account-wide commission rates because it does not expose the Global
  `account/commission` endpoint.

The exact-query behavior follows the Global Spot
[commission FAQ](https://github.com/binance/binance-spot-api-docs/blob/master/faqs/commission_faq.md)
and the USD-M
[user commission rate](https://developers.binance.com/docs/derivatives/usds-margined-futures/account/rest-api/User-Commission-Rate)
endpoint.

Exact queries require credentials. Because they issue one private request per selected symbol,
combine `load_ids` or filters with this option on large catalogues.

### Parser warnings

Some Binance instruments cannot be parsed into Nautilus objects if they contain
field values beyond what the platform handles. These instruments are skipped
with a warning.

To suppress these warnings:

```python
from nautilus_trader.adapters.binance import BinanceInstrumentProviderConfig

instrument_provider = BinanceInstrumentProviderConfig(
    load_all=True,
    log_warnings=False,
)
```

### Futures hedge mode

Binance Futures Hedge mode allows holding both long and short positions on the
same instrument simultaneously.

To use hedge mode, configure it on Binance, set
`oms_type=OmsType.HEDGING` on `BinanceExecClientConfig`, and keep `use_position_ids=True` to track
both venue position sides:

```python
from nautilus_trader.adapters.binance import BinanceExecClientConfig
from nautilus_trader.adapters.binance import BinanceProductType
from nautilus_trader.model import AccountId
from nautilus_trader.model import OmsType
from nautilus_trader.model import TraderId

config = BinanceExecClientConfig(
    trader_id=TraderId.from_str("TRADER-001"),
    account_id=AccountId.from_str("BINANCE-001"),
    product_type=BinanceProductType.USD_M,
    oms_type=OmsType.HEDGING,
    use_position_ids=True,
)
```

### COIN-M / USD-M architecture

Binance COIN-M Futures (CM / DAPI) and USD-M Futures (UM / FAPI) share a
unified architecture. This section covers the implications for the adapter.

See the [Important CM-UM Integration Notice](https://developers.binance.com/docs/derivatives/coin-margined-futures/Important-CM-UM-Integration-Notice)
for the full details.

#### WebSocket streams

Market-data stream payloads include `st` (symbol type: `1` = UM, `2` = CM) on
`<symbol>@aggTrade`, `<symbol>@ticker`, `<symbol>@bookTicker`,
`<symbol>@depth<levels>`, `<symbol>@miniTicker`, and all `!*@arr` streams.
UM-side single-symbol streams also include `ps` (pair symbol) on
`<symbol>@bookTicker`, `<symbol>@depth<levels>`, `<symbol>@miniTicker`, and
`<symbol>@rpiDepth`.

The adapter decodes JSON with `serde`, which ignores unknown fields by default, so these fields
are silently dropped.

All-market array streams (`!ticker@arr`, `!miniTicker@arr`, `!bookTicker`,
`!forceOrder@arr`, `!contractInfo`) deliver merged UM + CM content on both
`fstream` and `dstream`.

#### REST and WebSocket API

- Order placement and modification acknowledgement responses do not include
  `avgPrice` / `cumQuote` / `cumBase`. The adapter sources fills from the user
  data stream. Query endpoints (`GET /{f,d}api/v1/order`, `userTrades`) still
  return these fields.
- `PUT /dapi/v1/order` (COIN-M modify) requires both `price` and `quantity`.
  The adapter always sends both fields, falling back to the cached order's
  values for whichever the modify command omits.
- COIN-M conditional orders (STOP, TAKE_PROFIT, etc.) use the
  `/dapi/v1/algoOrder` endpoint. The adapter routes all futures conditional
  orders through the algo order API.
- `GET /dapi/v1/openOrders` with an invalid symbol returns error `-1121`.

#### Rate-limit pools

UM and CM share Binance rate-limit pools: 2400 weight/min per IP, plus
1200 orders/min and 300 orders/10s per account. Rust futures HTTP clients in the
same process share request-weight state across UM and CM for the same environment
or custom endpoint scope and configured egress path. They share order-count state
across UM and CM when authenticated with the same API key, regardless of egress
path.

Live, testnet, demo, and unrelated custom endpoint scopes remain isolated.
Different configured egress paths have separate request-weight state, while
different API keys have separate order-count state. Separate processes and
multiple API keys for one Binance account still require external coordination.

#### dualSidePosition

UM and CM share the same `dualSidePosition` setting. Changing it on either
side affects both. Ensure both UM and CM have no open orders or positions
before flipping the setting.

## Contributing

:::info
To contribute to the Binance adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
