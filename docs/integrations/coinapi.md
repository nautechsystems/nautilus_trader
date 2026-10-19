# CoinAPI

[CoinAPI](https://www.coinapi.io/products/market-data-api?ref=nautilus)
by [API Bricks](https://www.apibricks.io/?ref=nautilus) provides real-time and historical cryptocurrency
market data across multiple exchanges. Its REST and WebSocket APIs expose trades, quotes, order
books, and OHLCV bars through a common data format.

The NautilusTrader adapter supports live market data, historical requests, and loading downloaded
flat files. It has no execution client, account management, or order routing.

## Overview

The adapter is implemented in Rust with Python bindings and does not require an external CoinAPI
client library. Configure it through a live node with these Python components:

- `CoinApiDataClientConfig`: Live data client configuration.
- `CoinApiInstrumentSelection`: Exchanges, instruments, and metadata filters to load.
- `CoinApiDataClientFactory`: Factory registered with the node builder.

The Rust crate also exposes `CoinApiHttpClient` and `CoinApiWebSocketClient` for direct API access.

## Examples

- [Python examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/coinapi/)
- [Rust examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/coinapi/examples/)

The examples load Coinbase BTC/USD, stream quotes, trades, and one-minute bars, and request recent
bars. They use a 10-credit REST budget and a price precision override of 3. Edit the instrument and
subscription settings for your feed, and update the precision override when changing instruments.
Historical data can lag the live feed, so a request for recent bars may return no records.

## CoinAPI documentation

- [Market Data REST API](https://www.coinapi.io/products/market-data-api/docs/rest-api?ref=nautilus).
- [WebSocket V1 API](https://www.coinapi.io/products/market-data-api/docs/websocket?ref=nautilus).
- [Flat Files](https://www.coinapi.io/products/flat-files?ref=nautilus).

## Supported capabilities

| Capability                               | Interface                     | Status                                               |
| ---------------------------------------- | ----------------------------- | ---------------------------------------------------- |
| Spot, perpetual, and futures instruments | REST metadata                 | Native venue instrument IDs.                         |
| Order book deltas                        | WebSocket                     | L2 (`book`) and L3 (`book_l3`) with gap recovery.    |
| Fixed-depth book snapshots               | WebSocket                     | L2 at up to 50 levels (`book5`, `book20`, `book50`). |
| Current book snapshots                   | REST                          | L2 snapshot requests.                                |
| Historical book snapshots                | REST                          | Not supported.                                       |
| Quote ticks                              | WebSocket and historical REST | Live streams and historical requests.                |
| Trade ticks                              | WebSocket and historical REST | Live streams and historical requests.                |
| OHLCV bars                               | WebSocket and historical REST | Live streams and historical requests.                |
| Downloaded flat files                    | S3 CSV exports                | Rust and Python loaders.                             |
| Execution, orders, and account reports   | None                          | Out of scope.                                        |

Book types:

- L2 market-by-price (MBP): aggregate sizes by price level.
- L3 market-by-order (MBO): individual orders where the source and protocol expose them.

Fixed-depth subscriptions use the smallest channel that covers the requested depth. L3 requires
individual order IDs: an aggregated `book_l3` entry without an ID fails conversion and starts
recovery. An L3 book never syncs for a source that reports only aggregated levels.

Options, funding and open-interest metrics, FIX, flat file downloads, Hyperliquid L4 data, and
tokenized stocks are outside this adapter's scope. CoinAPI EMS execution is a separate product.

## Integrator attribution

REST requests and WebSocket connections include the fixed referral header `X-Referrer: nautilus`,
including retries and reconnects. It has no configuration override and carries no credential or
account data.

To refer a new account to NautilusTrader, sign up through the
[API Bricks console](https://console.apibricks.io/?ref=nautilus). Signup referrals and API request referrals
are separate mechanisms. See the
[API Bricks referral documentation](https://www.apibricks.io/platform/docs/general/referral?ref=nautilus).

## Authentication and environments

Set `api_key` in the data client config, or leave it unset to read `COINAPI_API_KEY`. Construction rejects
missing keys and keys containing characters outside visible ASCII. REST requests and WebSocket upgrades
authenticate with `X-CoinAPI-Key`. `hello` frames carry no credential. Credentials remain redacted in logs and
errors. Importing the Python package neither reads credentials nor connects.

Production uses encrypted REST and WebSocket endpoints. Sandbox access is not verified. Use a production
Market Data API key. No execution account configuration is required.

| Interface    | Production endpoint               | Role                        |
| ------------ | --------------------------------- | --------------------------- |
| REST         | `https://rest.coinapi.io/v1/`     | Primary GeoDNS endpoint.    |
| REST         | `https://api-emea.coinapi.io/v1/` | Optional regional endpoint. |
| REST         | `https://api-apac.coinapi.io/v1/` | Optional regional endpoint. |
| REST         | `https://api-ncsa.coinapi.io/v1/` | Optional regional endpoint. |
| WebSocket V1 | `wss://ws.coinapi.io/v1/`         | Primary streaming endpoint. |

REST defaults to GeoDNS. Set `base_url_http` to select a regional endpoint. The adapter does not switch
regions automatically. See CoinAPI's
[REST availability guide](https://www.coinapi.io/products/market-data-api/docs/rest-api/rest-high-availability?ref=nautilus)
and
[WebSocket endpoints](https://www.coinapi.io/products/market-data-api/docs/websocket/endpoints?ref=nautilus).

## Using CoinAPI with native adapters

CoinAPI uses native venue instrument IDs, such as `BTCUSDT.BINANCE`, and the data client ID `COINAPI`. Use
`client_id=COINAPI_CLIENT_ID` for CoinAPI requests and subscriptions when your node has multiple data clients.
Requests without an explicit client ID use venue or default routing. CoinAPI can be the default data client.

:::warning
Each instrument must have one definition source per node. Configure CoinAPI and native adapters
with disjoint instrument sets, including startup loads and refreshes. Sharing an instrument is
unsupported even when definitions match. Separate client IDs do not isolate definitions or feeds
for the same instrument, and the adapter does not detect or resolve these conflicts automatically.
:::

Check native instrument-loading defaults before combining providers. For example, Binance defaults to
`load_all=True`. Set it to `False` and provide non-overlapping `load_ids`. Keep the native adapter's venue
route for its other instruments.

CoinAPI's price and size resolutions describe its data and can differ from venue trading increments.
Ticks of `0.1` and `0.5` both use one decimal place but admit different prices. Native definitions
also carry trading constraints, including quantity and notional limits. Do not replace those
constraints with CoinAPI metadata.

## Instruments and symbology

### Supported instruments

The data client loads spot, perpetual, and dated futures instruments from the exchanges listed
below. Spot instruments become `CurrencyPair`, cryptocurrency perpetuals become `CryptoPerpetual`,
and cryptocurrency futures become `CryptoFuture` instruments. Binance USD-M commodity and equity
perpetuals, such as `XAUUSDT`, become `PerpetualContract` instruments.

Invalid, unsupported, or ambiguous definitions are skipped. If multiple definitions map
to the same native instrument ID, all of them are skipped. Selecting an instrument that cannot be
loaded causes the connection to fail.

### Metadata

CoinAPI's `price_precision` and `size_precision` metadata values are price and size increments,
not decimal-place counts or guaranteed venue trading increments. The adapter derives precision
from these resolutions, with optional [price precision overrides](#price-precision-overrides).

Base and quote currencies use CoinAPI's normalized asset codes. The multiplier comes from
`future_contract_unit`, and futures expiry comes from `future_delivery_time`. Activation uses
CoinAPI's `data_start` coverage date, which is not the venue listing time. Currency codes and
instrument classes can differ from a native adapter's definitions.

### Symbol mappings

Use the native `InstrumentId` forms below when configuring selections. Inverse contracts settle in the base
currency. Linear contracts settle in the quote currency. The adapter determines these properties from the
venue's product convention rather than CoinAPI's inverse flag.

| CoinAPI exchange | Symbol type | Exchange symbol form | Native `InstrumentId`          | Contract                     |
| ---------------- | ----------- | -------------------- | ------------------------------ | ---------------------------- |
| `BINANCE`        | `SPOT`      | `BTCUSDT`            | `BTCUSDT.BINANCE`              | Spot                         |
| `BINANCEFTS`     | `PERPETUAL` | `BTCUSDT`            | `BTCUSDT-PERP.BINANCE`         | Linear                       |
| `BINANCEFTS`     | `FUTURES`   | `BTCUSDT_261225`     | `BTCUSDT_261225.BINANCE`       | Linear                       |
| `BYBITSPOT`      | `SPOT`      | `BTCUSDT`            | `BTCUSDT-SPOT.BYBIT`           | Spot                         |
| `BYBIT`          | `PERPETUAL` | `BTCUSDT`            | `BTCUSDT-LINEAR.BYBIT`         | Linear                       |
| `BYBIT`          | `FUTURES`   | `BTCUSDT-30OCT26`    | `BTCUSDT-30OCT26-LINEAR.BYBIT` | Linear                       |
| `BYBITINVERSE`   | `PERPETUAL` | `BTCUSD`             | `BTCUSD-INVERSE.BYBIT`         | Inverse                      |
| `BYBITINVERSE`   | `FUTURES`   | `BTCUSDZ26`          | `BTCUSDZ26-INVERSE.BYBIT`      | Inverse                      |
| `OKEX`           | `SPOT`      | `BTC-USDT`           | `BTC-USDT.OKX`                 | Spot                         |
| `OKEX`           | `PERPETUAL` | `BTC-USD-SWAP`       | `BTC-USD-SWAP.OKX`             | Inverse for USD, else linear |
| `OKEX`           | `FUTURES`   | `BTC-USD-261225`     | `BTC-USD-261225.OKX`           | Inverse for USD, else linear |
| `COINBASE`       | `SPOT`      | `BTC-USD`            | `BTC-USD.COINBASE`             | Spot                         |
| `KRAKEN`         | `SPOT`      | `XBT/USD`            | `BTC/USD.KRAKEN`               | Spot                         |
| `KRAKENFTS`      | `PERPETUAL` | `PI_XBTUSD`          | `PI_XBTUSD.KRAKEN`             | `PI_` inverse, `PF_` linear  |

Only the listed exchange and product forms are supported. Native symbols must match the provider's
asset and expiry metadata. For Kraken spot, `XBT` maps to `BTC` and `XDG` maps to `DOGE`.
The adapter retains the full CoinAPI `symbol_id`, including any disambiguating suffix, for data
requests and subscriptions.

### Unsupported contracts

Other exchanges and symbol types are unsupported. This includes Binance COIN-M (`BINANCEFTSC`):
CoinAPI reports a one-coin contract unit for `BTCUSD_PERP`, while the native Binance contract is
100 USD.

Kraken dated futures (`KRAKENFTS` `FUTURES`) are also unsupported. CoinAPI reports midnight UTC on the
delivery date. Kraken's last trading time varies by product: 08:00 UTC for `FF_`, for example. For `FI_`
contracts, it is 15:00 or 16:00 UTC.

Other rejected forms include Bybit USDC contracts, OKX USD-margined futures such as
`BTC-USD_UM-261225`, and ambiguous Binance delivery IDs with a `USD` quote, such as
`BTCUSD_261225.BINANCE`. A symbol that resembles a supported native ID does not by itself establish
that its product is supported.

## Data subscriptions

Subscribing requires a connected client and a loaded instrument, and uses the `symbol_id` loaded by
`connect`. All subscriptions share the single WebSocket connection described in
[Raw WebSocket streams](#raw-websocket-streams).

| Subscription   | CoinAPI type                | Event             | `ts_event`        | `ts_init`                   |
| -------------- | --------------------------- | ----------------- | ----------------- | --------------------------- |
| L2 book deltas | `book`                      | `OrderBookDeltas` | `time_exchange`   | `time_coinapi`              |
| L3 book deltas | `book_l3`                   | `OrderBookDeltas` | `time_exchange`   | `time_coinapi`              |
| Book depth     | `book5`, `book20`, `book50` | `OrderBookDepth`  | `time_exchange`   | `time_coinapi`              |
| Quotes         | `quote`                     | `QuoteTick`       | `time_exchange`   | `time_coinapi`              |
| Trades         | `trade`                     | `TradeTick`       | `time_exchange`   | `time_coinapi`              |
| Bars           | `ohlcv`                     | `Bar`             | `time_period_end` | Adapter clock when emitted. |

### Trade aggressor sides

A trade's aggressor side is `BUY` or `SELL` only when the exchange reported it. CoinAPI's
`BUY_ESTIMATED` and `SELL_ESTIMATED` values are its own inferences, so they map to `NoAggressor`
together with `UNKNOWN` and unrecognized values.

### Bars

Bar subscriptions accept external last-price bar types with these CoinAPI streaming periods:

- 1, 2, 3, 4, 5, 6, 10, 15, 20, or 30 seconds or minutes.
- 1, 2, 3, 4, 6, 8, or 12 hours.
- One day.

The client rejects internal aggregation and other specifications.

CoinAPI sends running OHLCV totals when it sees a timestamp in the next period. It also sends them every five
seconds during period updates. These totals carry no closed flag. The client retains the latest update for
each period and emits a bar when an update for a later period arrives. It discards retained periods on
reconnect, since missed updates can leave them incomplete, and when a bar subscription starts.

### Subscription changes

Subscription changes apply in call order and share one WebSocket connection. CoinAPI does not acknowledge
scope changes. The first matching data confirms that a subscription is active.

Changes are paced at least two seconds apart to meet CoinAPI's subscription limits. Queued changes and book
recovery can delay them further. Unsubscribing from all streams closes the connection. A later subscription
opens a new one and retains this pacing.

A failed connection attempt retries after one second, doubling to at most 30 seconds until success or
disconnect. Later subscription changes wait behind the pending batch. A transport reconnect restores
subscriptions automatically. A CoinAPI `error` ends the streams without resubscribing. The next subscription
opens a new connection.

## Order books

### Channels and depths

`L2_MBP` delta subscriptions stream CoinAPI `book` and `L3_MBO` subscriptions stream `book_l3`. `L1_MBP` is
rejected. Both channels send the whole book, so a delta subscription ignores `depth`.

Depth subscriptions accept only `L2_MBP`. They use the smallest of `book5`, `book20`, and `book50` that covers
the requested depth (default 10 levels). A depth above 50 is rejected. Each depth message lists complete top
levels, is flagged `F_SNAPSHOT`, and carries zero order counts, since CoinAPI reports none.

### Channel conflicts

CoinAPI allows one book type per connection, so every book subscription on a client must use the same channel.
A second subscription fails if it needs another channel, such as L3 beside L2 deltas or depth 5 beside depth
20. A second subscription for the same instrument with a different depth also fails. Unsubscribe the active
books first to switch channels.

### Snapshot and delta events

A delta stream starts with a snapshot: a `Clear` followed by an `Add` per level, all flagged
`F_SNAPSHOT`, with `F_LAST` on the final delta. Each update becomes one `OrderBookDeltas` event
ending in `F_LAST`. For L3 books, CoinAPI reports `SUBTRACT` and `MATCH` sizes as the amount
removed, so the client tracks each order's remaining size and deletes an order once it is
exhausted.

### Sequence numbers

CoinAPI numbers each book stream from one per connection and restarts that numbering whenever it
restarts the stream. The data client keeps emitted sequence numbers
increasing across restarts and reconnects, so a managed `OrderBook` can consume the stream directly.

### Recovery

Recovery applies to delta subscriptions. A sequence gap, an update before the first snapshot, or
an invalid level stops book output until the client receives a valid replacement snapshot.
Updates received while waiting for that snapshot are discarded.

Depth subscriptions receive complete snapshots. The client drops an invalid depth message with a warning. The
next valid message replaces it, without a recovery attempt or snapshot deadline.

### Snapshot deadlines and retry limits

`book_snapshot_timeout_secs` sets the wait for a snapshot after subscribing, reconnecting, or starting
recovery. It defaults to 10 seconds. `0` disables the deadline. A missing snapshot starts or retries recovery.
Unsubscribing cancels recovery, and a CoinAPI `error` ends all book streams.

Book restarts are at least five seconds apart, and other subscription changes wait while a restart is pending.
Recovery makes up to eight attempts within 180 seconds, with backoff and jitter. It then retries until it
accepts a snapshot. The retry interval doubles from one minute to fifteen minutes. Reconnecting waits for
fresh book snapshots before output resumes.

### Effects on other streams

When other instruments are subscribed, recovering a book pauses every stream for the affected
instrument for about two seconds. Any retained OHLCV period spanning the pause is discarded to
avoid publishing an incomplete bar.

When the affected instrument is the only subscribed instrument, its quotes, trades, and bars
continue during book recovery. If the book is the only subscription, recovery keeps the connection
open without publishing an additional trade stream.

## Historical requests

Historical requests require a connected client and a loaded instrument and use its stored `symbol_id`.
Responses preserve the correlation ID, requested client ID, instrument or bar type, `start`, `end`, and
`params`. An absent client ID defaults to the adapter's ID. An absent `end` stays absent.

| Request       | CoinAPI endpoint                  | Response       | `ts_event`        | `ts_init`         |
| ------------- | --------------------------------- | -------------- | ----------------- | ----------------- |
| Trades        | `/trades/{symbol_id}/history`     | `TradeTick`s   | `time_exchange`   | `time_exchange`   |
| Quotes        | `/quotes/{symbol_id}/history`     | `QuoteTick`s   | `time_exchange`   | `time_exchange`   |
| Bars          | `/ohlcv/{symbol_id}/history`      | `Bar`s         | `time_period_end` | `time_period_end` |
| Book snapshot | `/orderbooks/{symbol_id}/current` | L2 `OrderBook` | `time_exchange`   | Not applicable.   |

Historical quotes, trades, and bars use their event time for `ts_init` as well as `ts_event`,
so response filtering preserves the requested exchange-time window.

### Time windows and limits

Trade, quote, and bar requests require `start` and page through [`start`, `end`), using now when
`end` is absent. `limit` caps the records returned, counting from `start`. Paging follows
[Raw HTTP requests](#raw-http-requests), and the client logs a warning when the credit budget
truncates a response.

Request bounds are rounded up to CoinAPI's 100-nanosecond timestamp resolution, preserving the
selection of records on that time grid.

### History lag

CoinAPI's history endpoints lag the live feed. A recent time window can return no records or an
incomplete result even when the live stream is active. If this happens, request an earlier window,
such as the previous day. The adapter uses historical endpoints without falling back to latest-data
endpoints.

### Closed bars

Bar requests accept the same bar types as bar subscriptions. A response includes only periods that closed by
`end` and by the time the response arrives. A period still in progress is never returned as a bar. `limit`
counts CoinAPI periods before that exclusion.

### Current book snapshots

Book snapshot requests pass `depth` as `limit_levels`, or omit it to use CoinAPI's default depth.
A level finer than the instrument precision rejects the whole snapshot rather than returning a
partial book.

### Failed requests

A failed request logs the error and sends no response.

## Precision

The client converts prices and sizes at the instrument's price and size precision. These come from
CoinAPI data resolutions, which can lag the stream, so a value with more decimal places is dropped
with a warning rather than rounded. Trailing zeros do not count as decimal places. Historical
responses apply the same rule to each record and log one warning with the dropped count.

OHLCV volume has a bounded exception for small numeric tails. The client rounds it half to even at the
instrument's size precision only if the adjustment meets both limits:

- At most `1e-15` times the reported volume.
- At most `1e-6` times one unit at that precision.

For size precision 8, the absolute cap is `0.00000000000001`. Negative volumes and larger precision mismatches
are rejected. Prices keep the exact-conversion rule.

The absolute cap stays fixed as volume grows, so larger aggregate volumes can still be rejected
even when their tails meet the relative bound.

A book level finer than the configured precision rejects the whole message. A delta subscription starts
recovery. A depth subscription drops the message. If CoinAPI's price resolution is too coarse for the feed,
set a [price precision override](#price-precision-overrides). Size precision has no override.

### Price precision overrides

Set `price_precisions` to replace the price precision for a selected instrument. The client
applies it when it builds the instrument, so the definition, streams, and historical responses all
use it. An override for an instrument outside the selections, or above the maximum Nautilus
precision, fails at construction. Size precision has no override.

```python
data_config = CoinApiDataClientConfig(
    instruments=[
        CoinApiInstrumentSelection(
            "COINBASE",
            [InstrumentId.from_str("LTC-USD.COINBASE")],
            filter_symbol_id="COINBASE_SPOT_LTC_USD",
        ),
    ],
    price_precisions={InstrumentId.from_str("LTC-USD.COINBASE"): 3},
)
```

In Rust, pass the same map to `CoinApiDataClientConfig::builder().price_precisions(...)`.

### OHLCV precision warnings

:::warning
CoinAPI OHLCV prices can carry more decimal places than instrument metadata allows, even when
trade prices fit. For example, a BTC/USD OHLCV price of `84761.955` does not fit a `0.01` resolution.
Set a suitable `price_precisions` override to avoid missing bars.

Historical OHLCV volumes can also contain small numeric tails, such as `1.2582318399999999` for `1.25823184`.
The client accepts them within the [bounded tolerance](#precision). Larger mismatches still drop the bar.
:::

## Connection lifecycle

`disconnect` cancels pending subscriptions and requests before closing the connection. Queued
subscriptions cannot reopen it, and canceled requests send no response. A later `connect` reloads
instruments and allows new subscriptions and requests. `stop`, `reset`, and `dispose` cancel pending
work and drop the connection without waiting.

## Usage and access constraints

### Account limits

REST usage is subject to concurrency limits and data credits. WebSocket usage is metered by data transfer.
Check your account's entitlements, connection allowance, and
[current pricing](https://www.coinapi.io/products/market-data-api/pricing?ref=nautilus) before choosing feeds.
A plan's transfer allowance does not guarantee access to every channel or source.

The pricing page lists these daily allowances as of October 2026. Check the linked page for changes
and overage charges.

| Plan          | REST credits | Tier 1 transfer | Tier 2 transfer |
| ------------- | ------------ | --------------- | --------------- |
| Pay As You Go | Metered      | Metered         | Metered         |
| Startup       | 1,000        | 32 GiB          | Metered         |
| Streamer      | 10,000       | 128 GiB         | 1 GiB           |
| Pro           | 100,000      | 512 GiB         | 64 GiB          |
| Enterprise    | Custom       | Custom          | Custom          |

CoinAPI classifies trades, quotes, and L2 books as Tier 1, and OHLCV as Tier 2. See the
[usage FAQ](https://www.coinapi.io/products/market-data-api/faq?ref=nautilus). Its
[L3 pricing guide](https://www.coinapi.io/blog/tier-1-vs-tier-2-market-data-level-3-order-book?ref=nautilus)
classifies continuous L3 streams as Tier 2. Confirm billing and access for your chosen feeds.

Each client uses one WebSocket connection. Other nodes or clients using the same account can occupy
its available connection slots and cause HTTP 429 responses. Reduce concurrent connections to fit
your account limit. Exhausting a hard-limited REST quota also returns HTTP 429.

:::warning
CoinAPI combines the session's data types, symbols, and bar periods. Trades for ten symbols plus a
book for one symbol can therefore transfer books for all ten. The adapter filters out unrequested
pairs, but their transfer is still billable. Consider this when combining feeds on one client.
:::

### Local credit budget

The data client's `credit_budget` caps the credits one client instance may spend and defaults to
100. Each symbol-metadata attempt costs one local credit, regardless of response size. Use `filter_symbol_id`
     to reduce transfer and parsing work. Allow for all historical requests: a range request stops at the
     budget and returns a truncated response. For bulk history, downloaded [flat files](#flat-files) avoid
     repeated REST requests. Compare their transfer cost with REST credits.

`max_concurrent_requests` bounds simultaneous REST requests. The local credit budget applies throughout the
client's lifetime and is separate from CoinAPI billing. Each attempt reserves `ceil(limit / 100)` credits for
historical requests, or one credit for requests without a record limit.

CoinAPI bills historical requests per 100 returned records. Thus, a successful historical page refunds the
unused reservation above `ceil(records / 100)`. The local minimum charge is one credit. Failed, retried, and
canceled attempts are not refunded. The client backs off on HTTP 429 and fails promptly on HTTP 403. See the
[API limits reference](https://www.coinapi.io/products/market-data-api/docs/api-limits-and-billing-metrics?ref=nautilus).

### Backfills

Set an explicit record limit and time window for historical requests. Without a record limit, one
request can reserve the remaining credit budget for its first page and leave none for other
requests. Each historical page has its own REST retry budget.

For bulk history, compare REST credit costs with downloaded [flat files](#flat-files). Fewer HTTP calls do not
necessarily mean fewer billable credits. Download files separately before loading them. The adapter does not
list or download S3 objects.

## Raw HTTP requests

For direct Rust access, `CoinApiHttpClient` accepts a `SecretString` API key and `CoinApiHttpConfig`. The base
URL includes `/v1/`. Authentication uses `X-CoinAPI-Key`. Redirects are rejected. The client ignores system
proxy environment variables and connects directly unless `proxy_url` is set.

The raw client exposes exchange and active-symbol metadata, historical trades, quotes, and OHLCV, and
single-symbol current L2 and L3 books. Financial values retain their exact decimal representation. Values
outside `Decimal` precision fail parsing instead of rounding.

### Single pages and ranges

The `get_trades`, `get_quotes`, and `get_ohlcv` methods return one page for an explicit time window and record
limit in `[1, 100000]`. A full page does not mean the window is complete. The `get_trades_range`,
`get_quotes_range`, and `get_ohlcv_range` methods page through a whole `HistoryRange`, whose optional limit
caps the total records and can exceed 100,000.

### Time boundaries

Trade and quote ranges select `[time_start, time_end)` by exchange time. OHLCV ranges select periods by their
start time. The data client also excludes periods that have not closed. Records outside the requested window
are filtered out.

### Pagination

#### Page sizes and completion

Each page requests the largest limit that fits the remaining request limit, the 100,000-record
maximum, and the remaining credit budget. A short page, or a full page reaching the request limit,
completes the range. With the default 100-credit budget, a page holds at most 10,000 records, so
continuation beyond one page needs a larger budget.

#### Records sharing a timestamp

Pagination avoids duplicate records at page boundaries by rereading a trailing timestamp group
when a full page needs continuation. Paging fails if records at one timestamp fill a whole page,
preventing cursor advancement, or if a page is not in ascending order.

#### Records outside the window

Records outside the window are dropped. CoinAPI returns nearby OHLCV periods from the same UTC
date when the window has no trades, and a page holding only such periods completes the range.

#### Credit exhaustion and concurrent ranges

If the credit budget cannot fund the next page, the result carries the records gathered so far and sets
`is_truncated=true`. The budget can be insufficient after a failed attempt keeps its reservation. Without
affordable credit for the first page, the request fails. Range requests on one client run one at a time, so
each sizes its pages after earlier ranges have settled their credits.

## Raw WebSocket streams

For direct Rust access, `CoinApiWebSocketClient` takes an API key and `CoinApiWebSocketConfig` and
returns a message receiver. Its default endpoint is `wss://ws.coinapi.io/v1/`. A `Subscription`
names a data type, an exact full CoinAPI `symbol_id`, and an OHLCV period where required.

The client decodes `trade`, `quote`, `book`, `book5`, `book20`, `book50`, `book_l3`, and `ohlcv`. It preserves
decimal values, logs and drops malformed frames, and ignores metadata feeds. Subscription changes return when
queued. Receiving matching data confirms the subscription. The [subscription pacing](#subscription-changes),
[book channel limits](#channel-conflicts), and [billable combined scope](#account-limits) apply to this client
too.

### Reconnects and errors

Five seconds without a message triggers a reconnect. The first attempt is immediate. Later attempts back off
from one second to 30 seconds, doubling with up to 500 milliseconds of jitter. On an account with one
connection slot, reconnects can receive HTTP 429 until CoinAPI releases the old connection.

Reconnection restores the scope and emits `CoinApiWsMessage::Reconnected`. Raw book sequences
restart per connection, so clear your book state after reconnecting. The data client handles book
state and sequence continuity automatically.

A server `reconnect` notice does not interrupt streaming. The client reconnects when the server closes. A
CoinAPI `error` emits `CoinApiWsMessage::Error`, clears all subscriptions, and ends the connection without
reconnecting. The next subscription opens a new connection. Authentication errors can arrive after a
successful WebSocket upgrade. Closing or dropping the client closes its connection.

## Flat files

CoinAPI [Flat Files](https://www.coinapi.io/products/flat-files?ref=nautilus) are daily or hourly `.csv.gz`
exports served over an S3-compatible API. The adapter loads downloaded files into Nautilus data. It does not
list or download them. The loaders follow the Tardis CSV loaders. Each `load_*` function returns a whole file,
and each `stream_*` function yields it in chunks. Both take optional price and size precisions and a record
`limit`.

| Schema                        | Load                           | Stream                  | Output           |
| ----------------------------- | ------------------------------ | ----------------------- | ---------------- |
| `T-TRADES`                    | `load_coinapi_trades`          | `stream_coinapi_trades` | `TradeTick`      |
| `T-QUOTES`                    | `load_coinapi_quotes`          | `stream_coinapi_quotes` | `QuoteTick`      |
| `T-LIMITBOOK_FULL`            | `load_coinapi_deltas`          | `stream_coinapi_deltas` | `OrderBookDelta` |
| `T-OHLCV_ACTIVE_CONSOLIDATED` | `CoinApiOhlcvLoader.load_bars` | None                    | `Bar`            |

In Rust, the same functions are `nautilus_coinapi::csv::{load_trades, stream_trades, ...}` and
`CoinApiOhlcvLoader::load_bars`.

### Order book example

The load call derives the date from `D-20250901`. The stream reads a renamed file, so it passes
`date` explicitly and sets precisions for the instrument's increments instead of inferring them.
These precisions must fit every value: finer values fail the stream rather than being rounded.

```rust tab="Rust"
use jiff::civil::date;
use nautilus_coinapi::csv::{load_deltas, stream_deltas};
use nautilus_model::identifiers::InstrumentId;

let instrument_id = InstrumentId::from("BTC-USD.COINBASE");

let path = "T-LIMITBOOK_FULL/D-20250901/E-COINBASE/IDDI-27576+SC-COINBASE_SPOT_BTC_USD+S-BTC__002DUSD.csv.gz";
let deltas = load_deltas(path, instrument_id, None, None, None, None)?;

let chunks = stream_deltas(
    "coinbase_btc_usd_book.csv.gz",
    instrument_id,
    Some(date(2025, 9, 1)),
    100_000,    // chunk_size
    Some(2),    // price_precision
    Some(8),    // size_precision
    None,       // limit
)?;

for chunk in chunks {
    let deltas = chunk?;
}
```

```python tab="Python"
import datetime

from nautilus_trader.adapters.coinapi import load_coinapi_deltas
from nautilus_trader.adapters.coinapi import stream_coinapi_deltas
from nautilus_trader.model import InstrumentId

instrument_id = InstrumentId.from_str("BTC-USD.COINBASE")

path = "T-LIMITBOOK_FULL/D-20250901/E-COINBASE/IDDI-27576+SC-COINBASE_SPOT_BTC_USD+S-BTC__002DUSD.csv.gz"
deltas = load_coinapi_deltas(path, instrument_id)

for chunk in stream_coinapi_deltas(
    "coinbase_btc_usd_book.csv.gz",
    instrument_id,
    date=datetime.date(2025, 9, 1),
    price_precision=2,
    size_precision=8,
):
    ...
```

### Bars example

`CoinApiOhlcvLoader` maps numeric `symbol_id` values to instruments. `27576` is the REST `symbol_id_int` of
`COINBASE_SPOT_BTC_USD`. It reads the period from `TP-1MIN` and skips unmapped symbols. Rust also provides
`CoinApiOhlcvLoader::from_symbols` to build the map from metadata.

```rust tab="Rust"
use nautilus_coinapi::csv::CoinApiOhlcvLoader;
use nautilus_model::identifiers::InstrumentId;

let loader = CoinApiOhlcvLoader::new([(27_576, InstrumentId::from("BTC-USD.COINBASE"))]);
let bars = loader.load_bars(
    "T-OHLCV_ACTIVE_CONSOLIDATED/D-20250911/E-COINBASE/TP-1MIN.csv.gz",
    None,   // period_id, read from the file name
    None,   // limit
)?;
```

```python tab="Python"
from nautilus_trader.adapters.coinapi import CoinApiOhlcvLoader
from nautilus_trader.model import InstrumentId

loader = CoinApiOhlcvLoader({27576: InstrumentId.from_str("BTC-USD.COINBASE")})
bars = loader.load_bars("T-OHLCV_ACTIVE_CONSOLIDATED/D-20250911/E-COINBASE/TP-1MIN.csv.gz")
```

### File layout

#### Symbol files

The loaders accept semicolon-delimited files with a header row. Trade, quote, and book files
hold one symbol under
`T-{SCHEMA}/D-{YYYYMMDD|YYYYMMDDHH}/E-{EXCHANGE}/IDDI-{ID}+SC-{SYMBOL_ID}+S-{EXCHANGE_SYMBOL}.csv.gz`.
Pass an explicit `instrument_id` for each file.

CoinAPI also documents export variants with comma delimiters, full date-time book timestamps, or numeric book
update types. These loaders require semicolon delimiters, time-of-day book timestamps, and named book updates
such as `SNAPSHOT`, `ADD`, and `SET`. Convert other layouts before loading them. See the
[full-book dataset guide](https://www.coinapi.io/products/flat-files/docs/datasets/limitbook?ref=nautilus).

#### Buckets and retention

The `coinapi` bucket holds historical data: daily partitions before 2026-06-09 and hourly
trade, quote, and full-book partitions from that date. The `coinapi-daily-tail` bucket holds only
the previous day's daily trade, quote, and full-book files, retained for 24 hours. It is not an
archive and contains no OHLCV. See the
[file structure](https://www.coinapi.io/products/flat-files/docs/datasets?ref=nautilus).

#### OHLCV files

The [OHLCV dataset guide](https://www.coinapi.io/products/flat-files/docs/datasets/ohlcv?ref=nautilus)
documents exchange-wide daily files in the `coinapi` bucket under
`T-OHLCV_ACTIVE_CONSOLIDATED/D-{YYYYMMDD}/E-{EXCHANGE}/TP-{PERIOD}.csv.gz`, one file per period of `1SEC`,
`1MIN`, `1HRS`, or `1DAY`. Each row's numeric `symbol_id` is the REST `symbol_id_int`. The loader maps it to
an instrument, skips unmapped rows, and reads the period from `TP-` unless `period_id` is given. The general
file-structure page also lists hourly OHLCV paths. Confirm available objects when downloading. The loader
reads the timestamps from the rows for either layout.

### Order book data

Book files with order IDs produce L3 market-by-order deltas. Files without them produce L2 market-by-price
deltas. For example, Coinbase spot files contain order IDs, while Binance spot files describe price levels.
Use the corresponding `BookType` when applying the deltas.

The initial snapshot emits a `Clear` followed by `Add` deltas, all flagged `F_SNAPSHOT`. Subsequent updates at
one exchange timestamp form one event ending in `F_LAST`. Streaming chunks normally end at event boundaries
and can exceed `chunk_size`. A record `limit` can cut the final event short. Its last delta still carries
`F_LAST`, so a limited load can leave an incomplete book.

L3 fill updates reduce an order's remaining size and remove it at zero. A later add can reuse an
order ID after deletion. Updates to unknown orders, other than deletes, are logged and skipped.

### Timestamps

Trade, quote, and OHLCV rows carry zone-less UTC date-times such as `2025-09-01T00:00:00.1780000`. Book rows
carry only a time of day, such as `00:00:00.0140000`. Thus, the book loaders date each row from the file's
`D-` partition. For a file outside a `D-` partition, they use `date`. A snapshot stamped later than the first
update carries the previous day's closing time and moves back one day. A `time_coinapi` more than half a day
below its `time_exchange` falls after midnight and moves forward one day.

`ts_event` is `time_exchange` and `ts_init` is `time_coinapi`. Bars take `time_period_end` for
both.

### Precision

Prices and sizes convert exactly from text. `load_*` infers unspecified precisions from all rows read,
stopping at `limit` if set. `stream_*` samples the first 10,000 rows and raises if a later value needs
rounding. Pass explicit precisions when the file's precision is unknown. The OHLCV loader infers precisions
per instrument, with the volume exception below.

CoinAPI uses [high-precision mode](../getting_started/installation.md#precision-mode) by default: 16 decimal
places rather than 9, to fit cryptocurrency sizes and CoinAPI volumes. OHLCV `sx_sum` can carry numeric tails
consistent with floating-point noise, such as `689.5507347100003`. The loader rounds volumes half to even at
the build's maximum precision, changing only values that exceed it, such as this example in a
standard-precision build. It rejects negative source volumes and positive volumes that round to zero. Zero
source volumes remain valid.

## Configuration

Register `CoinApiDataClientFactory` and `CoinApiDataClientConfig` with the node builder. The default client ID
is `COINAPI`. A custom client name becomes its client ID.

| Field                        | Default   | Purpose                                                     |
| ---------------------------- | --------- | ----------------------------------------------------------- |
| `api_key`                    | `None`    | API key; `None` reads `COINAPI_API_KEY`.                    |
| `base_url_http`              | `None`    | REST URL override, including `/v1/`.                        |
| `base_url_ws`                | `None`    | WebSocket URL override.                                     |
| `proxy_url`                  | `None`    | Explicit HTTP(S) proxy for REST and WebSocket.              |
| `instruments`                | Empty     | Instrument selections loaded on connect.                    |
| `credit_budget`              | `100`     | Local lifetime REST credit allowance for this client.       |
| `max_retries`                | `3`       | Maximum retries for a failed REST request.                  |
| `http_timeout_secs`          | `30`      | Per-attempt REST timeout, including concurrency admission.  |
| `max_concurrent_requests`    | `1`       | Maximum simultaneous REST requests.                         |
| `retry_delay_initial_ms`     | 1,000     | Initial REST retry delay.                                   |
| `retry_delay_max_ms`         | 10,000    | Maximum REST backoff; provider retry delays can exceed it.  |
| `retry_max_elapsed_ms`       | 120,000   | Total budget per REST GET, including admission and retries. |
| `transport_backend`          | `Sockudo` | Shared WebSocket backend; `Tungstenite` is also available.  |
| `book_snapshot_timeout_secs` | `10`      | Seconds to wait for a book snapshot; `0` disables.          |
| `price_precisions`           | Empty     | Price precision overrides by instrument ID.                 |

Configuration rejects unknown fields. Unset URL overrides use the production endpoints. Sockudo is the default
WebSocket backend. Set `transport_backend=TransportBackend.TUNGSTENITE` to use Tungstenite instead, importing
`TransportBackend` from `nautilus_trader.network`.

`proxy_url` routes both transports through an explicit HTTP(S) proxy. Ambient system proxies remain disabled.
The URL must include `http://` or `https://`. SOCKS URLs fail at client construction. Proxy credentials stay
redacted in Rust debug output and are not exposed by Python getters. The Python `has_proxy_url` property
reports whether an explicit proxy is configured.

`credit_budget`, `max_concurrent_requests`, `retry_max_elapsed_ms`, and `http_timeout_secs` must be positive.
Each historical page has its own REST retry budget. The initial retry delay must be positive, and the maximum
must be at least the initial delay. Invalid backoff settings fail at client construction. The per-attempt
timeout includes waiting for concurrency admission and does not include retry delays. The total elapsed budget
includes both.

### Python configuration

The `nautilus_trader.adapters.coinapi` package exports `CoinApiDataClientConfig`,
`CoinApiInstrumentSelection`, `CoinApiDataClientFactory`, and the `COINAPI` and `COINAPI_CLIENT_ID` constants.
The config takes the fields above as keyword arguments. It exposes each field except `api_key` and `proxy_url`
as a read-only property. Its representation omits every value. A selection takes the exchange, the instrument
IDs, and the optional `filter_symbol_id` and `filter_asset_id` filters.

```python
from nautilus_trader.adapters.coinapi import CoinApiDataClientConfig
from nautilus_trader.adapters.coinapi import CoinApiDataClientFactory
from nautilus_trader.adapters.coinapi import CoinApiInstrumentSelection
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId

instrument_id = InstrumentId.from_str("BTC-USD.COINBASE")

data_config = CoinApiDataClientConfig(
    instruments=[
        CoinApiInstrumentSelection(
            "COINBASE",
            [instrument_id],
            filter_symbol_id="COINBASE_SPOT_BTC_USD",
        ),
    ],
    price_precisions={instrument_id: 3},
)

node = (
    LiveNode.builder("COINAPI-001", TraderId("TESTER-001"), Environment.LIVE)
    .add_data_client(None, CoinApiDataClientFactory(), data_config)
    .build()
)
```

The factory creates the client during `build`, where invalid credentials, endpoint or proxy URLs, zero
concurrency, elapsed budgets, or attempt timeouts, and invalid retry backoff settings raise an error. The
example reads its key from `COINAPI_API_KEY`. It overrides BTC/USD price precision to admit three-decimal
OHLCV prices. See [Precision](#precision). For general node setup, see
[Configure live trading](../how_to/configure_live_trading.md).

### Instrument selections

Each `CoinApiInstrumentSelection` names a CoinAPI exchange, the native instrument IDs to load, and optional
`filter_symbol_id` and `filter_asset_id` metadata filters. Construction rejects empty ID lists, IDs mapped to
another CoinAPI exchange, and IDs selected more than once. Use `filter_symbol_id` to limit metadata transfer
and parsing. Each request attempt costs one local credit regardless of response size.

Connecting loads each selection once and fails if any selected instrument is missing. Only selected
instruments are published. Use explicit selections to keep CoinAPI and native adapters
[disjoint](#using-coinapi-with-native-adapters).

Instrument requests always fetch from CoinAPI and refresh the definitions used for stream conversion
and the `symbol_id` values used for REST requests. Subscriptions retain the IDs loaded by `connect`,
including after a transport reconnect. Disconnect and reconnect the data client to use refreshed
IDs for subscriptions.

- `request_instrument` accepts only a configured ID and returns an error for any other.
- `request_instruments` returns the configured instruments for the requested venue. If the request names no
  venue, it returns all configured instruments. A response without a requested venue carries the `COINAPI`
  venue.
- A request for a venue without configured instruments returns an empty response without a REST
  call.

## Troubleshooting

| Symptom                                              | Likely cause                                                                                         | Action                                                                                        |
| ---------------------------------------------------- | ---------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `CoinAPI API key not configured` at build            | Neither `api_key` nor `COINAPI_API_KEY` is set.                                                      | Set one of them.                                                                              |
| `subscription change failed, retrying` with HTTP 429 | Another client or node holds the account's only connection.                                          | Stop the other connection; the client retries until CoinAPI frees the slot.                   |
| `WsHelloPerIp` error, then no data                   | Too many `hello` messages from this IP, including other clients.                                     | Reduce subscription churn on the IP; the next subscription reconnects.                        |
| `Conflicting CoinAPI book subscription`              | The instrument already streams another book channel or depth.                                        | Unsubscribe its book first, or request the same channel and depth.                            |
| `CoinAPI streams one book channel per connection`    | Another instrument streams a different book channel.                                                 | Use the active channel, or unsubscribe every book first.                                      |
| Repeated `Invalid book message` warnings             | Book levels exceed instrument precision, or L3 entries lack order IDs.                               | Override price precision if needed, or use L2 for a source without order IDs.                 |
| `Dropping CoinAPI` quote, trade, or bar warnings     | Prices or sizes exceed the instrument's precision; OHLCV volume needs rounding beyond its tolerance. | Override price precision if needed; size precision and the volume tolerance have no override. |
| Historical request for the last hour returns empty   | History can lag live data; recent history may not be available yet.                                  | Request an earlier window.                                                                    |
| `credit budget truncated` history warning            | The credit budget ran out partway through a range request.                                           | Raise `credit_budget` or narrow the window.                                                   |

## Limitations

- Historical book snapshots and automatic regional REST failover are not supported.
- Sandbox access is not verified.
- Size precision comes only from CoinAPI metadata and has no override.
- Automatic instrument updates, status events, metrics, and options are not supported.
- The adapter loads downloaded flat files but does not list or download them from S3.

## Contributing

:::info
To contribute to the CoinAPI adapter, see the
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
