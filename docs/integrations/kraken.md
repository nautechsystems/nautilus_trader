# Kraken

Kraken offers spot and derivatives trading across a wide range of digital
assets. This integration connects to Kraken Pro and supports live market data
and order execution for Kraken Spot and Kraken Derivatives (Futures).

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

:::note
Most users configure these components through a live trading node and do not
need to work directly with the lower-level clients.
:::

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/kraken/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/kraken/examples/)

## Kraken documentation

Kraken provides detailed documentation for users:

- [Kraken API documentation](https://docs.kraken.com/)
- [Kraken Spot REST API](https://docs.kraken.com/exchange/guides/rest/introduction)
- [Kraken Derivatives API](https://docs.kraken.com/exchange/guides/futures/introduction)

Refer to the Kraken documentation in conjunction with this NautilusTrader
integration guide.

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
**Single product type per client**: Each Kraken data or execution client is
configured for a single `product_type` (`SPOT` or `FUTURES`); a single client
does not span both markets.
:::

## Spot instrument fees

Spot instruments do not carry maker or taker fee rates. Loading instruments does
not call Kraken's `TradeVolume` endpoint.

A Spot API key without `Funds permissions - Query` (**Query Funds**) fails when
the execution client requests account state.

## Bar streaming

### Supported intervals

The Kraken adapter supports real-time bar (OHLC) streaming for Spot markets via
WebSocket. The following intervals are available:

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

During normal streaming, the adapter buffers the current bar and emits it after
receiving an update with a new `interval_begin`. The delay therefore depends on
the first trade in the next interval and is not bounded to one bar period when a
market has no trades. When the WebSocket message handler stops, the adapter
flushes its buffered bars, including a current bar that may still be incomplete.

The adapter uses buffering instead of timer-based emission because:

- Timer-based emission could miss the final update before the bar closes.
- Kraken's updates are not guaranteed to arrive at exact interval boundaries.

This favors the latest venue update at the cost of latency.

:::tip
If bar latency matters for your strategy, consider using trade tick data
and aggregating bars locally with `BarAggregator`.
:::

:::tip
For most use cases, we recommend using `INTERNAL` bar aggregation (subscribing to
trades and aggregating bars locally) rather than `EXTERNAL` exchange-provided bars:

- Bars are emitted immediately when complete, with no buffering delay.
- Consistent behavior across all exchanges, simplifying multi-venue strategies.

:::

## Symbology

### Spot symbol normalization

Kraken uses different Bitcoin symbol conventions across their APIs:

| Market  | Symbol Format | Example            | Notes                                        |
| ------- | ------------- | ------------------ | -------------------------------------------- |
| Spot    | `BTC`         | `BTC/USD.KRAKEN`   | Adapter normalizes XBT to BTC at load time.  |
| Futures | `XBT`         | `PI_XBTUSD.KRAKEN` | Instrument symbols keep Kraken's native XBT. |

:::note
Kraken's REST API can return `XBT` for Bitcoin, while its WebSocket v2 API
requires `BTC`. The adapter normalizes Spot symbols to `BTC` when loading
instruments, whether `XBT` appears as the base currency (for example, `XBT/USD`
to `BTC/USD`) or quote currency (for example, `ETH/XBT` to `ETH/BTC`). Futures
instrument symbols retain Kraken's native `XBT` format; futures currency codes do
not, and are mapped like every other code (see Currency codes).
:::

Kraken also uses `XDG` for Dogecoin in some Spot responses. The adapter
normalizes it to `DOGE`, including in quote currency symbols.

### Currency codes

Kraken reports some assets under legacy codes, prefixing them with `X` or `Z`: `XXBT` for Bitcoin,
`ZEUR` for the euro. The adapter maps those to the standard code used everywhere else on the
platform, so instruments, balances, fees and currency configuration all agree: `XXBT` and `XBT`
become `BTC`, `XXDG` and `XDG` become `DOGE`, `ZEUR` becomes `EUR`, `ZUSD` becomes `USD`.

The mapping is an explicit table rather than a prefix rule, because the prefix is not a rule. `XTZ`,
`XRP`, `XLM`, `XAUT`, `ZRX` and `ZEC` legitimately begin with those letters, and a code the table
does not list passes through unchanged. Kraken's own CLI normalizes the same way.

Fees are booked in the currency the venue reports, where it reports one. Futures fills carry a fee
currency, which on an inverse contract is the base rather than the quote. Kraken's Spot
`TradesHistory` reports a fee amount without a currency, so those fills are booked in the
instrument's quote currency.

:::warning
This changes the currency codes the adapter emits, in three places that previously disagreed with
each other.

Instruments carried Kraken's codes unchanged, so stored instruments were denominated in `XXBT`,
`XETH`, `XXDG`, `ZUSD` and `ZEUR`, and so were the fills and positions that reference them. Those
become `BTC`, `ETH`, `DOGE`, `USD` and `EUR`.

Spot balances and the margin balance asset stripped one leading `X` or `Z`, so stored records carry
`XBT` and `XDG` rather than `BTC` and `DOGE`, and the corrupted forms `TZ`, `RX` and `AUT` rather
than `XTZ`, `ZRX` and `XAUT`. `KFEE` becomes `FEE`.

Futures balances used the venue's own spelling, which differs per wallet: cash and margin wallets
key an asset `xbt` while the flex wallet keys it `XBT`. Both become `BTC`, and `usd` becomes `USD`.
Because the spellings now meet under one code, an asset held in several wallets is reported as one
balance whose total and locked amounts are the sum of the wallets', each wallet's locked amount
bounded to its own total first and the sums then reported as they are. Free can therefore be
negative when one wallet's reservation exceeds the combined holding, which is a real shortfall
rather than something to clamp away. Previously each wallet produced its own entry and the account
kept whichever it read last.

A cache or database written by an earlier version needs migrating or rebuilding.

Configuration follows the same mapping and accepts either spelling, so
`spot_positions_quote_currency="ZEUR"` and `"EUR"` both match a euro-quoted instrument.

Money precision changes where a code now resolves to a built-in currency. `ZEUR` and `ZUSD` were
unknown to the platform and were registered as 8-decimal crypto; `EUR` and `USD` are built-in fiat
with 2 decimals, and `JPY` with none. That affects the instrument quote currency, REST fill
commissions and the PnL derived from them. Account balances keep their 8-decimal precision, because
the balance parsers construct their own currency from the code rather than resolving a registered
one. Two exceptions run the other way: the futures flex `portfolioValue` entry and the account-wide
USD margin entries were built on the 2-decimal `USD` and now share the 8-decimal balance currency,
which widens them without loss.
:::

### Spot markets

NautilusTrader uses normalized, slash-separated symbols for Kraken Spot
instruments. The adapter translates them to Kraken's native format internally.

**Instrument ID format:**

```python
InstrumentId.from_str("BTC/USD.KRAKEN")  # Spot BTC/USD
InstrumentId.from_str("ETH/USD.KRAKEN")  # Spot ETH/USD
InstrumentId.from_str("SOL/USD.KRAKEN")  # Spot SOL/USD
InstrumentId.from_str("BTC/USDT.KRAKEN")  # Spot BTC/USDT
InstrumentId.from_str("ETH/BTC.KRAKEN")  # Spot ETH/BTC (normalized from ETH/XBT)
```

### Futures markets

Kraken Futures instruments use a specific naming convention with prefixes:

- `PI_` - Perpetual Inverse contracts (e.g., `PI_XBTUSD`)
- `PF_` - Perpetual Fixed-margin contracts (e.g., `PF_XBTUSD`)
- `PV_` - Perpetual Vanilla contracts (e.g., `PV_XRPXBT`)
- `FI_` - Fixed maturity Inverse contracts (e.g., `FI_XBTUSD_230929`)
- `FF_` - Flex futures contracts

**Instrument ID format:**

```python
InstrumentId.from_str("PI_XBTUSD.KRAKEN")  # Perpetual inverse BTC
InstrumentId.from_str("PI_ETHUSD.KRAKEN")  # Perpetual inverse ETH
InstrumentId.from_str("PF_XBTUSD.KRAKEN")  # Perpetual fixed-margin BTC
```

## Data capability

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

Kraken sends a CRC32 checksum with each Spot `book` snapshot and update, computed over the top ten
levels of each side at the venue's wire scales. By default the adapter validates it against its
shadow book, rendering prices at `pair_decimals` and quantities at `lot_decimals` from `AssetPairs`;
for a handful of pairs the price scale is one digit finer than the tick size, so the instrument
carries it when it is finer (a coarser scale would truncate the price digits). On mismatch the
adapter emits a `Clear` delta, drops the shadow book, unsubscribes and resubscribes the symbol at
its depth with a snapshot, and ignores further updates until that snapshot arrives; every `book`
unsubscribe names the depth, since the venue keys the subscription by symbol and depth and takes an
unsubscribe without one as depth 10. The recovery is serialized with the user's own subscription
changes, so a replacement subscription is never canceled by a stale recovery. A stream is identified
by the `book` subscribe that opened it: frames are accepted only from the stream of the symbol's
latest subscribe once the venue has confirmed that request, so frames of a replaced subscription or
a superseded recovery are dropped, and a snapshot changes the book only once it has parsed and
applied. A recovery issued before a snapshot is accepted or before a reconnect sends nothing. If the
snapshot does not arrive within 10 seconds the data client requests it again, doubling the wait each
time up to five requests, then logs an error and leaves the book cleared until the next subscription
change or reconnect; this watchdog is the only retry for a `book` recovery, and a replacement
subscription starts its own wait rather than inheriting its predecessor's. The watchdog runs whether
or not checksum validation is enabled, since it recovers a stream whose snapshot never arrived
rather than a checksum mismatch. A `book` subscribe the venue rejects, when it is the symbol's
latest request, clears any book still held and is logged at error with the venue's reason and when
the next request is due: the rejection counts as one failed request, so the watchdog asks again
after 20 seconds and keeps doubling, and a pair the venue will not serve is given up after four more
rejections, about five minutes, while a transient rejection recovers within 20 seconds; a rejection
of a superseded request is ignored. A reconnect retires the subscribes on record and replays each
`book` subscribe under its original request id, so a replay's answer is matched to no request; a
replay the venue rejects is noticed by the watchdog within its base wait of 10 seconds. A shadow
book dropped off the frame path, by a reconnect, by the confirmation of a new stream, by a rejection
of the latest request, or by the watchdog finding the latest subscribe unconfirmed, is cleared
downstream with a `Clear` delta. Three mismatches on one instrument with no valid update between
them switch validation off for that instrument with an error log and keep its book as received, so a
book the venue hashes differently cannot loop on resubscription; a snapshot that validates does not
reset the count. Kraken Futures `book` messages carry no checksum. To disable validation:

```python
config = KrakenDataClientConfig(
    validate_l2_checksum=False,
)
```

## L3 order book (market-by-order)

Kraken exposes Spot per-order book data via the WebSocket v2 `level3` channel at
`wss://ws-l3.kraken.com/v2`. This gives venue order IDs, per-order quantities,
and true incremental events (`add`, `modify`, `delete`). The adapter hashes each
venue order ID into the `u64` `BookOrder.order_id` field used by NautilusTrader.

### Prerequisites

L3 subscriptions require Spot API credentials because Kraken's `level3` channel
is authenticated. Pass them to `KrakenDataClientConfig`:

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

await client.subscribe_book_deltas(
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

`OrderBookDelta` already carries `order_id: u64` in its Arrow schema, so L3 data
is stored identically to L2 in the `ParquetDataCatalog`. L3 generates significantly
more events per instrument than L2. Recommended settings:

- Lower chunk size (e.g. `chunk_size=50_000`) for faster parallel reads.
- Enable `zstd` compression in catalog config.
- Use per-instrument path partitioning (enabled by default).

## Orders capability

### Order types

| Order type             | Spot | Futures | Notes                                      |
| ---------------------- | ---- | ------- | ------------------------------------------ |
| `MARKET`               | ✓    | ✓       | Immediate execution at market price.       |
| `LIMIT`                | ✓    | ✓       | Execution at specified price or better.    |
| `STOP_MARKET`          | ✓    | ✓       | Conditional market order (stop-loss).      |
| `MARKET_IF_TOUCHED`    | ✓    | ✓       | Conditional market order (take-profit).    |
| `STOP_LIMIT`           | ✓    | ✓       | Conditional limit order (stop-loss-limit). |
| `LIMIT_IF_TOUCHED`     | ✓    | ✓       | Maps to `take_profit` with `limit_price`.  |
| `TRAILING_STOP_MARKET` | ✓    | -       | Trailing stop with `trailing_offset`.      |
| `TRAILING_STOP_LIMIT`  | ✓    | -       | Trailing stop-limit with `limit_offset`.   |

### Time in force

| Time in Force | Spot | Futures | Notes                                               |
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

Conditional orders (stop, take-profit, trailing stop) support a trigger price
reference on Spot:

| Trigger Type  | Spot | Futures | Notes                       |
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
| Batch Submit | ✓    | ✓       | Spot chunks at 15 orders. Futures chunks at 10.        |
| Batch Modify | -    | ✓       | Futures HTTP method only. Execution sends one command. |
| Batch Cancel | ✓    | ✓       | Auto-chunks into batches of 50.                        |

:::note
**Cancel all orders**:

- Spot selects the matching open and in-flight orders for the requested instrument
  and cancels them by explicit order ID, with or without a side filter, so a
  request never reaches another instrument. In-flight orders are included because
  the venue can have accepted an order the cache still records as submitted.
- Futures uses the venue's symbol-scoped bulk cancellation when no side filter is
  given, and selects matching cached open and in-flight orders by explicit order ID
  when one is.
- Selected IDs go through the batch-cancel endpoint and are auto-chunked into
  batches of 50. Kraken keys the two identifier kinds separately, so venue order
  IDs are sent as `orders` and client order IDs as `cl_ord_ids`; the batch limit
  counts both together. Each cancel keeps the owning strategy of the order it targets,
  and aggregate or ambiguous responses are left to reconciliation rather than
  producing per-order outcomes.

:::

### Position management

| Feature          | Spot | Futures | Notes                                               |
| ---------------- | ---- | ------- | --------------------------------------------------- |
| Query positions  | ✓    | ✓       | Spot margin via `OpenPositions`; spot cash opt-in.  |
| Position mode    | -    | -       | Single position per instrument.                     |
| Leverage control | ✓    | -       | Spot tiers; per-order `params={"leverage": N}`.     |
| Margin mode      | ✓    | ✓       | Spot/Futures cross margin; no isolated spot margin. |

### Order querying

| Feature              | Spot | Futures | Notes                                        |
| -------------------- | ---- | ------- | -------------------------------------------- |
| Query open orders    | ✓    | ✓       | List all active orders.                      |
| Query order history  | ✓    | ✓       | Historical order data with pagination.       |
| Order status updates | ✓    | ✓       | Real-time order state changes via WebSocket. |
| Trade history        | ✓    | ✓       | Execution and fill reports.                  |

### Contingent orders

| Feature            | Spot | Futures | Notes                                       |
| ------------------ | ---- | ------- | ------------------------------------------- |
| Linked order lists | -    | -       | Submitted lists contain independent orders. |
| OCO orders         | -    | -       | *Not supported*.                            |
| Bracket orders     | -    | -       | *Not supported*.                            |
| Conditional orders | ✓    | ✓       | Stop and take-profit orders.                |

### Maker Protection (Futures)

Kraken Futures applies
[Maker Protection](https://docs.kraken.com/exchange/guides/futures/maker-protection)
on selected markets: placements and edits that could take liquidity are held
for the market's configured window before reaching the matching engine. The
classification is by order type, so any order not marked `post_only` is held
even when it would in fact have rested. Post-only placements and all
cancellations are never held, and no held-order state is exposed on any API.
The venue applies the hold per market to every client; the adapter decodes
the per-market window (`makerProtectionMillis`) on the raw venue instrument
model and exposes no configuration for it.

Order-state handling accounts for the held-order semantics:

- A cancel acknowledged while an order is held is not terminal. The order is
  released as IOC and can still fill. Fills and terminal states are driven
  by venue order updates, never by the cancel acknowledgement itself.
- An order that cannot trade after such a release is reported with the venue
  status `iocWouldNotExecute` on REST (`IOC_WOULD_ENTER_BOOK` on market data),
  which the adapter treats as a terminal rejection. On the order-update feed
  the same outcome arrives as a terminal cancellation whose venue reason the
  adapter preserves.
- A released order cancels a resting order of the same account it would
  match, overriding the configured self-trade strategy. The resting order is
  reported canceled with reason `CANCELLED_BY_SELF_TRADE`.

The adapter closes an order only once the venue's fills for it are accounted.

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
`POST /orders/status` before treating the order as missing. That window reports
orders that are open or were filled or canceled in the last 5 seconds.

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

The Kraken adapter provides reconciliation capabilities for both
Spot and Futures markets, allowing traders to synchronize their local state with
the exchange state at startup or during operation.

### Bounded reports

When reconciliation supplies a lookback, both execution clients derive a single cutoff and apply it
to every historical query, then record it on the mass status through `set_report_window`. Using one
cutoff avoids a report set that never existed at the venue, which a moving cutoff can produce.

Declaring the cutoff is what lets the engine apply its bounded-history rules. The completeness flag
described below qualifies that set rather than gating it: the engine logs a warning when a bounded
set arrives incomplete and reconciles what it received.

An in-scope open order or position whose instrument cannot be resolved fails the read on both
clients, as the adapter guide's scope table requires: dropped, it would read to reconciliation as
an order or position the venue never had. A read scoped to an instrument the client does not hold
returns no rows rather than failing, since the spot and futures clients share the venue and the
engine may ask either one about an order it has not routed. A position that cannot be parsed
fails the read on both clients for the same reason. The completeness flag covers the other gaps:
any order or fill record that cannot be parsed, open or historical, marks the set incomplete on
both clients, and so does a historical order or fill record whose instrument could not be
resolved. Position records do not contribute to the flag. The futures single-order status lookup
and `query_order` read only the queried instrument's orders, so an unresolvable order on another
contract is out of scope for them rather than failing them.

Spot closed-order and fill reads page through an offset until the venue returns an empty page, and
stop after 500 pages. A read cut short by that cap logs a warning, and how it surfaces depends on
the caller. Startup mass status carries the completeness of the order and fill reads in its report
window, so the engine sees the set as incomplete. `generate_order_status_reports` and
`generate_fill_reports` return the records read up to the cap and do not expose a completeness
flag.

### Spot reconciliation

**Order status reports:**

- Open orders: Fetches all currently active orders.
- Closed orders: Fetches historical orders with pagination support.
- Time-bounded queries: Supports filtering by start/end timestamps.
- Startup mass status reads closed orders alongside open ones, so an order that reached a terminal
  state while the node was down is reconciled. The reconciliation lookback bounds the read, and a
  closed-order read cut short by the page cap leaves the mass status incomplete.

**Fill reports:**

- Trade history: Fetches execution history with pagination.
- Time-bounded queries: Supports filtering by start/end timestamps.
- All fill types: Market, limit, and conditional order fills.

**Pair spelling:**

- Kraken spells a pair two ways: the `AssetPairs` key (`XXBTZEUR`), used as the instrument
  `raw_symbol`, and the altname (`XBTEUR`). `OpenPositions` returns the key, while `OpenOrders`
  and `TradesHistory` return the altname.
- The adapter resolves both spellings, so an order or fill on a legacy-named pair is reported.
- A read scoped to one instrument resolves each row and compares instrument IDs, rather than
  comparing a cached `raw_symbol` against the venue's spelling, so a scoped read returns that
  pair's own records whichever way Kraken spells it.
- A read scoped to an instrument the client does not hold returns nothing. Spot and futures IDs
  share the `KRAKEN` venue, so a futures ID can reach the spot client and the reverse; neither
  falls back to returning every instrument's records.
- A spot instrument whose altname differs from its `AssetPairs` key carries the altname in its
  `info` map, so a client whose instruments arrive through `cache_instrument` or
  `cache_instruments` resolves altname-spelled records without refetching `AssetPairs`.
- On an unscoped read, an open order whose pair cannot be resolved to a cached instrument fails the
  read, rather than being omitted from an otherwise successful one. A scoped read skips a record it
  cannot resolve, since it cannot belong to the requested instrument.
- A closed order or fill that cannot be resolved is logged as a warning and skipped, preserving the
  records that do resolve. Historical records routinely outlive the loaded instrument set.

**Account balances:**

- Wallet balances: Fetched from `POST /0/private/BalanceEx`, which reports both the
  total and the held (`hold_trade`) amount per asset. The held amount populates
  `AccountBalance.locked`, so `free` excludes funds Kraken has reserved against
  resting orders. For accounts with a credit line, net credit (`credit - credit_used`)
  is included in `AccountBalance.total`, so `free` matches Kraken's available balance
  of `balance + credit - credit_used - hold_trade`.
- Zero balances: An asset Kraken lists at zero is reported at zero rather than omitted,
  on both spot and futures. The engine only ever inserts balances, so a currency left out
  of a snapshot keeps its previous value. On futures the zero joins the per-currency sum,
  so a funded wallet alongside an empty one of the same asset reports the funded amount.
  An asset Kraken drops from the response entirely still keeps its last reported value.

**Margin position reports** (when `spot_account_type=Margin`):

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
- Margin balances: `POST /0/private/TradeBalance` is called alongside the
  account-state refresh; used margin populates `MarginBalance.initial`, while
  equity and free margin populate the summary balance (see Spot margin trading).

:::warning
A leveraged position closed while the node was down is not recovered from its closing fill when
`reconciliation_lookback_mins` is set. A fully closed lot is absent from `OpenPositions`, so the
instrument carries no position report, and the engine projects that order's fill as order-only:
the order reaches `FILLED`, while the cached position keeps both its quantity and its realized
PnL, so the closing PnL is never recorded. This is the shared engine's documented behavior for an
instrument with no in-scope position report, not a Kraken rule. See
[Order-only fill projection](../concepts/execution/reconciliation.md#order-only-fill-projection).
Removing the synthetic FLAT is what exposes Kraken spot margin to it, because the sweep previously
supplied an explicit FLAT.

A periodic position check does not recover it either, since margin mode declares no bulk position
coverage, and that skip is logged at debug level. The condition also persists across restarts: the
closing order is then cached as `FILLED` and matches the venue exactly, so reconciliation treats it
as already in sync.

Leaving `reconciliation_lookback_mins` unset avoids the projection, and a closing order the cache
already holds, such as a strategy exit submitted before the outage, then recovers into its
position: the fill applies to the cached order and closes the position it belongs to. It is not a
general remedy, because a closing order absent from the cache is attributed to the `EXTERNAL`
strategy and keys a netting position by instrument and strategy. Unless the cached position is
itself `EXTERNAL`-owned or the instrument is claimed through `external_order_claim`, that recovered
fill opens a second, opposite position rather than closing the cached one: net exposure reaches
zero, but the stale position and its realized PnL remain.

Until this is addressed, reconcile a margin position closed during downtime manually, or run
`spot_account_type=Cash` with `use_spot_position_reports=True`, where the wallet read enumerates
every holding it covers and an absent report is genuine evidence of flat.
:::

### Futures reconciliation

**Order status reports:**

- Open orders: Fetches all currently active futures orders.
- Historical orders: Fetches closed and filled orders when `open_only=False`.
- Order events: Full order lifecycle history via `/api/history/v3/orders` endpoint. A read takes one
  page: every Kraken `/history` endpoint draws on one pool of 100 tokens, replenished at 100 every
  10 minutes, at a token per page, so a page that hands back a continuation token, in the body or
  the `Next-Continuation-Token` header, logs a warning and leaves the set incomplete rather than
  reading further. The history lists every lifecycle event, so the read hands back one report per
  order: the open-order snapshot when the venue still lists the order, else its latest history
  state; a closed order is not reopened by an open state stamped later, such as a refused edit
  logged after a cancel. The contract name is resolved as the venue spells it, exactly first and
  then case-insensitively. The history carries no trigger price, so a stop order is reported as the
  limit or market order it executes as once triggered, and venue-initiated orders (liquidation,
  assignment, hedge assignment, unwind, block, RFQ) are reported as market orders. A row the adapter
  cannot represent is skipped with a warning and leaves the set incomplete: an event kind the
  adapter does not know, an order whose type the venue reports as `Unknown` or omits, an unknown
  direction, and a timestamp before the epoch. `OrderNotFound` carries no order state and is skipped
  without affecting completeness; a venue error reported with a success status fails the read. A
  terminal history row with an executed quantity carries no average price, so it is priced from the
  order's fills on the fills page when they cover its filled quantity exactly, else together with
  the fills a cached order has recorded, again exactly, and a failed fills read counts as an empty
  page; when nothing covers it, the single-order query fails and the bulk read leaves the order out,
  so reconciliation defers it rather than infer the executions at the limit price.
- Startup mass status reads one page of the order history alongside open orders, so an order that
  reached a terminal state while the node was down is reconciled when that page holds it. When the
  history read returns a venue error body under a success status, whatever its code, or HTTP 429,
  the mass status logs a warning, falls back to the open orders alone and marks the set
  incomplete. Any other failure, such as another HTTP error status, a transport failure or a body
  that cannot be parsed, fails the mass status, as a failed open-order read does.
- Startup pricing safeguard: the mass status prices each terminal history order with an executed
  quantity from its fills, with the same exact coverage. An order its fills do not cover exactly is
  withheld with a warning naming it, and the set is marked incomplete; the adapter reads only the
  latest fills page, so the missing execution does not come back on a later read. A withheld
  order's page fills stay when the cache holds the order, since the engine reconciles them against
  it without a report; an uncached order's fills are withheld with it.
- Flat instruments: the position read returns open positions only, so an instrument with no
  position report is flat at the venue. The fills read is a single page, so a round trip whose
  opening fill is older than that page would leave its closing side alone, and with no
  `reconciliation_lookback_mins` the engine applies every kept fill to positions, opening a
  position the venue does not hold. To compensate, an unbounded startup read nets the fills of
  every order on the read pages that the cache does not hold on a flat instrument, open orders
  included. When they do not net to zero and a terminal history order is among them, the terminal
  orders are withheld with their fills, with a warning, and the set is marked incomplete. Open orders stay, so an open order's own fill can
  still open a position. A held instrument is left to its position report, and a bounded lookback
  leaves terminal orders to the engine, which projects them onto order state only.

**Fill reports:**

- Fill history: Reads the latest fills page, the last 100 fills across all futures contracts;
  older executions are not returned.
- Time filtering: Client-side filtering by start/end timestamps (parses
  RFC3339 timestamps).
- All fill types: Maker and taker fills with fee information.

**Position status reports:**

- Open positions: Fetches all active futures positions.
- Real-time data: Includes unrealized funding, average price, and position size.

**Account state:**

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

In cash mode, the Kraken adapter can optionally report wallet balances as
position status reports for spot instruments. This feature is disabled by
default and must be explicitly enabled via configuration. Margin-mode accounts
should leave it disabled and rely on `OpenPositions` instead (see Spot margin
trading).

**How it works:**

- When enabled, wallet balances are converted to `PositionStatusReport` objects.
- Positive balances are reported as `LONG` positions.
- Only instruments matching the configured quote currency are reported (default: `USDT`).
  The same filter decides which instruments the client declares bulk position coverage for,
  so an instrument quoted in anything else is never reconciled to flat from a missing report.
- This prevents duplicate reports when the same asset is available with multiple
  quote currencies (e.g., BTC/USD, BTC/USDT, BTC/EUR).

**Configuration:**

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
**Use with caution**: Enabling spot position reports may lead to unintended
behavior if your strategy is not designed to handle spot positions. For example,
a strategy that expects to close positions may attempt to sell your wallet
holdings.
:::

## Spot margin trading

Kraken Spot supports leveraged trading on selected pairs. Per-pair availability
and the valid leverage tiers are advertised by Kraken on the instruments
endpoint as `AssetPairInfo.leverage_buy` and `leverage_sell`; the adapter
caches these at instrument-load time and validates the requested tier before
order submission. Margin trading is enabled per-execution-client via
`spot_account_type`, with per-order `leverage` params.

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
order = strategy.order_factory.limit(
    instrument_id=BTC_USD,
    order_side=OrderSide.BUY,
    quantity=Quantity.from_str("0.01"),
    price=Price.from_str("50000.00"),
    params={"leverage": 5},
)
```

The adapter validates the requested tier against
`AssetPairInfo.leverage_buy` / `leverage_sell` for the pair before submitting;
an invalid tier produces an `OrderDenied` event and never hits the venue.

### Reduce-only

Margin orders can carry `reduce_only=True` so they reduce an existing position
without opening a larger opposite position. Set `spot_account_type=Margin` and supply
either `default_leverage` or per-order `params={"leverage": N}`. The adapter denies
cash orders with `reduce_only` before sending them to Kraken.

### Account state

When `spot_account_type=Margin`, the execution client calls Kraken's
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
the same filter). Under `spot_account_type=Margin` the source is `OpenPositions`, which omits
unleveraged lots, and cash mode without wallet-derived reports returns nothing at all. Wherever
coverage is not declared, an absent report leaves the cached position untouched instead of
closing it.

## Funding rates

The adapter receives funding rate data from the
[Futures ticker](https://docs.kraken.com/exchange/api-reference/futures-websocket/ticker)
WebSocket feed, which provides `relative_funding_rate` and
`next_funding_rate_time` for perpetual futures.

The `interval` field on `FundingRateUpdate` is `None` for Kraken because the
ticker feed does not include a funding interval field and the Kraken API
documentation does not specify a fixed funding period.

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

The current Spot REST call-counter limits are:

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

The product type for each client is specified via the `product_type` option.

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
| `default_leverage`              | `None`    | Default spot margin leverage sent as `"N:1"` when set.                |
| `use_spot_position_reports`     | `False`   | Report wallet balances as positions; cash mode only.                  |
| `spot_positions_quote_currency` | `"USDT"`  | Quote filter for spot wallet position reports and their coverage.     |
| `margin_balance_asset`          | `None`    | Summary asset for `TradeBalance`; `None` defaults to `ZUSD`.          |
| `use_ws_trade`                  | `True`    | Use Spot WebSocket v2 for order operations when active.               |
| `ws_request_timeout_secs`       | `5`       | Spot WebSocket order response timeout.                                |
| `transport_backend`             | `Sockudo` | WebSocket transport backend.                                          |

For spot margin, `default_leverage` applies when an order has no per-order leverage
param. `margin_balance_asset` only changes the `TradeBalance` summary denomination;
per-position figures remain in the pair's quote currency.

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
`KrakenDataClientConfig`. Public market data does not
require credentials.

The lower-level Python HTTP and WebSocket clients load the following variables
when their credential arguments are omitted. Rust applications can use
`KrakenCredential::from_env_spot()` or
`KrakenCredential::from_env_futures(demo)` to load them before constructing
live-node configs.

| Environment Variable             | Description                              |
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
For additional features or to contribute to the Kraken adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
