# Deribit

Founded in 2016, Deribit is a cryptocurrency derivatives exchange for options, futures,
perpetuals, spot, and combo instruments. It is one of the largest crypto options exchanges
by volume, and a leading platform for crypto derivatives trading.

This integration supports live market data ingest and order execution with Deribit.

## Overview

The Deribit adapter is implemented in Rust and exposed to Python through configurations,
factories, clients, enums, and constants. Deribit uses JSON-RPC 2.0 over both HTTP and WebSocket
transports. The adapter prefers WebSocket for subscriptions, real-time data, and order
operations, and uses HTTP for instrument loading, historical requests, and reconciliation.

Components:

- `DeribitRawHttpClient`: Low-level HTTP client owning JSON-RPC framing, signing, rate limits, and retries.
- `DeribitHttpClient`: Domain HTTP client parsing venue responses into Nautilus types; reach the raw client with `inner()`.
- `DeribitWebSocketClient`: Low-level WebSocket connectivity (JSON-RPC over WebSocket).
- `DeribitDataClient`: Market data feed manager.
- `DeribitDataClientFactory`: Data client factory.
- `DeribitExecutionClient`: Account management and trade execution gateway.
- `DeribitExecutionClientFactory`: Execution client factory.

Python surface available from `nautilus_trader.adapters.deribit`:

- `DeribitDataClientConfig`, `DeribitExecClientConfig`
- `DeribitDataClientFactory`, `DeribitExecutionClientFactory`
- `DeribitHttpClient`, `DeribitWebSocketClient`
- `DeribitCurrency`, `DeribitEnvironment`, `DeribitProductType`, `DeribitUpdateInterval`
- `DeribitBookSummary`, `DeribitVolatilityIndex`
- `get_deribit_http_base_url`, `get_deribit_ws_url`
- `DERIBIT`, `DERIBIT_CLIENT_ID`, and `DERIBIT_VENUE`

:::note
Most users will define a configuration for a live trading node (as shown below),
and won't need to work directly with these lower-level components.
:::

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/deribit/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/deribit/examples/)

## Deribit documentation

- [API reference](https://docs.deribit.com/)
- [Rate limits](https://docs.deribit.com/articles/rate-limits)
- [Connection management](https://docs.deribit.com/articles/connection-management-best-practices)
- [Market data collection](https://docs.deribit.com/articles/market-data-collection-best-practices)
- [Order management](https://docs.deribit.com/articles/order-management-best-practices)
- [Access scope](https://docs.deribit.com/articles/access-scope)

It's recommended you also refer to the Deribit documentation in conjunction with this
NautilusTrader integration guide.

## Product support

Each client loads only the product families listed in its `product_types` configuration, once on
connect. Neither client refreshes the instrument set afterwards, so restart the node to pick up
newly listed instruments. On the data client, `auto_load_missing_instruments` instead fetches an
uncached instrument on first subscribe; see [Lazy-load on subscribe](#lazy-load-on-subscribe).

| Product type      | Data feed | Trading | Notes                                          |
| ----------------- | --------- | ------- | ---------------------------------------------- |
| Perpetual futures | ✓         | ✓       | Loaded with `DeribitProductType.FUTURE`.       |
| Dated futures     | ✓         | ✓       | Loaded with `DeribitProductType.FUTURE`.       |
| Options           | ✓         | ✓       | Loaded with `DeribitProductType.OPTION`.       |
| Spot              | ✓         | ✓       | Loaded with `DeribitProductType.SPOT`.         |
| Future combos     | ✓         | ✓       | Loaded with `DeribitProductType.FUTURE_COMBO`. |
| Option combos     | ✓         | ✓       | Loaded with `DeribitProductType.OPTION_COMBO`. |

## Symbology

Deribit uses specific symbol conventions for different instrument types.
All instrument IDs should include the `.DERIBIT` suffix when referencing them
(e.g., `BTC-PERPETUAL.DERIBIT` for BTC perpetual).

### Quantity units

Nautilus quantities map to Deribit's `amount` field, not the optional `contracts` field. Deribit
reports perpetual and inverse futures amounts in USD units, and reports options and linear futures
amounts in the underlying base currency. The Deribit `contract_size` field converts between
`amount` and contract count; the adapter does not apply it again as the Nautilus multiplier.

### Perpetual futures

Format: `{Currency}-PERPETUAL`

Examples:

- `BTC-PERPETUAL` - Bitcoin perpetual swap.
- `ETH-PERPETUAL` - Ethereum perpetual swap.

To subscribe to BTC perpetual in your strategy:

```python
InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
```

### Dated futures

Format: `{Currency}-{DDMMMYY}`

Examples:

- `BTC-25DEC26` - Bitcoin future expiring December 25, 2026.
- `ETH-26MAR27` - Ethereum future expiring March 26, 2027.

```python
InstrumentId.from_str("BTC-25DEC26.DERIBIT")
```

### Options

Format: `{Currency}-{DDMMMYY}-{Strike}-{Type}`

Examples:

- `BTC-25DEC26-100000-C` - Bitcoin call option, $100,000 strike, expiring December 25, 2026.
- `BTC-25DEC26-80000-P` - Bitcoin put option, $80,000 strike, expiring December 25, 2026.
- `ETH-26MAR27-4000-C` - Ethereum call option, $4,000 strike, expiring March 26, 2027.

Where:

- `C` = Call option.
- `P` = Put option.

```python
InstrumentId.from_str("BTC-25DEC26-100000-C.DERIBIT")
```

### Spot

Format: `{BaseCurrency}_{QuoteCurrency}`

Examples:

- `BTC_USDC` - Bitcoin against USDC.
- `ETH_USDC` - Ethereum against USDC.

```python
InstrumentId.from_str("BTC_USDC.DERIBIT")
```

### Future combos

Format: `{Currency}-FS-{LegA}_{LegB}`

Legs are dated futures or the perpetual (denoted `PERP` inside combo names, even though the
standalone instrument is `BTC-PERPETUAL`). The combo expires with its earliest leg.

Examples:

- `BTC-FS-25DEC26_PERP` - calendar spread between the December 2026 future and the perpetual.
- `BTC-FS-26MAR27_25DEC26` - inter-month spread between two dated futures.

```python
InstrumentId.from_str("BTC-FS-25DEC26_PERP.DERIBIT")
```

The adapter models future combos as `CryptoFuturesSpread`, priced in USD as the spread
between legs, with crypto settlement currency and `is_inverse` set per the upstream
`instrument_type`.

### Option combos

Format: `{Currency}-{Strategy}-{DDMMMYY}-{Strikes}`

Strategy codes include CS (call spread), PS (put spread), STRG (strangle), STRD (straddle),
BOX (box), and RR (risk reversal). The strikes segment separates multiple strikes with `_`.

Examples:

- `BTC-CS-25DEC26-70000_75000` - 70k / 75k call spread expiring December 25, 2026.
- `BTC-STRG-26MAR27-72000_80000` - 72k / 80k strangle expiring March 26, 2027.
- `BTC-STRD-26MAR27-77000` - 77k straddle expiring March 26, 2027.
- `BTC-BOX-26MAR27-58000_60000` - 58k / 60k box expiring March 26, 2027.

```python
InstrumentId.from_str("BTC-STRG-26MAR27-72000_80000.DERIBIT")
```

The adapter models option combos as `CryptoOptionSpread`, priced in the base currency under
Deribit's inverse-option convention; fractional `size_increment` (e.g. `0.1`) is preserved
end-to-end.

## Traded expirations

Deribit exposes active traded expirations through the `public/get_expirations` HTTP endpoint.
Option-chain loaders can use the high-level HTTP client to refresh active option series without
scanning every instrument.

```rust tab="Rust"
use nautilus_deribit::http::models::DeribitCurrency;

let expirations = client
    .request_option_expirations(DeribitCurrency::BTC)
    .await?;
```

```python tab="Python"
from nautilus_trader.adapters.deribit import DeribitCurrency
from nautilus_trader.adapters.deribit import DeribitHttpClient

client = DeribitHttpClient()
expirations = await client.request_option_expirations(DeribitCurrency.BTC)
```

The high-level method returns option expirations only. For lower-level Rust requests, call
`client.inner().get_expirations(...)` with `GetExpirationsParams`. Deribit returns a
currency-keyed result for concrete currencies such as `BTC`, and a direct kind-keyed result for
`currency=any`; the adapter handles both shapes.

## Combo instruments

Instrument loading includes combos when `product_types` contains the future-combo or
option-combo variant. In Python, use `DeribitProductType.FUTURE_COMBO` or
`DeribitProductType.OPTION_COMBO`. Deribit exposes the leg makeup of every active combo on
`/public/get_combos`, and the combo's trading metadata (tick size, contract size, expiration,
min trade amount) on the standard `/public/get_instruments?kind=option_combo|future_combo`
response. The adapter merges both, attaching `deribit_combo_id`, `deribit_combo_state`, and
`deribit_combo_legs` to the loaded instrument's `info` map so strategies can resolve legs
without a second venue call.

### Trade publishing

Deribit publishes each combo trade twice:

- On the combo's trade channel (`trades.{combo_name}.{interval}`): the parent trade plus a
  `legs[]` array describing each leg fill.
- On each leg's trade channel (`trades.{leg_instrument}.{interval}`): a standalone trade for the
  leg, tagged with `combo_id` and `combo_trade_id` pointing back to the parent.

A subscriber to a plain option or future therefore sees combo-origin fills on its existing
trade stream, and a subscriber to the combo itself sees the combo-level trade. The adapter
does not fan out combo parent messages into extra leg ticks; it forwards the upstream parent
and per-leg messages as separate `TradeTick`s against their respective `InstrumentId`s, so a
subscriber to both the combo and an underlying leg sees one combo tick plus one leg tick for
that combo trade, not duplicate ticks against the same instrument.

To have the Deribit data client open the real leg trade channels alongside a combo trade
subscription, pass `params={"subscribe_combo_legs": True}` to `subscribe_trades`. When
unsubscribing that combo trade stream, Nautilus also closes the leg subscriptions opened by
this opt-in.

Deribit already publishes block trades and Block RFQs per leg, so the adapter forwards
them through the standard 1:1 trade path. See [Trade ID provenance](#trade-id-provenance)
for how block- and RFQ-origin trades are tagged on the resulting `TradeTick`.

### Historical combo trades

The standard per-instrument trades endpoint accepts combo instrument names. To sweep all combos
of a given product kind in one call, use `get_last_trades_by_currency` via
`DeribitHttpClient::inner()`:

```rust
use nautilus_deribit::http::{
    models::{DeribitCurrency, DeribitProductType},
    query::GetLastTradesByCurrencyParams,
};

let params = GetLastTradesByCurrencyParams::builder()
    .currency(DeribitCurrency::BTC)
    .kind(DeribitProductType::FutureCombo)
    .count(50_u32)
    .include_old(true)
    .build()?;
let resp = client.inner().get_last_trades_by_currency(params).await?;
```

Each returned `DeribitPublicTrade` carries `legs: Option<Vec<DeribitTradeLeg>>` plus the
`combo_id` and `combo_trade_id` fields used to correlate per-leg trades.

## Trade ID provenance

Public `TradeTick`s emitted by the adapter prefix the venue trade ID when the trade
originated from a Block RFQ, a block trade, or a combo. Strategies that need to
distinguish these from plain trades can pattern-match the prefix on `TradeTick.trade_id`.
The raw Deribit `trade_id` is preserved after the prefix, so reconciliation against
Deribit's own IDs is a prefix strip.

| Prefix       | Source field     | Meaning                                                        |
| ------------ | ---------------- | -------------------------------------------------------------- |
| `RFQ-`       | `block_rfq_id`   | Trade originated from a Block RFQ.                             |
| `BLK-`       | `block_trade_id` | Trade is a non-RFQ block trade.                                |
| `COMBO-`     | `combo_id`       | Per-leg trade whose parent originated from a combo instrument. |
| *unprefixed* | (none of above)  | Standard trade.                                                |

Precedence when multiple tags are present: `RFQ-` > `BLK-` > `COMBO-`. Block RFQs are
themselves block trades on Deribit, so the RFQ tag wins; combos executed as block trades
are tagged `BLK-` because the block flow is the more important reconciliation signal.

This applies only to public trades (`TradeTick`). `FillReport.trade_id` is unchanged so
reconciliation against `get_user_trades_*` keeps working.

:::note
This is a one-way convention introduced in NautilusTrader 1.228.0. Replay data captured
before that release lacks prefixes. Strategies that store and compare `trade_id` strings
across releases should strip the prefix on the new-data side, or filter by prefix only on
data they know was captured after upgrading.
:::

## Market data

Live subscriptions are served from WebSocket channels; historical requests go over HTTP. Every
channel that takes an `{interval}` resolves it through the `interval` subscription parameter and
the default selection rules described under
[Order book subscriptions](#order-book-subscriptions).

| Nautilus subscription | Deribit channel                                                           | Notes                                                                      |
| --------------------- | ------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| Instruments           | `instrument.state.{kind}.{currency}`                                      | `kind` and `currency` params both default to `any`.                        |
| Instrument status     | `instrument.state.{kind}.{currency}`                                      | Channel derived from the instrument ID; every instrument on it is emitted. |
| Book deltas           | `book.{instrument}.raw` or `book.{instrument}.{group}.{depth}.{interval}` | `L2_MBP` only; see [Order book subscriptions](#order-book-subscriptions).  |
| Book depth10          | `book.{instrument}.{group}.10.{interval}`                                 | Always the grouped channel, fixed at depth 10.                             |
| Quotes                | `quote.{instrument}`                                                      | Top of book; this channel takes no interval.                               |
| Trades                | `trades.{instrument}.{interval}`                                          | See [Trade publishing](#trade-publishing) for combo behavior.              |
| Bars                  | `chart.trades.{instrument}.{resolution}`                                  | See [Bars](#bars).                                                         |
| Mark prices           | `ticker.{instrument}.{interval}`                                          | Parsed from the ticker payload.                                            |
| Index prices          | `ticker.{instrument}.{interval}`                                          | Parsed from the ticker payload.                                            |
| Option greeks         | `ticker.{instrument}.{interval}`                                          | Options only.                                                              |
| Funding rates         | `perpetual.{instrument}.{interval}`                                       | Perpetuals only; other instruments are rejected.                           |
| Volatility index      | `deribit_volatility_index.{index_name}`                                   | Custom data; see [Volatility index](#volatility-index).                    |

| Nautilus request     | Deribit endpoint                                         | Notes                                                        |
| -------------------- | -------------------------------------------------------- | ------------------------------------------------------------ |
| Instruments          | `public/get_instruments`                                 | Scoped to the configured `product_types`.                    |
| Trades               | `public/get_last_trades_by_instrument_and_time`          | Accepts combo instrument names.                              |
| Bars                 | `public/get_tradingview_chart_data`                      | See [Bars](#bars).                                           |
| Book snapshot        | `public/get_order_book`                                  | Requires the instrument in the client cache.                 |
| Forward prices       | `public/ticker` or `public/get_book_summary_by_currency` | Single instrument uses the ticker; bulk uses book summaries. |
| `DeribitBookSummary` | `public/get_book_summary_by_currency`                    | Custom data; see [Book summaries](#book-summaries).          |

### Bars

Deribit chart resolutions are `1`, `3`, `5`, `10`, `15`, `30`, `60`, `120`, `180`, `360`, and
`720` minutes, plus `1D`. Both the live and historical paths require `EXTERNAL` aggregation.

Historical requests are strict: a bar type that does not map onto one of those resolutions is
rejected, as is any aggregation other than minute, hour, and day.

Live subscriptions snap instead. The requested step is rounded **up** to the next supported
resolution, so a 2-minute step opens Deribit's 3-minute channel and a 6-minute step opens the
10-minute channel; anything longer than 720 minutes becomes `1D`. Aggregations other than minute,
hour, and day fall back to 1 minute with a warning. Each received candle is stamped with the
`BarType` derived from the venue resolution, always with a `LAST` price type. Subscribe with a bar
type naming a supported resolution and `LAST`; otherwise the bars publish under a different
`BarType` than the one you subscribed with and never reach your handler.

`Bar.volume` comes from the chart `cost` field (USD) for inverse perpetuals and inverse futures,
and from the `volume` field (base currency) for everything else, including options and option
spreads flagged inverse. This keeps `Bar.volume` and `TradeTick.size` on one unit per instrument.

## Order book subscriptions

Deribit publishes L2 (market-by-price) book data only, so `subscribe_book_deltas` and
`subscribe_book_depth10` reject any book type other than `BookType.L2_MBP`. Two feed families
are available, each suited to different use cases.

### Raw feeds (tick-by-tick)

Raw channels deliver every single update as an individual message. Subscribing to a raw order book
gives you a notification for every order insertion, update, or deletion in the book.

- Requires authenticated connection (safeguard against abuse).
- Use when you need every price level change for HFT or market making.
- Higher message volume.
- Delivers one initial snapshot of the full book, then incremental deltas.

### Aggregated feeds (batched)

Aggregated channels deliver updates in batches at a fixed interval (e.g., every 100ms).
This groups multiple order book changes into single messages.

- Available without authentication.
- Recommended for most use cases.
- Lower message volume, easier to process.
- Default unauthenticated interval: 100ms.
- Every message is a complete depth-limited snapshot, so each update replaces the book
  rather than patching it.

### Subscription parameters

The Nautilus adapter supports both feed types via subscription parameters:

| Parameter  | Values                 | Notes                                                                     |
| ---------- | ---------------------- | ------------------------------------------------------------------------- |
| `interval` | `raw`, `100ms`, `agg2` | `agg2` batches at about 1 second intervals. `raw` requires auth.          |
| `group`    | `none`, price group    | Default: `none`. Applies only to grouped non-raw book channels.           |
| `depth`    | `1`, `10`, `20`        | Default: `10`. Number of price levels per side for grouped book channels. |

The data client chooses the order book interval as follows:

1. Uses `params["interval"]` when supplied.
2. Uses `raw` when the WebSocket connection is authenticated and no interval is supplied.
3. Uses Deribit's public `100ms` grouped feed when the connection is not authenticated.

```python
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId

instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")

# Public 100ms aggregated feed when no API credentials are configured.
strategy.subscribe_book_deltas(instrument_id, BookType.L2_MBP)

# Raw feed. This is also the authenticated default when no interval is supplied.
strategy.subscribe_book_deltas(
    instrument_id,
    BookType.L2_MBP,
    params={"interval": "raw"},
)

# Force an aggregated feed on an authenticated connection.
strategy.subscribe_book_deltas(
    instrument_id,
    BookType.L2_MBP,
    params={"interval": "100ms", "depth": 10},
)
```

:::note
Raw order book feeds require an authenticated WebSocket connection. Ensure API credentials are
configured before subscribing to raw feeds.
:::

:::tip
For most strategies, the 100ms aggregated feed provides sufficient granularity with lower message
overhead. Set `params={"interval": "100ms"}` when you provide credentials but do not need raw
tick-by-tick book updates.
:::

### Sequence gap recovery

The adapter records the `change_id` of every book message and checks the `prev_change_id` of
each incremental update against it. Only raw-channel updates carry `prev_change_id`; aggregated
messages are self-contained snapshots and need no sequence check.

When a gap is detected (a missed message), the adapter automatically:

1. Drops the delta that exposed the gap, and every further delta for the affected instrument.
2. Unsubscribes from that instrument's book channels.
3. Resubscribes once the unsubscribe is acknowledged, to obtain a fresh snapshot.
4. Resumes normal processing once the snapshot arrives, reseeding the sequence from it.

During resync, the strategy will not receive stale or incomplete book updates. A user-initiated
unsubscribe while a resync is pending cancels the resubscribe instead of reopening the channel.

## Funding rates

Deribit exchanges funding continuously (every few seconds) rather than at fixed intervals
like most other exchanges. The `interval` field on `FundingRateUpdate` is `None` for
Deribit because this continuous model does not map to a discrete period.

## Deribit specific data

### Book summaries

Request `DeribitBookSummary` custom data to fetch one bulk snapshot filtered by currency and product
kind. Each response item includes the Nautilus instrument ID, implied volatility, open interest,
prices, volume, and other fields returned by `public/get_book_summary_by_currency`.

The actor or strategy receives the complete response through one `on_historical_data` callback.
Each item is a `CustomData` wrapper containing a `DeribitBookSummary` in its `data` field:

```python
from nautilus_trader.adapters.deribit import DERIBIT_CLIENT_ID
from nautilus_trader.adapters.deribit import DeribitBookSummary
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType


def on_start(self) -> None:
    self.request_data(
        DataType(
            DeribitBookSummary.__name__,
            metadata={"currency": "BTC", "kind": "option"},
        ),
        DERIBIT_CLIENT_ID,
    )


def on_historical_data(self, data: list[CustomData]) -> None:
    for item in data:
        summary = item.data
        if isinstance(summary, DeribitBookSummary):
            self.log.info(
                f"{summary.instrument_id}: mark_iv={summary.mark_iv}, "
                f"open_interest={summary.open_interest}",
            )
```

The `currency` metadata field is required. The optional `kind` field defaults to `option`.
Decimal-backed venue fields, such as `mark_iv` and `open_interest`, are exposed to Python as strings
or `None`. An empty response invokes `on_historical_data` once with an empty list. A failed venue
request also invokes the callback with an empty list after logging an error. A request rejected
before it reaches the venue, such as one missing `currency`, produces no callback.

### Volatility index

The adapter emits `DeribitVolatilityIndex` custom data from Deribit's
`deribit_volatility_index.{index_name}` WebSocket channel. Deribit provides
volatility index streams such as `btc_usd` and `eth_usd`.

| Field        | Type    | Description                                              |
| ------------ | ------- | -------------------------------------------------------- |
| `index_name` | `str`   | Deribit volatility index name, for example `btc_usd`.    |
| `volatility` | `float` | Current volatility index value.                          |
| `ts_event`   | `int`   | UNIX timestamp in nanoseconds when the update occurred.  |
| `ts_init`    | `int`   | UNIX timestamp in nanoseconds when the object was built. |

Subscribe from an actor or strategy with `DataType(DeribitVolatilityIndex.__name__)`.
The `index_name` metadata key is required:

```python
from nautilus_trader.adapters.deribit import DeribitVolatilityIndex
from nautilus_trader.model import ClientId
from nautilus_trader.model import DataType

self.subscribe_data(
    data_type=DataType(DeribitVolatilityIndex.__name__, metadata={"index_name": "btc_usd"}),
    client_id=ClientId.from_str("DERIBIT"),
)
```

## Orders capability

Below are the order types, execution instructions, and time-in-force options supported on Deribit.

### Order types

| Nautilus order type    | Deribit order type | Supported | Notes                                   |
| ---------------------- | ------------------ | --------- | --------------------------------------- |
| `MARKET`               | `market`           | ✓         | Immediate execution at market price.    |
| `LIMIT`                | `limit`            | ✓         | Execution at specified price or better. |
| `STOP_MARKET`          | `stop_market`      | ✓         | Conditional market order on trigger.    |
| `STOP_LIMIT`           | `stop_limit`       | ✓         | Conditional limit order on trigger.     |
| `MARKET_IF_TOUCHED`    | `take_market`      | ✓         | Take-profit style market order.         |
| `LIMIT_IF_TOUCHED`     | `take_limit`       | ✓         | Take-profit style limit order.          |
| `TRAILING_STOP_MARKET` | `trailing_stop`    | -         | *Not currently implemented*.            |
| `TRAILING_STOP_LIMIT`  | N/A                | -         | *Not supported by Deribit*.             |
| `MARKET_TO_LIMIT`      | `market_limit`     | -         | *Not currently implemented*.            |

### Execution instructions

| Instruction   | Supported | Notes                                                                            |
| ------------- | --------- | -------------------------------------------------------------------------------- |
| `post_only`   | ✓         | Order will be rejected if it would take liquidity. Uses `reject_post_only=true`. |
| `reduce_only` | ✓         | Order can only reduce an existing position.                                      |

### Time in force

| Time in force | Supported | Notes                                                |
| ------------- | --------- | ---------------------------------------------------- |
| `GTC`         | ✓         | Good till canceled (`good_til_cancelled`).           |
| `GTD`         | ✓         | Good till day. Expires at 8:00 UTC (`good_til_day`). |
| `IOC`         | ✓         | Immediate or cancel (`immediate_or_cancel`).         |
| `FOK`         | ✓         | Fill or kill (`fill_or_kill`).                       |

Deribit applies time in force to limit-style orders. The adapter omits `time_in_force`
for `MARKET`, `STOP_MARKET`, and `MARKET_IF_TOUCHED` orders because Deribit rejects that
parameter on market-style order types.

:::note
**GTD on Deribit**: Unlike other exchanges where GTD accepts an arbitrary expiry time,
Deribit's `good_til_day` always expires at 8:00 UTC the same or next day. Custom expiry times
will be logged as warnings and the order will use the exchange's fixed expiry behavior.
:::

### Trigger types

Conditional orders (stop orders) support different trigger price sources:

| Trigger type  | Supported | Notes                                                |
| ------------- | --------- | ---------------------------------------------------- |
| `last_price`  | ✓         | Uses the last traded price. Also used for `DEFAULT`. |
| `mark_price`  | ✓         | Uses the mark price.                                 |
| `index_price` | ✓         | Uses the underlying index price.                     |

Any other Nautilus trigger type is dropped from the request rather than mapped. Deribit requires
`trigger` on stop-loss and take-profit orders, so use one of the three above for conditional
orders.

```python
# Example: Stop loss using mark price trigger
stop_order = order_factory.stop_market(
    instrument_id=instrument_id,
    order_side=OrderSide.SELL,
    quantity=Quantity.from_str("0.1"),
    trigger_price=Price.from_str("45000.0"),
    trigger_type=TriggerType.MARK_PRICE,  # Use mark price for trigger
)
strategy.submit_order(stop_order)
```

### Batch operations

| Operation                | Supported | Notes                                                                    |
| ------------------------ | --------- | ------------------------------------------------------------------------ |
| Submit order list        | ✓         | Sends each order as an individual Deribit order. No atomic venue batch.  |
| Batch cancel by order ID | ✓         | Sends individual `private/cancel` requests for each venue order ID.      |
| Cancel all by instrument | ✓         | Uses `private/cancel_all_by_instrument` when no side filter is supplied. |
| Side-filtered cancel all | ✓         | Filters cached open orders locally, then cancels each matching order.    |
| Batch modify             | -         | *Not currently implemented*: single order modify is supported.           |

### Post-only behavior

Deribit offers two post-only modes:

1. **Price adjustment (Deribit default)**: If a post-only order would cross the spread and execute,
   Deribit automatically adjusts the price to one tick inside the spread.
2. **Reject mode**: Order is immediately rejected if it would cross the spread.

The Nautilus adapter uses **reject mode** (`reject_post_only=true`) for deterministic behavior.
If a post-only order would take liquidity, it is rejected with error code `11054`, and an `OrderRejected`
event is emitted with the `due_post_only` flag set to `true`.

This allows strategies to differentiate between:

- Orders rejected due to post-only violation (attempted to take liquidity).
- Orders rejected for other reasons (insufficient margin, invalid price, etc.).

### Order modification

The adapter uses Deribit's native `private/edit` endpoint rather than cancel-and-replace.
This provides several advantages:

| Benefit                     | Description                                                       |
| --------------------------- | ----------------------------------------------------------------- |
| Single request              | Faster execution, lower latency than cancel + new order.          |
| Queue priority preservation | Keeps position when only reducing quantity or keeping same price. |
| Fill history maintained     | Partial fills remain linked to the same order ID.                 |

**Queue priority rules:**

- **Decreasing quantity only**: Keeps queue position.
- **Same price**: Keeps queue position.
- **Increasing quantity or changing price**: Loses queue position (treated as new order).

A modify command must carry the venue order ID and a price; either one missing rejects the
command locally without reaching Deribit. Quantity is optional and falls back to the cached
order's current quantity.

### Position management

| Feature          | Supported | Notes                                                              |
| ---------------- | --------- | ------------------------------------------------------------------ |
| Query positions  | ✓         | Fetched from `private/get_positions` for reconciliation.           |
| Position mode    | -         | *Not supported by Deribit*: net position mode only.                |
| Leverage control | -         | *Not supported by Deribit*: no direct leverage setting.            |
| Margin mode      | -         | *Not currently implemented*: Deribit exposes account margin modes. |

The execution client subscribes to `user.orders`, `user.trades`, and `user.portfolio` (balance and
margin), not to Deribit's combined `user.changes` channel, so no position push reaches the adapter.
Live position state is maintained by Nautilus from the fills on `user.trades`, and
`private/get_positions` supplies the venue snapshot that reconciliation compares against.

### Order querying

| Feature              | Supported | Notes                          |
| -------------------- | --------- | ------------------------------ |
| Query open orders    | ✓         | List all active orders.        |
| Query order history  | ✓         | Historical order data.         |
| Order status updates | ✓         | Real-time order state changes. |
| Trade history        | ✓         | Execution and fill reports.    |

### Contingent orders

| Feature                        | Supported | Notes                                                                          |
| ------------------------------ | --------- | ------------------------------------------------------------------------------ |
| Order lists                    | ✓         | Submitted sequentially as independent orders; there is no atomic venue list.   |
| Native linked orders           | -         | *Not currently implemented*: Deribit exposes `linked_order_type`.              |
| OCO orders                     | -         | *Not currently implemented*: Deribit exposes `one_cancels_other`.              |
| Bracket orders                 | -         | *Not currently implemented*: Deribit exposes `one_triggers_one_cancels_other`. |
| Conditional stop orders        | ✓         | Stop market and stop limit orders.                                             |
| Conditional take-profit orders | ✓         | Market-if-touched and limit-if-touched orders.                                 |

### Liquidation handling

Deribit tags any trade that was triggered by a liquidation. On the
`user.trades` stream and `private/get_user_trades_*` endpoints, the optional
`liquidation` field indicates which side was being liquidated:

| Value  | Meaning                       |
| ------ | ----------------------------- |
| `"M"`  | Maker side was liquidated.    |
| `"T"`  | Taker side was liquidated.    |
| `"MT"` | Both sides were liquidated.   |
| absent | Normal non-liquidation trade. |

The adapter logs a warning for each liquidation-tagged fill with the
instrument, trade ID, order ID, and liquidation side, and then emits the
`FillReport` through the normal pipeline. Deribit does not operate an ADL
mechanism distinct from the liquidation + insurance-fund / portfolio margin
process, so there is no separate ADL signal to surface.

Upstream references:

- [`user.trades.{instrument_name}.{interval}` channel](https://docs.deribit.com/subscriptions/user/usertradesinstrument_nameinterval)
- [Liquidation documentation](https://support.deribit.com/hc/en-us/articles/25944769313309-Liquidations)

## Rate limiting

Deribit uses credit-based and endpoint-specific rate limits. The official Deribit limits are
authoritative, and they can vary by endpoint, account tier, and current venue policy. The adapter
adds local token buckets to reduce avoidable throttling, but it does not replace Deribit's own
server-side checks.

### HTTP limits

Every HTTP request consumes the global bucket, plus the order or account bucket when the JSON-RPC
method falls into that category, plus a per-method bucket keyed `deribit:{method}` that uses the
global quota.

| Bucket / key       | Adapter bucket        | Notes                                                          |
| ------------------ | --------------------- | -------------------------------------------------------------- |
| `deribit:global`   | 20 req/sec, 100 burst | Applied to every HTTP request.                                 |
| `deribit:orders`   | 5 req/sec, 20 burst   | Matching-engine methods when driving the HTTP client directly. |
| `deribit:account`  | 5 req/sec, 5 burst    | Account, position, order state, and user trade endpoints.      |
| `deribit:{method}` | 20 req/sec, 100 burst | Per-method bucket; falls back to the global quota.             |

### WebSocket limits

| Operation             | Adapter bucket      | Notes                                                          |
| --------------------- | ------------------- | -------------------------------------------------------------- |
| Subscribe/unsubscribe | 3 req/sec, 10 burst | Also the default bucket for every non-order WebSocket request. |
| Order operations      | 5 req/sec, 20 burst | Buy, sell, edit, and cancel via WebSocket.                     |

:::note
The Nautilus adapter uses WebSocket for order submission (not HTTP) for lower latency.
Order operations are rate-limited by `DERIBIT_WS_ORDER_QUOTA` (5 req/sec, 20 burst).
:::

### Credit-based system details

Deribit replenishes non-matching-engine credits continuously. Current public documentation lists
the default non-matching-engine pool as follows:

**Non-matching engine requests:**

| Parameter        | Value              | Notes                           |
| ---------------- | ------------------ | ------------------------------- |
| Cost per request | 500 credits        | Each API call consumes credits. |
| Maximum pool     | 50,000 credits     | Allows 100 request burst.       |
| Refill rate      | 10,000 credits/sec | ~20 sustained requests/second.  |

**Matching engine requests (default tier):**

| Parameter      | Value          | Notes                            |
| -------------- | -------------- | -------------------------------- |
| Sustained rate | 5 requests/sec | Continuous rate limit.           |
| Burst capacity | 20 requests    | Maximum burst before throttling. |

Higher matching-engine limits are available for market makers and high-volume traders based on
7-day trading volume tiers.

Some Deribit endpoints have stricter method-specific limits. For example, current venue docs list
`public/get_instruments` at 1 request per second with a 50 request burst, and subscription methods
at about 3.3 requests per second with a 10 request burst. Keep `product_types` scoped to the
families you need and avoid repeated full instrument reloads in live systems.

The Nautilus adapter implements broad token bucket rate limiters configured as:

- `DERIBIT_HTTP_REST_QUOTA`: 20 req/sec with 100 burst (global and per-method HTTP)
- `DERIBIT_HTTP_ORDER_QUOTA`: 5 req/sec with 20 burst (matching-engine HTTP)
- `DERIBIT_HTTP_ACCOUNT_QUOTA`: 5 req/sec with 5 burst (account HTTP)
- `DERIBIT_WS_ORDER_QUOTA`: 5 req/sec with 20 burst (matching-engine WebSocket)
- `DERIBIT_WS_SUBSCRIPTION_QUOTA`: 3 req/sec with 10 burst (subscribe, unsubscribe, and other non-order requests)

For more details, see the
[Rate limits article](https://docs.deribit.com/articles/rate-limits).

:::warning
Deribit returns error code `10028` (too_many_requests) when you exceed the allowed quota.
Repeated violations may result in temporary throttling.
:::

## Connection management

### Platform limits

| Limit                                            | Current Deribit guidance |
| ------------------------------------------------ | ------------------------ |
| Active sessions per API key or login             | 16                       |
| Session and connection-scoped connections per IP | 32                       |
| Web app connections per browser session          | 2                        |

### Session-based authentication

The adapter uses **separate WebSocket sessions** for data and execution clients, each with its own
authentication scope:

| Client           | Session Name         | Purpose                                             |
| ---------------- | -------------------- | --------------------------------------------------- |
| Data client      | `nautilus-data`      | Market data subscriptions (raw feeds require auth). |
| Execution client | `nautilus-execution` | Order operations (buy, sell, edit, cancel).         |

**Authentication flow:**

1. WebSocket connects to Deribit.
2. Client authenticates using `client_signature` grant type with session scope.
3. Tokens are refreshed before expiry.
4. On reconnection, re-authentication is retried with exponential backoff (up to 3 attempts).
   If all attempts fail, only public channel subscriptions are restored.

This session-based approach allows:

- Independent token management per client type.
- Isolated failure domains (data auth failure does not affect execution).
- Clear audit trail in Deribit's session logs.

### Best practices

The adapter follows Deribit's
[recommended connection practices](https://docs.deribit.com/articles/connection-management-best-practices):

1. **Uses WebSocket subscriptions** for real-time data instead of REST polling, resulting in fewer requests,
   lower latency, and reduced rate limit consumption.
2. **Authenticates all connections** when credentials are provided. Authenticated users benefit
   from higher rate limits and are less likely to be IP rate-limited.
3. **Implements heartbeats** (30 second interval by default) to maintain connection health and detect
   disconnections early.
4. **Handles reconnection** automatically with re-authentication and subscription recovery.

:::tip
Always provide API credentials even for public data access. Authenticated connections have higher
rate limits, and Deribit contacts authenticated clients before applying restrictions during
high-load periods.
:::

:::note
The adapter uses a 30 second heartbeat interval by default. Deribit requires WebSocket heartbeat
intervals to be at least 10 seconds.
:::

## Authentication

Deribit uses API key authentication with HMAC-SHA256 signatures for private endpoints.

To create API credentials:

1. Log into your Deribit account at [deribit.com](https://www.deribit.com)
   (or [test.deribit.com](https://test.deribit.com) for testnet).
2. Navigate to **Account** -> **API**.
3. Click **Add new key** and configure permissions:
   - Enable **read** for market data access
   - Enable **trade** for order execution
   - Enable **wallet** if you need account balance access
4. Note down your **Client ID** (API key) and **Client Secret** (API secret).

:::warning
Keep your API secret secure. Never share it or commit it to version control.
:::

### API key scopes

Each API key on Deribit is assigned a default access scope, which defines the maximum permissions.
Configure appropriate permissions when
[creating your API key](https://docs.deribit.com/articles/creating-api-key):

| Scope              | Required For                           |
| ------------------ | -------------------------------------- |
| `account:read`     | Account information, portfolio data.   |
| `trade:read`       | View orders and positions.             |
| `trade:read_write` | Place, modify, and cancel orders.      |
| `wallet:read`      | View balances and transaction history. |

**Recommended minimum for trading:** `account:read`, `trade:read_write`, `wallet:read`

:::tip
Follow the principle of least privilege. For data-only access (market data, no trading),
create a read-only key without `trade:read_write`.
:::

## Testnet

Deribit provides a testnet environment for testing strategies without real funds.
To use the testnet, set `environment=DeribitEnvironment.TESTNET` in your client configuration:

```python
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitEnvironment
from nautilus_trader.adapters.deribit import DeribitExecClientConfig
from nautilus_trader.adapters.deribit import DeribitProductType
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId

product_types = [DeribitProductType.FUTURE]
trader_id = TraderId.from_str("TRADER-001")
account_id = AccountId.from_str("DERIBIT-001")

data_config = DeribitDataClientConfig(
    product_types=product_types,
    environment=DeribitEnvironment.TESTNET,
)

exec_config = DeribitExecClientConfig(
    trader_id=trader_id,
    account_id=account_id,
    product_types=product_types,
    environment=DeribitEnvironment.TESTNET,
)
```

When testnet mode is enabled:

- HTTP requests use `https://test.deribit.com`.
- WebSocket connections use `wss://test.deribit.com/ws/api/v2`.
- Loads credentials from `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET` environment variables.

:::note
Testnet API keys are separate from production keys. Create API keys specifically
for the testnet through the testnet interface at [test.deribit.com](https://test.deribit.com).
:::

## Configuration

### Data client configuration options

| Option                             | Default    | Description                                                        |
| ---------------------------------- | ---------- | ------------------------------------------------------------------ |
| `api_key`                          | `None`     | Deribit API key. Loads from environment variables when omitted.    |
| `api_secret`                       | `None`     | Deribit API secret. Loads from environment variables when omitted. |
| `product_types`                    | `[FUTURE]` | Product types to load.                                             |
| `environment`                      | `MAINNET`  | Environment enum (`MAINNET` or `TESTNET`).                         |
| `base_url_http`                    | `None`     | Override for the HTTP JSON-RPC base URL.                           |
| `base_url_ws`                      | `None`     | Override for the WebSocket base URL.                               |
| `proxy_url`                        | `None`     | Optional proxy URL for HTTP and WebSocket transports.              |
| `http_timeout_secs`                | `60`       | Request timeout in seconds for HTTP calls.                         |
| `max_retries`                      | `3`        | Maximum retry attempts for recoverable errors.                     |
| `retry_delay_initial_ms`           | `1,000`    | Initial delay in milliseconds before retrying.                     |
| `retry_delay_max_ms`               | `10,000`   | Maximum delay in milliseconds between retries.                     |
| `heartbeat_interval_secs`          | `30`       | WebSocket heartbeat interval.                                      |
| `auth_timeout_secs`                | `None`     | Seconds to await the WebSocket auth result; unset means 30.        |
| `update_instruments_interval_mins` | `60`       | Accepted but not yet acted on; instruments load once on connect.   |
| `auto_load_missing_instruments`    | `False`    | Lazy-load uncached instruments on subscribe.                       |
| `transport_backend`                | `Sockudo`  | WebSocket transport backend.                                       |

#### Lazy-load on subscribe

`subscribe_*` commands look up the instrument in the local cache before sending the
WebSocket subscribe so the handler can parse the inbound frames. With
`auto_load_missing_instruments = False` (the default), a subscribe for an instrument that
was not preloaded (because of the configured `product_types`) returns an error up front
rather than silently succeeding and dropping subsequent frames at the handler.

Set `auto_load_missing_instruments = True` to instead fetch the instrument over HTTP on
the first subscribe, seed the WebSocket handler cache, and then forward the subscribe.
HTTP failures are logged and the WebSocket subscribe is skipped.

### Execution client configuration options

| Option                   | Default    | Description                                                        |
| ------------------------ | ---------- | ------------------------------------------------------------------ |
| `trader_id`              | Required   | Nautilus trader ID for generated reports and events.               |
| `account_id`             | Required   | Nautilus account ID for generated reports and events.              |
| `api_key`                | `None`     | Deribit API key. Loads from environment variables when omitted.    |
| `api_secret`             | `None`     | Deribit API secret. Loads from environment variables when omitted. |
| `product_types`          | `[FUTURE]` | Product types to load.                                             |
| `environment`            | `MAINNET`  | Environment enum (`MAINNET` or `TESTNET`).                         |
| `base_url_http`          | `None`     | Override for the HTTP JSON-RPC base URL.                           |
| `base_url_ws`            | `None`     | Override for the WebSocket base URL.                               |
| `proxy_url`              | `None`     | Optional proxy URL for HTTP and WebSocket transports.              |
| `http_timeout_secs`      | `60`       | Request timeout in seconds for HTTP calls.                         |
| `max_retries`            | `3`        | Maximum retry attempts for recoverable errors.                     |
| `retry_delay_initial_ms` | `1,000`    | Initial delay in milliseconds before retrying.                     |
| `retry_delay_max_ms`     | `10,000`   | Maximum delay in milliseconds between retries.                     |
| `auth_timeout_secs`      | `None`     | Seconds to await the WebSocket auth result; unset means 30.        |
| `transport_backend`      | `Sockudo`  | WebSocket transport backend.                                       |

`transport_backend` selects between the `Sockudo` and `Tungstenite` WebSocket transports. The
default is `Sockudo` when the `transport-sockudo` Cargo feature is enabled, which the published
Python wheels do; a Rust build without that feature defaults to `Tungstenite`.

### Production configuration

Below is an example live node using Deribit data and execution clients:

```python
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitDataClientFactory
from nautilus_trader.adapters.deribit import DeribitEnvironment
from nautilus_trader.adapters.deribit import DeribitExecClientConfig
from nautilus_trader.adapters.deribit import DeribitExecutionClientFactory
from nautilus_trader.adapters.deribit import DeribitProductType
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId

product_types = [DeribitProductType.FUTURE]
trader_id = TraderId.from_str("TRADER-001")
account_id = AccountId.from_str("DERIBIT-001")

node = (
    LiveNode.builder("DERIBIT-NODE-001", trader_id, Environment.LIVE)
    .add_data_client(
        None,
        DeribitDataClientFactory(),
        DeribitDataClientConfig(
            product_types=product_types,
            environment=DeribitEnvironment.MAINNET,
            api_key=None,
            api_secret=None,
        ),
    )
    .add_exec_client(
        None,
        DeribitExecutionClientFactory(),
        DeribitExecClientConfig(
            trader_id=trader_id,
            account_id=account_id,
            product_types=product_types,
            environment=DeribitEnvironment.MAINNET,
            api_key=None,
            api_secret=None,
        ),
    )
    .build()
)
```

### API credentials

There are multiple options for supplying your credentials to the Deribit clients.
Either pass the corresponding values to the configuration objects, or
set the following environment variables:

For Deribit live (production) clients:

- `DERIBIT_API_KEY`
- `DERIBIT_API_SECRET`

For Deribit testnet clients:

- `DERIBIT_TESTNET_API_KEY`
- `DERIBIT_TESTNET_API_SECRET`

:::tip
We recommend using environment variables to manage your credentials.
:::

### Product types

The `product_types` configuration option controls which Deribit product families are loaded.
Available options via the `DeribitProductType` enum:

- `DeribitProductType.FUTURE` - Perpetual and dated futures.
- `DeribitProductType.OPTION` - Call and put options.
- `DeribitProductType.SPOT` - Spot trading pairs.
- `DeribitProductType.FUTURE_COMBO` - Future spread instruments.
- `DeribitProductType.OPTION_COMBO` - Option spread instruments.

Example loading multiple product types:

```python
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitProductType

config = DeribitDataClientConfig(
    product_types=[
        DeribitProductType.FUTURE,
        DeribitProductType.OPTION,
    ],
    # ... other config
)
```

### Base URLs

The `environment` option selects these defaults. Set `base_url_http` or `base_url_ws` to point
either transport somewhere else, such as a local proxy or a capture harness.

| Environment | HTTP URL                   | WebSocket URL                      |
| ----------- | -------------------------- | ---------------------------------- |
| Production  | `https://www.deribit.com`  | `wss://www.deribit.com/ws/api/v2`  |
| Testnet     | `https://test.deribit.com` | `wss://test.deribit.com/ws/api/v2` |

## Server infrastructure

Deribit's matching engine is located in **Equinix LD4, Slough, UK**. For latency-sensitive strategies,
consider hosting in or near London. Colocation and cross-connect options are available directly
from Deribit for institutional clients.

For most users connecting via internet, the adapter's built-in retry logic, heartbeat monitoring,
and automatic reconnection handling provide reliable connectivity.

For more details, see the [Server Infrastructure article](https://support.deribit.com/hc/en-us/articles/25944617582877-Server-Infrastructure).

## Contributing

:::info
For additional features or to contribute to the Deribit adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
