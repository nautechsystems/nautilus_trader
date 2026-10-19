# Kraken

The Kraken adapter supports live market data and order execution for Kraken
Spot and Kraken Derivatives (Futures).

## Overview

The adapter is implemented in Rust with Python bindings and does not require an
external Kraken client library. Each data or execution configuration selects a
Spot or Futures client through its `product_type`.

The main Python components are:

- `KrakenDataClientConfig` and `KrakenExecutionClientConfig`: Live client
  configuration.
- `KrakenDataClientFactory` and `KrakenExecutionClientFactory`: Factories used
  by the trading node builder.
- `KrakenSpotHttpClient` and `KrakenFuturesHttpClient`: Lower-level HTTP access
  for direct requests.

The Rust crate also exposes `KrakenSpotWebSocketClient` and `KrakenFuturesWebSocketClient` for
lower-level WebSocket access.

Configure the adapter through a live trading node, or use the lower-level clients
for direct API access.

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/kraken/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/kraken/examples/)

## Kraken documentation

- [Kraken API documentation](https://docs.kraken.com/)
- [Kraken Spot REST API](https://docs.kraken.com/exchange/guides/rest/introduction)
- [Kraken Derivatives API](https://docs.kraken.com/exchange/guides/futures/introduction)

## Products

The adapter supports these product categories:

| Product type          | Supported | Notes                                               |
| --------------------- | --------- | --------------------------------------------------- |
| Spot currency pairs   | ✓         | Cash trading and margin on eligible pairs.          |
| Spot tokenized assets | ✓         | Loaded from Kraken's `tokenized_asset` asset class. |
| Futures               | ✓         | Instruments returned by the Kraken Futures API.     |

:::warning
Kraken Futures can return instrument definitions that need more than standard-precision mode's nine decimal places.
Keep [high-precision mode](../getting_started/installation.md#precision-mode) enabled for Futures. Standard-precision
mode continues to support Spot, but Futures clients fail to start or return instruments when any definition cannot
be parsed. Futures catalog requests return no partial result and never round, clamp, or omit an unsupported
definition.
:::

:::note
**Single product type per client**: Each Kraken data or execution client is configured for a single
`product_type` (`SPOT` or `FUTURES`). A single client does not span both markets.
:::

## Spot instrument fees

Spot instrument loading does not populate venue maker or taker fee rates and does
not call Kraken's `TradeVolume` endpoint.

A Spot API key without `Funds permissions - Query` (**Query Funds**) fails when
the execution client requests account state.

## Bar streaming

### Supported intervals

For Spot, subscribe to `EXTERNAL` bars to receive Kraken's WebSocket OHLC data.
The following intervals are available:

| Interval   | BarType specification |
| ---------- | --------------------- |
| 1 minute   | `1-MINUTE-LAST`       |
| 5 minutes  | `5-MINUTE-LAST`       |
| 15 minutes | `15-MINUTE-LAST`      |
| 30 minutes | `30-MINUTE-LAST`      |
| 1 hour     | `1-HOUR-LAST`         |
| 4 hours    | `4-HOUR-LAST`         |
| 1 day      | `1-DAY-LAST`          |
| 1 week     | `1-WEEK-LAST`         |
| 15 days    | `15-DAY-LAST`         |

:::note
**Futures limitation**: Kraken Futures does not support bar streaming via
WebSocket. Use `request_bars()` for historical bar data instead.
:::

### Bar emission latency

Kraken's [Spot WebSocket OHLC channel](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/ohlc)
updates the current, incomplete bar on trade events. It does not provide a
field that marks a bar as closed.

During normal streaming, the adapter buffers the current bar and emits it after receiving an update with a new
`interval_begin`. Thus, the delay depends on the first trade in the next interval. For a market with no
trades, it is not bounded to one bar period. When the WebSocket message handler stops, the adapter flushes its
buffered bars, including a current bar that may still be incomplete.

The adapter uses buffering instead of timer-based emission because:

- Timer-based emission could miss the final update before the bar closes.
- Kraken's updates are not guaranteed to arrive at exact interval boundaries.

:::tip
For lower bar latency, subscribe to trade ticks and use `INTERNAL` aggregation
with `BarAggregator`. Time bars use local timers, avoiding the wait for Kraken's
next-interval trade, and use the same aggregation behavior across venues.
:::

## Symbology

### Spot symbol normalization

The adapter uses different Bitcoin symbol conventions for Spot and Futures:

| Market  | Symbol format | Example            | Notes                                        |
| ------- | ------------- | ------------------ | -------------------------------------------- |
| Spot    | `BTC`         | `BTC/USD.KRAKEN`   | Adapter normalizes XBT to BTC at load time.  |
| Futures | `XBT`         | `PI_XBTUSD.KRAKEN` | Instrument symbols keep Kraken's native XBT. |

:::note
Kraken's REST API can return `XBT` for Bitcoin, while its WebSocket v2 API requires `BTC`. The adapter
normalizes Spot symbols to `BTC` when loading instruments, whether `XBT` appears as the base currency (for
example, `XBT/USD` to `BTC/USD`) or quote currency (for example, `ETH/XBT` to `ETH/BTC`). Futures instrument
symbols retain Kraken's native `XBT` format. Futures currency codes do not, and follow the same
[currency mapping](#currency-codes) as Spot.
:::

Kraken also uses `XDG` for Dogecoin in some Spot responses. The adapter
normalizes it to `DOGE`, including in quote currency symbols.

### Currency codes

The adapter maps Kraken's legacy asset codes to standard codes consistently across
instruments, balances, fees, and currency configuration:

- `XXBT` and `XBT` become `BTC`.
- `XXDG` and `XDG` become `DOGE`.
- `ZEUR` becomes `EUR`, and `ZUSD` becomes `USD`.

The mapping uses an explicit table, as Kraken's own CLI does, rather than stripping prefixes. `XTZ`, `XRP`,
`XLM`, `XAUT`, `ZRX`, and `ZEC` are valid codes. An unlisted code passes through unchanged.

Configuration accepts either spelling. For example,
`spot_positions_quote_currency="ZEUR"` and `"EUR"` both match a euro-quoted instrument.

Fees use the venue-reported currency when available. Futures fills can carry a fee
currency, which on an inverse contract is the base rather than the quote. Spot
`TradesHistory` reports only a fee amount, so those commissions use the instrument's
quote currency.

#### Migration from earlier currency codes

:::warning
Migrate or rebuild caches and databases written before currency normalization.
Stored instruments, fills, positions, and balances can use the earlier codes below.
:::

The earlier mappings differed by source:

- Instruments retained `XXBT`, `XETH`, `XXDG`, `ZUSD`, and `ZEUR`, as did their fills
  and positions. These become `BTC`, `ETH`, `DOGE`, `USD`, and `EUR`.
- Spot balances and the margin summary asset lost one leading `X` or `Z`. This left
  `XBT` and `XDG` instead of `BTC` and `DOGE`, and `TZ`, `RX`, and `AUT` instead of
  `XTZ`, `ZRX`, and `XAUT`. `KFEE` becomes `FEE` under the explicit mapping.
- Futures balances retained the wallet's spelling: cash and margin wallets use `xbt`, while the flex wallet
  uses `XBT`. Both become `BTC`. `usd` becomes `USD`.

#### Futures wallet aggregation

Futures balances with the same normalized code combine into one entry. Total and locked amounts sum across
wallets. Each wallet's locked amount is bounded to `[0, total]` for a non-negative total. A negative total
keeps its reported locked amount. The aggregate is not clamped. Free balance can therefore be negative when
reservations exceed the combined holding, preserving the actual shortfall.

Previously, separate wallet entries overwrote each other in the account, leaving only the last entry read.

#### Money precision

Normalized codes can resolve to built-in currencies with different precision.
Previously, unknown codes such as `ZEUR` and `ZUSD` registered as crypto currencies
with eight decimal places. Built-in `EUR` and `USD` use two, and `JPY` uses none.
This affects instrument quote currencies, REST fill commissions, and derived PnL.

Account balances retain eight decimal places because their parsers construct the currency directly rather than
use the registry. The Futures flex `portfolioValue` entry and account-wide USD margin entries also use this
eight-decimal currency. They previously used two-decimal `USD`, so their precision increases without loss.

### Spot markets

NautilusTrader uses normalized, slash-separated symbols for Kraken Spot
instruments. The adapter translates them to Kraken's native format internally.

**Instrument ID format**:

```python
from nautilus_trader.model import InstrumentId


InstrumentId.from_str("BTC/USD.KRAKEN")  # Spot BTC/USD
InstrumentId.from_str("ETH/USD.KRAKEN")  # Spot ETH/USD
InstrumentId.from_str("SOL/USD.KRAKEN")  # Spot SOL/USD
InstrumentId.from_str("BTC/USDT.KRAKEN")  # Spot BTC/USDT
InstrumentId.from_str("ETH/BTC.KRAKEN")  # Spot ETH/BTC (normalized from ETH/XBT)
```

### Futures markets

Kraken Futures symbols use contract prefixes:

- `PI_`: Perpetual inverse contracts (for example, `PI_XBTUSD`).
- `PF_`: Perpetual linear multi-collateral contracts (for example, `PF_XBTUSD`).
- `PV_`: Perpetual vanilla contracts (for example, `PV_XRPXBT`).
- `FI_`: Fixed-maturity inverse contracts (for example, `FI_XBTUSD_230929`).
- `FF_`: Fixed-maturity linear contracts.

Kraken's
[ticker symbol reference](https://support.kraken.com/articles/360022835891-ticker-symbols-derivatives) lists
the main contract prefixes. Use the returned instrument catalog to determine which contracts are available.
Example symbols can refer to expired or unlisted contracts.

**Instrument ID format**:

```python
from nautilus_trader.model import InstrumentId


InstrumentId.from_str("PI_XBTUSD.KRAKEN")  # Perpetual inverse BTC
InstrumentId.from_str("PI_ETHUSD.KRAKEN")  # Perpetual inverse ETH
InstrumentId.from_str("PF_XBTUSD.KRAKEN")  # Perpetual linear multi-collateral BTC
```

## Data capabilities

### Subscriptions (real-time)

| Data type           | Spot | Futures | Notes                                    |
| ------------------- | ---- | ------- | ---------------------------------------- |
| `QuoteTick`         | ✓    | ✓       | Spot ticker; Futures L2 book.            |
| `TradeTick`         | ✓    | ✓       |                                          |
| `OrderBookDeltas`   | ✓    | ✓       | Spot L2/L3 and Futures L2 updates.       |
| `OrderBookDepth`    | -    | -       | Use `OrderBookDeltas` with depth `10`.   |
| `Bar`               | ✓    | -       | Spot WS OHLC channel. See bar section.   |
| `MarkPriceUpdate`   | -    | ✓       | From futures ticker feed.                |
| `IndexPriceUpdate`  | -    | ✓       | From futures ticker feed.                |
| `FundingRateUpdate` | -    | ✓       | Perpetuals only.                         |
| `InstrumentStatus`  | -    | -       | Live clients do not emit status updates. |

### Requests (historical)

| Data type              | Spot | Futures | Notes                                  |
| ---------------------- | ---- | ------- | -------------------------------------- |
| `TradeTick`            | ✓    | ✓       |                                        |
| `Bar`                  | ✓    | ✓       |                                        |
| `OrderBook` (snapshot) | ✓    | ✓       | Via HTTP depth endpoint.               |
| `FundingRateUpdate`    | -    | ✓       | Client-side start/end/limit filtering. |

### L2 book checksum validation

Kraken's [Spot L2 `book` channel](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/book)
sends a CRC32 checksum for the top ten levels on each side in snapshots and updates.
The adapter validates it by default against a local shadow book, using `pair_decimals`
for prices and `lot_decimals` for quantities from `AssetPairs`. Some pairs have a wire
price scale one digit finer than their tick size. The instrument retains that finer
scale in its metadata so checksum rendering does not truncate price digits.
Kraken Futures `book` messages carry no checksum.

#### Recovery and stream identity

On a checksum mismatch, the adapter emits a `Clear` delta, drops the shadow book,
and unsubscribes and resubscribes the symbol at its configured depth with a snapshot.
It ignores updates until that snapshot arrives. Every `book` unsubscribe includes
its depth: Kraken keys subscriptions by symbol and depth, and defaults an omitted
unsubscribe depth to `10`.

Recovery runs serially with user subscription changes, so stale recovery never
cancels a replacement subscription. The subscribe request identifies each stream.
The adapter accepts frames only from the symbol's latest stream after Kraken confirms
that request, dropping frames from replaced subscriptions and superseded recoveries.
A snapshot changes the book only after it parses and applies successfully. A recovery
request made before an accepted snapshot or reconnect sends nothing after that event.

The adapter also emits a `Clear` delta when it drops a shadow book outside frame processing. This occurs on
these events:

- Reconnect.
- Confirmation of a new stream.
- Rejection of the latest subscribe request.
- Detection of an unconfirmed latest request by the snapshot watchdog.

#### Missing snapshots and rejected subscriptions

The snapshot watchdog is the only retry mechanism for `book` recovery. It runs even
with checksum validation disabled, because it also recovers streams that never
receive a snapshot. Each replacement subscription starts a fresh wait rather than
inheriting its predecessor's retry count.

If no snapshot arrives within 10 seconds, the data client requests it again. It doubles the wait between
requests (`10`, `20`, `40`, `80`, and `160` seconds), up to five requests. At the cap, it logs an error and
stops retrying. The book stays cleared and updates are dropped until a snapshot arrives. A subscription change
or reconnect starts recovery again.

Kraken's rejection of the latest subscribe request clears any held book and logs the
venue's reason and the next request time at error level. The rejection counts as one
failed request, so the next attempt follows after 20 seconds. Four more rejections
exhaust the allowance in about five minutes. A transient rejection can therefore
recover on the next attempt after 20 seconds. Rejections of superseded requests are ignored.

On reconnect, the adapter retires its recorded subscribes and replays each subscribe
with its original request ID. Replay responses match no active request. If Kraken
rejects a replay, the watchdog detects the missing snapshot within its base wait of
10 seconds.

#### Repeated checksum mismatches

Three mismatches on one instrument without a valid update between them disable
validation for that instrument and log an error. The adapter keeps its book as
received, preventing a resubscription loop when Kraken hashes it differently.
A valid snapshot does not reset the mismatch count.

To disable L2 checksum validation:

```python
config = KrakenDataClientConfig(
    validate_l2_checksum=False,
)
```

## L3 order book (market-by-order)

Kraken exposes Spot per-order book data via the WebSocket v2
[`level3` channel](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/level3) at
`wss://ws-l3.kraken.com/v2`. This gives venue order IDs, per-order quantities,
and true incremental events (`add`, `modify`, `delete`). The adapter hashes each
venue order ID into the `u64` `BookOrder.order_id` field used by NautilusTrader.

### Prerequisites

L3 subscriptions require Spot API credentials with `WebSocket interface - On`
permission, which the adapter uses to obtain an
[authentication token](https://docs.kraken.com/api-reference/trading/get-websockets-token).
Pass the credentials to `KrakenDataClientConfig`:

```python
from nautilus_trader.adapters.kraken import KrakenDataClientConfig

config = KrakenDataClientConfig(
    api_key="YOUR_KEY",
    api_secret="YOUR_SECRET",
)
```

Then subscribe with `book_type=BookType.L3_MBO`:

```python
from nautilus_trader.model import BookType

# Inside a registered strategy or data actor:
self.subscribe_book_deltas(
    instrument_id=instrument_id,
    book_type=BookType.L3_MBO,
    depth=1000,  # valid: 10, 100, 1000
)
```

Valid depths are `10`, `100`, and `1000`. A `depth` of `0` uses `1000`.

### CRC32 checksum validation

By default, the adapter validates the CRC32 checksum on each L3 snapshot and
update when Kraken provides one. On mismatch, it emits a `Clear` delta, clears
local L3 state, refreshes the auth token, and resubscribes so Kraken
sends a fresh snapshot. To disable validation for benchmarking:

```python
config = KrakenDataClientConfig(
    api_key="...",
    api_secret="...",
    validate_l3_checksum=False,
)
```

### Storage recommendations

`OrderBookDelta` carries `order_id: u64` in its Arrow schema, so `ParquetDataCatalog`
stores L3 data with the same schema as L2. L3 produces more events per instrument.

- Use a smaller query-session `chunk_size` (for example, `50_000`) to target smaller streaming batches.
  Timestamp-aligned batches can exceed that target. The setting does not control file size or parallel reads.
- Set `compression="zstd"` in `DataCatalogConfig` to compress stored data.
- The Parquet catalog partitions data by instrument by default.

## Order capabilities

### Order types

| Order type             | Spot | Futures | Notes                                                                 |
| ---------------------- | ---- | ------- | --------------------------------------------------------------------- |
| `MARKET`               | ✓    | ✓       | Immediate execution at market price.                                  |
| `LIMIT`                | ✓    | ✓       | Execution at specified price or better.                               |
| `STOP_MARKET`          | ✓    | ✓       | Conditional market order (stop-loss).                                 |
| `MARKET_IF_TOUCHED`    | ✓    | ✓       | Conditional market order (take-profit).                               |
| `STOP_LIMIT`           | ✓    | ✓       | Conditional limit order (stop-loss-limit).                            |
| `LIMIT_IF_TOUCHED`     | ✓    | ✓       | Spot: `take-profit-limit`; Futures: `take_profit` with `limit_price`. |
| `TRAILING_STOP_MARKET` | ✓    | -       | Trailing stop with `trailing_offset`.                                 |
| `TRAILING_STOP_LIMIT`  | ✓    | -       | Trailing stop-limit with `limit_offset`.                              |

### Time in force

| Time in force | Spot | Futures | Notes                                               |
| ------------- | ---- | ------- | --------------------------------------------------- |
| `GTC`         | ✓    | ✓       | Good Till Canceled.                                 |
| `GTD`         | ✓    | -       | Good Till Date (Spot only, requires `expire_time`). |
| `IOC`         | ✓    | ✓       | Immediate or Cancel.                                |
| `FOK`         | ✓    | -       | Spot limit orders only.                             |

:::note
**Market orders** are inherently immediate and do not support time-in-force.
`IOC` only applies to limit-type orders.
:::

### Execution instructions

| Instruction      | Spot | Futures | Notes                                                      |
| ---------------- | ---- | ------- | ---------------------------------------------------------- |
| `post_only`      | ✓    | ✓       | Available for limit orders.                                |
| `reduce_only`    | ✓    | ✓       | Spot requires a margin account and resolved leverage.      |
| `quote_quantity` | ✓    | -       | Spot only. Volume in quote currency (`viqc`); REST routed. |
| `display_qty`    | ✓    | -       | Spot only. Iceberg orders (`displayvol`).                  |

### Trigger types

Conditional orders support these trigger price references:

| Trigger type  | Spot | Futures | Notes                       |
| ------------- | ---- | ------- | --------------------------- |
| `LAST_PRICE`  | ✓    | ✓       | Default. Last traded price. |
| `INDEX_PRICE` | ✓    | ✓       | Broader market index price. |
| `MARK_PRICE`  | -    | ✓       | Futures only.               |

:::note
The adapter rejects unsupported trigger types (e.g., `BID_ASK`) at submission
time rather than silently coercing them.
:::

### Batch operations

| Operation    | Spot | Futures | Notes                                                  |
| ------------ | ---- | ------- | ------------------------------------------------------ |
| Batch submit | ✓    | ✓       | Spot chunks at 15 orders. Futures chunks at 10.        |
| Batch modify | -    | ✓       | Futures HTTP method only. Execution sends one command. |
| Batch cancel | ✓    | ✓       | Auto-chunks into batches of 50.                        |

:::note
**Cancel all orders**:

- Spot selects matching open and in-flight orders for the requested instrument, with or without a side filter.
  It cancels them by explicit order ID. Thus, a request never reaches another instrument. In-flight orders are
  included because the venue can have accepted an order the cache still records as submitted.
- Futures uses the venue's symbol-scoped bulk cancellation when no side filter is
  given, and selects matching cached open and in-flight orders by explicit order ID
  when one is.
- Selected IDs go through the batch-cancel endpoint and are auto-chunked into batches of 50. Kraken keys the
  two identifier kinds separately, so venue order IDs are sent as `orders` and client order IDs as
  `cl_ord_ids`. The batch limit counts both together. Each cancel keeps the owning strategy of the order it
  targets, and aggregate or ambiguous responses are left to reconciliation rather than producing per-order
  outcomes.

:::

### Order modification

A modify's quantity is the order's new total, fills included, which is how Kraken
Spot amends take `order_qty`. Kraken Futures reads a bare edit `size` as the open
size, so the adapter sends `qtyMode=ABSOLUTE` with every futures edit that resizes
the order, single or batch. Kraken Futures does not document its answer to a
total at or below the filled quantity.

### Position management

| Feature          | Spot | Futures | Notes                                                |
| ---------------- | ---- | ------- | ---------------------------------------------------- |
| Query positions  | ✓    | ✓       | Spot margin via `OpenPositions`; spot cash opt-in.   |
| Position mode    | -    | -       | Single position per instrument.                      |
| Leverage control | ✓    | -       | Spot REST tiers; per-order `params={"leverage": N}`. |
| Margin mode      | ✓    | ✓       | Spot/Futures cross margin; no isolated spot margin.  |

### Order querying

| Feature              | Spot | Futures | Notes                                           |
| -------------------- | ---- | ------- | ----------------------------------------------- |
| Query open orders    | ✓    | ✓       | List all active orders.                         |
| Query order history  | ✓    | ✓       | Spot paginates; Futures reads one history page. |
| Order status updates | ✓    | ✓       | Real-time order state changes via WebSocket.    |
| Trade history        | ✓    | ✓       | Execution and fill reports.                     |

### Contingent orders

| Feature            | Spot | Futures | Notes                                       |
| ------------------ | ---- | ------- | ------------------------------------------- |
| Linked order lists | -    | -       | Submitted lists contain independent orders. |
| OCO orders         | -    | -       | *Not supported*.                            |
| Bracket orders     | -    | -       | *Not supported*.                            |
| Conditional orders | ✓    | ✓       | Stop and take-profit orders.                |

### Maker Protection (Futures)

Kraken Futures applies [Maker Protection](https://docs.kraken.com/exchange/guides/futures/maker-protection) on
selected markets: placements and edits that could take liquidity are held for the market's configured window
before reaching the matching engine. The classification is by order type, so a non-post-only limit placement
is held even when it would have rested. Stop and take-profit trigger placements are not held. The orders they
fire are held unless post-only. Post-only placements and all cancellations are never held, and no held-order
state is exposed on any API.

The venue applies the hold per market to every client. The adapter decodes the per-market window
(`makerProtectionMillis`) on the raw venue instrument model and exposes no configuration for it.

Order-state handling accounts for the held-order semantics:

- A cancel acknowledged for a held limit placement is not terminal. The order is
  released as IOC and can still fill. Held market and IOC orders cannot be recalled:
  cancel returns `ORDER_NOT_FOUND`, and the order still reaches matching at release.
  Venue order updates drive fills and terminal states, never the cancel acknowledgement itself.
- An order that cannot trade after such a release is reported with the venue
  status `iocWouldNotExecute` on REST (`IOC_WOULD_ENTER_BOOK` on market data),
  which the adapter treats as a terminal rejection. On the order-update feed
  the same outcome arrives as a terminal cancellation whose venue reason the
  adapter preserves.
- A released order cancels a resting order of the same account it would
  match, overriding the configured self-trade strategy. The resting order is
  reported canceled with reason `CANCELLED_BY_SELF_TRADE`.

#### Order-update feed

A removal with `is_cancel=true` and reason `partial_fill` discards the remainder
and is terminal (a converted hold, or any IOC-style order). The delta carries
the venue's cumulative filled.

- For a tracked order, the adapter closes from the feed once the fills stream
  has accounted that quantity. A fill still in flight is never orphaned, and a
  tracked order is not left open after its fills are accounted.
- For a removal it cannot match, the adapter skips and converges through
  reconciliation.

| Unmatched removal             | Reason                        |
| ----------------------------- | ----------------------------- |
| Cancel-only message           | Carries no cumulative filled. |
| No resolvable client order ID | Cannot match a tracked order. |

#### Reconciliation

A held order never reaches the book, so it is absent from `/openorders`. Mass
status, open-only report runs, and targeted single-order queries consult
[`POST /orders/status`](https://docs.kraken.com/api-reference/order-management/get-specific-orders-status)
before treating the order as missing. That window reports
orders that are open or were filled or canceled in the last 5 seconds.

The venue echoes the client order ID an order was sent with, which the window is
asked about under both forms the venue can hold it under (see
[Client order IDs](#client-order-ids)). Mass status and open-only report runs map
each window report to a cached order:

- A cached venue order ID identifies the order directly.
- Otherwise, exactly one order queried by client order ID must hold the echoed
  ID, and exactly one report with an unknown venue order ID must carry it. The
  instrument and side must match, and the venue placement time must be no more
  than five seconds before the order's creation time, allowing for clock skew.
  This keeps out an older order whose ID shortens alike or that reused the ID.
  Two reports that would name one order, one echoing each form, name neither.

The adapter withholds any other window report with a warning naming the reason,
because it cannot tie that report to exactly one cached order.

The client order ID request returns every order holding the ID. A window
report of an order the open-orders or history read already returned names that
report by the cached order it describes and is then left out, so the order
reaches reconciliation once with the venue read's fields, such as time in force,
post-only and trigger fields, which the window lacks. When that report is open
and the window report is a later update, the report takes the window's status,
quantities, prices, cancel reason and update time, and the window rules below
apply to it. A
closed report keeps its state. A run scoped to one instrument keeps only that
instrument's window reports.

The venue order ID and client order ID requests run one after the other, so an
order asked about by venue order ID can come back from the client order ID
request too, in a different state. The adapter chooses the latest state regardless of
which request ran later: a closed state over an open one, then the later venue
update, then the larger filled quantity. The naming rule above and
the fill-price rule below apply to the report kept.

In those runs, the window carries no fill price. Thus, a fully executed order is left out with a warning. Its
executions are not inferred at the order price. A targeted single-order query prices it from recent fills
instead, or fails so reconciliation defers it. Mass status marks the set incomplete when it leaves out a
window report. A canceled order with executions is reported, since reconciliation waits for its fills rather
than infer them.

A hold that fills after a cancel acknowledgement reconciles to its true
terminal state with the venue's cumulative filled, not a premature cancellation.

## Order routing (Spot)

The Spot execution client routes order submission, modification, cancellation,
and batch cancellation through Kraken's authenticated WebSocket v2 trade
channel by default. It falls back to REST when the WebSocket is inactive. Set
`use_ws_trade=False` on `KrakenExecutionClientConfig` to route these operations
through REST.

### Order shapes routed via REST

Kraken's [Spot WebSocket v2 `add_order` method](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/add_order)
supports these shapes, but the adapter routes them through REST:

| Shape                      | Adapter behavior                                                  |
| -------------------------- | ----------------------------------------------------------------- |
| `FOK` time in force        | The WebSocket parameter builder does not encode `FOK`.            |
| Trailing stop / stop-limit | The WebSocket parameter builder does not encode trailing offsets. |
| Iceberg (`display_qty`)    | The WebSocket parameter builder does not encode iceberg orders.   |
| Quote-quantity orders      | WS supports non-margin buy market orders; the adapter uses REST.  |

Mixed-symbol order lists also use REST because Kraken's WebSocket `batch_add`
request requires one shared symbol. Unsupported trigger references fall back to
the REST path, which rejects them locally before sending a request to Kraken.
For Spot margin orders, use REST as described under [Spot margin trading](#spot-margin-trading).

The per-call `params={"use_ws_trade": False}` override forces a single
command through REST regardless of the configured default. Set it on
`SubmitOrder`, `ModifyOrder`, `CancelOrder`, `SubmitOrderList`, or
`BatchCancelOrders`.

### WebSocket request timeout

When a WebSocket round-trip exceeds `ws_request_timeout_secs` (default `5`),
the venue outcome remains unknown. Submit, modify, cancel, and batch-add
requests remain in flight without a terminal rejection. The dispatcher retains
the request ID so a delayed matching response can still apply the normal
success or definitive rejection handling.

Submit and batch-add timeouts also send a best-effort compensating cancel over
the same WebSocket for every affected client order ID. This cancel limits
exposure if Kraken accepted the order but delayed its response. It does not
replace the unknown outcome with local terminal state.

Stream updates and the live execution reconciliation engine resolve orders when
no matching response arrives. Targeted status queries can resolve modify or
cancel requests that already have a venue order ID. A matching response or
execution client shutdown retires the retained request correlation.

:::tip
Set `ws_request_timeout_secs` comfortably above your observed round-trip
latency. A premature timeout can send a compensating cancel for a submit or
batch add that Kraken accepted.
:::

### WebSocket order-routing options

`KrakenExecutionClientConfig` exposes:

| Option                    | Default | Description                                           |
| ------------------------- | ------- | ----------------------------------------------------- |
| `use_ws_trade`            | `True`  | Route orders via WS when the trade channel is active. |
| `ws_request_timeout_secs` | `5`     | Seconds to wait for a Spot WS order response.         |

## Reconciliation

Both execution clients reconcile local orders, fills, positions, and account
state with Kraken at startup and during operation.

### Bounded reports

When reconciliation supplies a lookback, both execution clients derive one cutoff
for every historical query and record it on the mass status through `set_report_window`.
A moving cutoff could combine reports that never existed together at the venue.

Declaring the cutoff enables the engine's bounded-history rules. An incomplete set still reconciles the
reports received, with a warning. The completeness flag qualifies the set rather than blocking reconciliation.

#### Scope and read failures

An in-scope open order or position that cannot be resolved to an instrument or parsed
fails the read on both clients. Omitting it would tell reconciliation that the venue
never held that order or position.

Futures makes one exception: an untouched reduce-only stop or take-profit can have no
unfilled size until it fires. If its contract has no open position from which to size
it, the read skips the order and its history rows with a warning and marks the set
incomplete. A failed open-position read fails the order read too. A single-order status
lookup for that trigger fails while it is skipped, so reconciliation defers it.

Both clients return no rows for a query scoped to an instrument they do not hold. Spot and Futures share the
`KRAKEN` venue, so either client can receive a query for an order it did not route. Single-order status
lookups and `query_order` inspect only the command's instrument. Unresolved or unparsable records on another
instrument cannot fail that scoped read, though a lookup cannot rule out an order such a record may name.

Other gaps mark the set incomplete: closed or historical orders and fills that cannot be parsed, or whose
instruments cannot be resolved. Position records do not contribute to the completeness flag. In-scope position
failures fail the read.

#### Single-order lookups

A single-order status lookup that finds no report tells reconciliation that the venue
has no such order, and reconciliation resolves the order on that answer. The lookup
gives that answer only from a read that rules the order out, and otherwise fails, so
reconciliation defers the order:

- A complete read rules out any order it could have returned. A held futures order
  shows only in the orders-status window, which the Futures lookup reads for a cached
  order before answering.
- A read that skipped a row, including one whose instrument the client cannot
  resolve, rules out an order with a venue order ID that no skipped row carries. A
  skipped row that names no order, such as an undocumented history event kind, rules
  out none.
- A lookup on an instrument of the other product reads complete, since the other
  client answers for its orders. One on an instrument of the client's own product
  that it does not hold reads nothing and rules out none.
- A read cut short rules out an order placed after the oldest event it holds, by more
  than a 60-second clock skew allowance, taking the earlier of the order's creation
  and acceptance times. Futures history pages are newest first, so a page that hands
  back a continuation token reaches back to its oldest event. A spot closed-order read
  stopped at its page cap rules out none.

An order a read cannot rule out is deferred again on every open-order check. On Futures each such lookup draws
one order-history token.

#### Spot pagination limits

Spot closed-order and fill reads advance an offset until Kraken returns an empty
page, with a cap of 500 pages per read. Reaching the cap logs a warning:

- Startup mass status records the order and fill reads' completeness in its report
  window, so the engine sees the truncated set as incomplete.
- `generate_order_status_reports` and `generate_fill_reports` return the records
  read up to the cap without exposing a completeness flag.
- A single-order status lookup that finds nothing in a closed-order read cut short
  by the cap fails, so reconciliation defers the order.

### Spot reconciliation

#### Order status reports

- Open orders: Fetches all currently active orders.
- Closed orders: Fetches historical orders with pagination support.
- Time-bounded queries: Supports filtering by start/end timestamps.
- Startup mass status reads closed orders alongside open ones, so an order that reached a terminal
  state while the node was down is reconciled. The reconciliation lookback bounds the read, and a
  closed-order read cut short by the page cap leaves the mass status incomplete.
- A report carries the order's own prices, not the row's top-level `price`. That field is the average fill
  price and reads zero on an unfilled order. The report's `avg_px` comes from `cost` and `vol_exec`.

  The limit comes from `descr.price` for a limit order and `descr.price2` for a stop-loss-limit or
  take-profit-limit order, or a trailing-stop-limit order once `misc` marks it `stopped`. A
  trailing-stop-limit's `limitprice` holds the peak price, not its limit. A market, settle-position,
  stop-loss, take-profit or trailing-stop order carries no limit.

  The trigger comes from `stopprice`, which a stop-loss, take-profit, stop-loss-limit or take-profit-limit
  order fills only once triggered, so `descr.price`, as AddOrder takes it, stands in before. A zero or
  relative price reads as none.
- An untriggered trailing-stop-limit order has no absolute limit, so its report carries no price,
  and an order the cache does not hold cannot be rebuilt from it until it triggers.
- An executions frame states the order's price only when it carries `limit_price`, never at a
  fill price.

#### Fill reports

- Trade history: Fetches execution history with pagination.
- Time-bounded queries: Supports filtering by start/end timestamps.
- All fill types: Market, limit, and conditional order fills.

#### Pair spelling

- Kraken spells a pair two ways: the `AssetPairs` key (`XXBTZEUR`), used as the instrument
  `raw_symbol`, and the altname (`XBTEUR`). `OpenPositions` returns the key, while `OpenOrders`
  and `TradesHistory` return the altname.
- The adapter resolves both spellings, so an order or fill on a legacy-named pair is reported.
- A read scoped to one instrument resolves each row and compares instrument IDs instead of the cached
  `raw_symbol` and venue spelling. Thus, it returns that pair's own records whichever way Kraken spells it.
- A read scoped to an instrument the client does not hold returns nothing. Spot and futures IDs share the
  `KRAKEN` venue, so a futures ID can reach the spot client and the reverse. Neither falls back to returning
  every instrument's records. A single-order lookup on an unheld instrument of the client's own product rules
  out no order.
- A spot instrument whose altname differs from its `AssetPairs` key carries the altname in its
  `info` map, so a client whose instruments arrive through `cache_instrument` or
  `cache_instruments` resolves altname-spelled records without refetching `AssetPairs`.
- On an unscoped read, an open order whose pair cannot be resolved to a cached instrument fails the
  read, rather than being omitted from an otherwise successful one. A scoped read skips a record it
  cannot resolve, which may be another instrument's, and a single-order lookup cannot rule out the
  order it names.
- A closed order or fill that cannot be resolved is logged as a warning and skipped, preserving the
  records that do resolve. Historical records routinely outlive the loaded instrument set.

#### Account balances

- Wallet balances: Fetched from
  [`POST /0/private/BalanceEx`](https://docs.kraken.com/api-reference/account-data/get-extended-balance),
  which reports the total and held (`hold_trade`) amount per asset. The held amount populates
  `AccountBalance.locked`, so `free` excludes funds Kraken has reserved against
  resting non-margin Spot orders. For accounts with a credit line, net credit (`credit - credit_used`)
  is included in `AccountBalance.total`, so `free` matches Kraken's available balance
  of `balance + credit - credit_used - hold_trade`.
- Zero balances: An asset Kraken lists at zero is reported at zero rather than omitted,
  on both spot and futures. The engine only ever inserts balances, so a currency left out
  of a snapshot keeps its previous value. On futures the zero joins the per-currency sum,
  so a funded wallet alongside an empty one of the same asset reports the funded amount.
  An asset Kraken drops from the response entirely still keeps its last reported value.

#### Margin position reports

With `spot_account_type=AccountType.MARGIN`:

- Open positions: Fetched from `POST /0/private/OpenPositions` and aggregated
  by pair into `PositionStatusReport` entries. Kraken returns one entry per lot,
  so opposing lots for the same pair net into a single report.
- Entry average: Each report carries `avg_px_open`, derived from the lot `cost`
  and `vol` fields and weighted by the volume still open. Long and short lots are
  averaged separately, so the reported average describes the side that survives
  netting. Reconciliation needs this value to open a position from a report when
  the cache holds no order or fill history for it.
- The average is marked `AvgPxReconciliation::OpeningOnly`, because Kraken closes margin lots
  FIFO and drops a fully closed one from `OpenPositions`. After a partial close the average
  therefore describes the lots that remain open rather than the opening fills, and would not match
  a netting position's average. Reconciliation uses it to open a position from flat and never
  compares it. Futures keeps the default, since that endpoint reports one netted position whose
  price Kraken documents as the average entry price.
- No synthetic FLAT cleanup: `OpenPositions` reports leveraged positions only, so an
  unleveraged spot holding never appears there and its absence is not evidence that the
  position is closed. The bulk read reports only what the venue returns.
- Margin balances: `POST /0/private/TradeBalance` is called alongside the account-state refresh. Used margin
  populates `MarginBalance.initial`, while equity and free margin populate the summary balance (see Spot
  margin trading).

:::warning
A leveraged position closed while the node was down is not recovered from its closing fill when
`reconciliation_lookback_mins` is set. A fully closed lot is absent from `OpenPositions`, so the instrument
carries no position report. Thus, the engine projects that order's fill as order-only. The order reaches
`FILLED`, but the cached position keeps both its quantity and realized PnL. The closing PnL is never recorded.

This is the shared engine's documented behavior for an instrument with no in-scope position report, not a
Kraken rule. See
[Order-only fill projection](../concepts/execution/reconciliation.md#order-only-fill-projection). Removing the
synthetic FLAT is what exposes Kraken spot margin to it, because the sweep previously supplied an explicit
FLAT.

A periodic position check does not recover it either, since margin mode declares no bulk position
coverage, and that skip is logged at debug level. The condition also persists across restarts: the
closing order is then cached as `FILLED` and matches the venue exactly, so reconciliation treats it
as already in sync.

Leaving `reconciliation_lookback_mins` unset avoids the projection. A closing order already in the cache, such
as a strategy exit submitted before the outage, then recovers into its position. The fill applies to the
cached order and closes its position.

It is not a general remedy, because a closing order absent from the cache is attributed to the `EXTERNAL`
strategy and keys a netting position by instrument and strategy. Unless the cached position is
`EXTERNAL`-owned or the instrument is claimed through `external_order_claim`, that recovered fill opens a
second, opposite position. It does not close the cached one. Net exposure reaches zero, but the stale position
and its realized PnL remain.

Until this is addressed, reconcile a margin position closed during downtime manually, or run
`spot_account_type=AccountType.CASH` with `use_spot_position_reports=True`, where the wallet read enumerates
every holding it covers and an absent report is genuine evidence of flat.
:::

### Futures reconciliation

#### Client order IDs

Futures orders are sent with their full client order ID: Kraken Futures takes
up to 100 characters, and an order whose ID is longer is denied before it is
sent, with every other open member of its order list. The venue reports a client
order ID it already holds, or one too long or otherwise invalid, as a send status
that rejects the order.

A cancel or edit names the order by one identifier: its venue order ID when
known, else its full client order ID.

Open-order and fill reports read the client order ID under the documented
`cliOrdId` key, and under `cli_ord_id` as well. An echoed ID that is empty or not
a valid client order ID, such as one another tool on the account sent, names no
order.

A futures order sent with its ID shortened to `O` and its last 17 characters, as
a spot ID longer than 18 characters is unless it is a UUID, is held under that
form. Every REST report and fill of an order the cache indexes by venue order ID
carries the cached client order ID, whatever the venue echoes, and the
orders-status window is asked about a held order under both forms. A cancel or
edit of such an order reaches it only by its venue order ID; one sent before a
report carrying that ID reaches the cache names no order at the venue and is
rejected.

A targeted single-order query takes the report carrying the order's venue order
ID when known. Otherwise a row is the order's when it echoes the client order ID
in either form, on the order's instrument and side, placed no more than five
seconds before the order's creation; an older row is another order whose ID
shortens alike or that reused the ID. A fill carries no placement time, so its
fill time stands in, which rules out only a fill executed before the order's
creation. A row or fill of a venue order the cache indexes for another order
never fits, whatever it echoes, since that order's full ID can be this order's
shortened one. With the venue order ID unknown, the query fails when one read holds
rows of two venue orders that fit, or a fitting row of a venue order the cache
indexes for no order that echoes the full ID of another cached order sent without
a venue order ID, on that order's instrument and side and placed no more than five
seconds before its creation, or, no row fitting, when the recent fills that fit
belong to two venue orders, include such a fill, or fall short of the order's
quantity; with it known,
the query fails when no row or fill carries it and a fitting row or fill of
another venue order appears. Either way reconciliation defers the order rather
than taking a row or reporting the order missing.

#### Order status reports

The adapter returns open orders and, when `open_only=False`, historical closed and
filled orders. Historical reads have the limits below.

#### Order history and pricing

The adapter reads order lifecycle events from
[`/api/history/v3/orders`](https://docs.kraken.com/api-reference/account-history/get-order-events). Each read
takes one page: Kraken `/history` endpoints share a pool of 100 tokens, replenished at 100 every 10 minutes.
An order-history page costs one token, so a page that hands back a continuation token, in the body or the
`Next-Continuation-Token` header, logs a warning and leaves the set incomplete rather than reading further. A
single-order status lookup can still report a cached order absent when it was placed after the page's oldest
event by more than the clock skew allowance. See [Single-order lookups](#single-order-lookups).

The history lists every lifecycle event. The read returns one report per order: the open-order snapshot if the
venue still lists it. Otherwise, it returns the latest history state. A closed order is not reopened by an
open state stamped later, such as a refused edit logged after a cancel.

The contract name is resolved as the venue spells it, exactly first and then case-insensitively. The history
carries no trigger price. Thus, a stop order is reported as the limit or market order it executes as once
triggered. Venue-initiated orders (liquidation, assignment, hedge assignment, unwind, block, RFQ) are reported
as market orders.

The adapter skips unrepresentable rows with a warning and marks the set incomplete:

- An unknown event kind.
- An order type reported as `Unknown` or omitted.
- An unknown direction.
- A timestamp before the epoch.

`OrderNotFound` carries no order state and is skipped without affecting completeness.
A venue error returned under an HTTP success status fails the read.

A terminal history row with executions carries no average price. The adapter prices
it from page fills that cover its filled quantity exactly, or from page fills plus
the fills recorded on a cached order, again requiring exact coverage. A failed fills
read counts as an empty page. Without exact coverage, a single-order query fails
and a bulk read withholds the order, so reconciliation defers it instead of inferring
executions at the limit price.

#### Startup history fallback

Startup mass status reads one history page alongside open orders. This recovers an
order that reached a terminal state during downtime when that page contains it.
A venue error body under an HTTP success status, regardless of its code, or HTTP
429 triggers a warning and fallback to open orders alone, with the set marked incomplete.
Other HTTP errors, transport failures, and unparsable bodies fail mass status,
as does a failed open-order read.

#### Startup pricing safeguard

Mass status applies the same exact fill-coverage requirement to terminal history orders with executions. It
withholds an uncovered order with a warning naming it and marks the set incomplete. Later reads still request
only the latest fills page. They cannot page back to recover an older missing execution.

A withheld order's page fills remain when the cache holds the order, because the
engine can reconcile those fills against it without a report. An uncached order's
fills are withheld with the order.

#### Flat instruments

The position read returns open positions only, so an instrument with no position report is flat at the venue.
The fills read is one page. A round trip with an opening fill older than that page leaves only its closing
side. Without `reconciliation_lookback_mins`, the engine applies every kept fill to positions. This can open a
position the venue does not hold.

To compensate, an unbounded startup read nets the fills of every order on the read pages that the cache does
not hold on a flat instrument, open orders included. When they do not net to zero and a terminal history order
is among them, the terminal orders are withheld with their fills, with a warning, and the set is marked
incomplete.

Open orders stay, so an open order's fill can still open a position. An instrument
with an open venue position is left to its position report. A bounded lookback leaves
terminal orders to the engine, which projects fills with no position report onto order state only.

#### Fill reports

- Fill history: Reads the latest
  [`/fills` page](https://docs.kraken.com/api-reference/historical-data/get-your-fills), the last 100 fills
  across all futures contracts. Older executions are not returned.
- Time filtering: Client-side filtering by start/end timestamps (parses
  RFC3339 timestamps).
- All fill types: Maker and taker fills with fee information.

#### Position status reports

- Open positions: Fetches all active futures positions.
- Report fields: Side, average entry price, and position size from the REST snapshot.
  Kraken also returns unrealized funding, but `PositionStatusReport` does not expose it.

#### Account state

- Balances: One entry per asset across wallets, as described under Currency codes.
- Margins: One entry per currency, summed across wallets as balances are, at eight decimals. A flex
  wallet's requirement is in USD, from its `initialMargin` and `maintenanceMargin`. A
  single-collateral wallet's is in its `currency`, so a `fi_xbtusd` requirement is reported in BTC,
  and its available funds bound that currency's balance alone. A wallet without a usable
  `currency` contributes no margin entry, reports none of its assets as locked, and logs a warning.

:::note
**Futures time filtering**: The Kraken Futures fills endpoint does not support
server-side time range filtering. The adapter implements client-side filtering
by parsing `fillTime` fields and comparing against requested start/end
timestamps.
:::

### Spot position reports (cash mode)

Set `use_spot_position_reports=True` in cash mode to report wallet balances as
Spot positions. It defaults to `False`. Margin accounts use `OpenPositions`
independently of this option (see [Spot margin trading](#spot-margin-trading)).

**How it works**:

- When enabled, wallet balances are converted to `PositionStatusReport` objects.
- Positive balances are reported as `LONG` positions.
- Only instruments matching the configured quote currency are reported (default: `USDT`).
  The same filter decides which instruments the client declares bulk position coverage for,
  so an instrument quoted in anything else is never reconciled to flat from a missing report.
- This prevents duplicate reports when the same asset is available with multiple
  quote currencies (e.g., BTC/USD, BTC/USDT, BTC/EUR).

**Configuration**:

```python
from nautilus_trader.adapters.kraken import KrakenExecutionClientConfig
from nautilus_trader.model import AccountId


exec_config = KrakenExecutionClientConfig(
    account_id=AccountId.from_str("KRAKEN-001"),
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    use_spot_position_reports=True,
    spot_positions_quote_currency="USDT",  # Default
)
```

:::warning
A strategy that closes reported Spot positions may sell your wallet holdings.
Enable wallet-derived position reports only when your strategy handles those holdings.
:::

## Spot margin trading

Kraken Spot supports leveraged trading on selected pairs. `AssetPairs` advertises
per-pair tiers in `leverage_buy` and `leverage_sell`. The adapter caches these at
instrument-load time and validates REST orders against them before submission.
Enable margin trading with `spot_account_type=AccountType.MARGIN` and set leverage
through `default_leverage` or per-order submission parameters.

:::warning
Use `use_ws_trade=False` for Spot margin orders. The adapter's WebSocket path
forwards numeric `leverage` without checking the pair's cached tiers. Kraken's
[WebSocket `add_order` schema](https://docs.kraken.com/exchange/api-reference/spot-websocket-v2/add_order)
instead specifies a boolean `margin` field that selects the pair's maximum leverage.
REST supports an explicit [leverage parameter](https://docs.kraken.com/api-reference/trading/add-order)
and the adapter's local tier checks.
:::

### Configuration

```python
from nautilus_trader.adapters.kraken import KrakenExecutionClientConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType


exec_config = KrakenExecutionClientConfig(
    account_id=AccountId.from_str("KRAKEN-001"),
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    spot_account_type=AccountType.MARGIN,
    use_ws_trade=False,
    default_leverage=3,  # Optional config-level default
    margin_balance_asset="ZGBP",  # Optional summary-display asset
)
```

`margin_balance_asset` controls only the denomination of the account-summary
metrics returned by Kraken's `TradeBalance` endpoint (equity, free margin,
used margin, etc.). Per-position figures from `OpenPositions` are always in
the traded pair's quote currency.

### Per-order leverage

Override the configured default on a single order via `params`:

```python
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity


order = strategy.order_factory.limit(
    instrument_id=InstrumentId.from_str("BTC/USD.KRAKEN"),
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("0.01"),
    price=Price.from_str("50000.00"),
)
strategy.submit_order(order, params={"leverage": 5})
```

On REST, the adapter validates the tier against the pair's `leverage_buy` or
`leverage_sell` before sending the order. An unsupported tier produces an
`OrderRejected` event for a single-order submission without reaching Kraken.
A malformed per-order leverage parameter, such as a string or a value outside
`[0, 65535]`, produces `OrderDenied` before submission on either route.

### Reduce-only

Margin orders can carry `reduce_only=True` so they reduce an existing position
without opening a larger opposite position. Set `spot_account_type=AccountType.MARGIN` and supply
either `default_leverage` or per-order `params={"leverage": N}`. The adapter denies
cash orders with `reduce_only` before sending them to Kraken.

### Account state

When `spot_account_type=AccountType.MARGIN`, the execution client calls Kraken's
`TradeBalance` endpoint during account refreshes. The live account state uses:

- Equity (`e`) and free margin (`mf`) for the balance denominated by
  `margin_balance_asset`.
- Used margin (`m`) for `MarginBalance.initial`. Maintenance margin is zero
  because Kraken does not return a separate maintenance-margin amount.

The lower-level `KrakenSpotHttpClient` methods `request_margin_metrics()` and
`request_account_state_with_metrics()` return the full `TradeBalance` metrics
dictionary for direct consumers. The live execution client does not attach
that dictionary to `AccountState.info`.

### Position reconciliation

Open spot margin positions are surfaced via `POST /0/private/OpenPositions`
on each `position_check_interval_secs` tick. This path is independent of
`use_spot_position_reports` (which is wallet-derived, cash-mode-only).

The spot client declares bulk position coverage per instrument, and only for instruments the
read would actually enumerate: cash mode with `use_spot_position_reports=True`, and the
instrument quoted in `spot_positions_quote_currency` (see Spot position reports, which applies
the same filter). Under `spot_account_type=AccountType.MARGIN` the source is `OpenPositions`, which omits
unleveraged lots, and cash mode without wallet-derived reports returns nothing at all. Wherever
coverage is not declared, an absent report leaves the cached position untouched instead of
closing it.

## Funding rates

The adapter receives funding rate data from the
[Futures ticker](https://docs.kraken.com/exchange/api-reference/futures-websocket/ticker)
WebSocket feed, which provides `relative_funding_rate` and
`next_funding_rate_time` for perpetual futures.

The adapter leaves `FundingRateUpdate.interval` as `None` because the ticker feed does not include a funding
interval field. This does not mean the contract has no defined funding period. Kraken's
[linear multi-collateral contract specification](https://support.kraken.com/articles/4844359082772-linear-multi-collateral-derivatives-contract-specifications)
defines hourly funding periods. Accrued funding settles at the end of the hour or when the net position
changes.

## Rate limiting

Each Kraken HTTP client applies an adapter-side request throttle. The default is
five requests per second and `max_requests_per_second` can override it. This is
a request-count throttle, not a complete model of Kraken's endpoint costs or
account-tier budgets.

Kraken applies different venue limits to Spot and Futures:

- [Spot REST rate limits](https://docs.kraken.com/exchange/guides/rest/ratelimits)
  use a tier-dependent call counter. Ledger and trade history calls add `2`,
  most other REST calls add `1`, and order management uses a separate trading
  limiter.
- [Derivatives rate limits](https://docs.kraken.com/exchange/guides/futures/ratelimits)
  use endpoint costs and separate budgets for `/derivatives` and `/history`
  paths.

Kraken documents these Spot REST call-counter limits:

| Spot tier    | Maximum counter | Counter decay |
| ------------ | --------------- | ------------- |
| Starter      | 15              | 0.33/second   |
| Intermediate | 20              | 0.5/second    |
| Pro          | 20              | 1/second      |

If the adapter's fixed request rate is too high for the endpoint mix and account
tier, Kraken can still reject or throttle requests.

### Reconciliation interval guidance

The execution engine's `open_check_interval_secs` and
`position_check_interval_secs` settings create sustained private REST API load.
Short intervals can exhaust Kraken's venue budgets even when the adapter stays
below its configured requests-per-second throttle.

Use conservative intervals as a starting point, especially for a Spot Starter
account:

```python
from nautilus_trader.config import LiveExecutionEngineConfig


exec_engine = LiveExecutionEngineConfig(
    reconciliation=True,
    open_check_interval_secs=30.0,  # Conservative Spot Starter-tier starting point
    position_check_interval_secs=120.0,
)
```

Tune these values for the account tier, enabled reconciliation checks, and other
clients using the same API key. If Kraken returns `EAPI:Rate limit exceeded`,
increase the intervals or reduce `max_requests_per_second`.

## Configuration

### Data client configuration options

| Option                    | Default   | Description                                                      |
| ------------------------- | --------- | ---------------------------------------------------------------- |
| `product_type`            | `SPOT`    | Product type for this client (`SPOT` or `FUTURES`).              |
| `environment`             | `LIVE`    | Trading environment (`LIVE` or `DEMO`); demo only for Futures.   |
| `api_key`                 | `None`    | API key for Spot L3 data.                                        |
| `api_secret`              | `None`    | API secret for Spot L3 data.                                     |
| `base_url`                | `None`    | Override for the Kraken REST base URL.                           |
| `ws_public_url`           | `None`    | Override for the public WebSocket URL.                           |
| `ws_private_url`          | `None`    | Override for the private WebSocket URL.                          |
| `ws_l3_url`               | `None`    | Override for the Spot L3 WebSocket URL.                          |
| `validate_l3_checksum`    | `True`    | Validate Kraken Spot L3 checksums and resync on mismatch.        |
| `validate_l2_checksum`    | `True`    | Validate Kraken Spot L2 `book` checksums and resync on mismatch. |
| `proxy_url`               | `None`    | Optional proxy URL for HTTP and WebSocket transports.            |
| `timeout_secs`            | `30`      | HTTP request timeout in seconds.                                 |
| `heartbeat_interval_secs` | `30`      | WebSocket heartbeat interval in seconds.                         |
| `ws_idle_timeout_ms`      | `10,000`  | Data-silence timeout for the Spot v2 WebSocket; `0` disables.    |
| `max_requests_per_second` | `None`    | Per-client request throttle; default is 5 req/s.                 |
| `transport_backend`       | `Sockudo` | WebSocket transport backend.                                     |

### Execution client configuration options

| Option                          | Default   | Description                                                           |
| ------------------------------- | --------- | --------------------------------------------------------------------- |
| `account_id`                    | required  | Account ID for the Kraken account.                                    |
| `api_key`                       | required  | Kraken API key.                                                       |
| `api_secret`                    | required  | Kraken API secret.                                                    |
| `product_type`                  | `SPOT`    | Product type for this client (`SPOT` or `FUTURES`).                   |
| `environment`                   | `LIVE`    | Trading environment (`LIVE` or `DEMO`); demo only for Futures.        |
| `base_url`                      | `None`    | Override for the Kraken REST base URL.                                |
| `ws_url`                        | `None`    | Override for the Kraken WebSocket URL.                                |
| `proxy_url`                     | `None`    | Optional proxy URL for HTTP and WebSocket transports.                 |
| `timeout_secs`                  | `30`      | HTTP request timeout in seconds.                                      |
| `heartbeat_interval_secs`       | `30`      | WebSocket heartbeat interval in seconds.                              |
| `auth_timeout_secs`             | `None`    | Futures WebSocket auth timeout; `None` uses the client default.       |
| `max_requests_per_second`       | `None`    | Per-client request throttle; default is 5 req/s.                      |
| `max_retries`                   | `3`       | Maximum retry attempts for retryable REST requests.                   |
| `spot_account_type`             | `CASH`    | Account type for spot trading; `MARGIN` enables leverage and reports. |
| `default_leverage`              | `None`    | Default Spot margin leverage; REST sends `"N:1"` when set.            |
| `use_spot_position_reports`     | `False`   | Report wallet balances as positions; cash mode only.                  |
| `spot_positions_quote_currency` | `"USDT"`  | Quote filter for spot wallet position reports and their coverage.     |
| `margin_balance_asset`          | `None`    | Summary asset for `TradeBalance`; `None` defaults to `ZUSD`.          |
| `use_ws_trade`                  | `True`    | Use Spot WebSocket v2 for order operations when active.               |
| `ws_request_timeout_secs`       | `5`       | Spot WebSocket order response timeout.                                |
| `transport_backend`             | `Sockudo` | WebSocket transport backend.                                          |

For Spot margin, `default_leverage` applies when an order has no per-order leverage
parameter. See [Spot margin trading](#spot-margin-trading) for submission and account-state examples.

### Demo environment setup

To test with Kraken Futures demo (paper trading):

1. Sign up at [Kraken Futures demo](https://demo-futures.kraken.com)
   and generate API credentials.
1. Set environment variables with your demo credentials:
   - `KRAKEN_FUTURES_DEMO_API_KEY`
   - `KRAKEN_FUTURES_DEMO_API_SECRET`
1. Read the credentials and pass them to `KrakenExecutionClientConfig`, then set
   `environment=KrakenEnvironment.DEMO` and
   `product_type=KrakenProductType.FUTURES`.

The [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/kraken/)
show the complete demo and live `LiveNode` configurations.

### Production configuration

Use `KrakenDataClientConfig` with `KrakenDataClientFactory`, and use
`KrakenExecutionClientConfig` with `KrakenExecutionClientFactory`. The Python
examples show the complete `LiveNode.builder(...)` configuration for data and
execution clients.

### API credentials

Live-node configuration objects do not read credential environment variables
automatically. Pass `api_key` and `api_secret` explicitly to
`KrakenExecutionClientConfig` and, for Spot L3 data, to
`KrakenDataClientConfig`. Market data other than Spot L3 does not require credentials.

The lower-level Python HTTP clients load the following variables
when their credential arguments are omitted. Rust applications can use
`KrakenCredential::from_env_spot()` or
`KrakenCredential::from_env_futures(demo)` to load them before constructing
live-node configs.

| Environment variable             | Description                              |
| -------------------------------- | ---------------------------------------- |
| `KRAKEN_SPOT_API_KEY`            | API key for Kraken Spot live trading.    |
| `KRAKEN_SPOT_API_SECRET`         | API secret for Kraken Spot live trading. |
| `KRAKEN_FUTURES_API_KEY`         | Kraken Futures live API key.             |
| `KRAKEN_FUTURES_API_SECRET`      | Kraken Futures live API secret.          |
| `KRAKEN_FUTURES_DEMO_API_KEY`    | API key for Kraken Futures (demo).       |
| `KRAKEN_FUTURES_DEMO_API_SECRET` | API secret for Kraken Futures (demo).    |

:::note
**Demo environment**: Only Kraken Futures offers a demo environment
(`https://demo-futures.kraken.com`) for testing without real funds. Kraken Spot
does not have a demo or testnet environment.
:::

:::tip
Use environment variables to store credentials, then pass their values into
live-node configuration at the application boundary.
:::

Authentication errors are reported when a private client connects or performs a
private operation. Required permissions depend on the requested data or trading
operation.

## Contributing

:::info
To contribute to the Kraken adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
