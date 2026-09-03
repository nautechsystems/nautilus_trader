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

:::note
**Single product type per client**: Each Kraken data or execution client is
configured for a single `product_type` (`SPOT` or `FUTURES`); a single client
does not span both markets.
:::

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

:::warning
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

| Market  | Symbol Format | Example            | Notes                                       |
| ------- | ------------- | ------------------ | ------------------------------------------- |
| Spot    | `BTC`         | `BTC/USD.KRAKEN`   | Adapter normalizes XBT to BTC at load time. |
| Futures | `XBT`         | `PI_XBTUSD.KRAKEN` | Uses Kraken's native XBT format.            |

:::note
Kraken's REST API can return `XBT` for Bitcoin, while its WebSocket v2 API
requires `BTC`. The adapter normalizes Spot symbols to `BTC` when loading
instruments, whether `XBT` appears as the base currency (for example, `XBT/USD`
to `BTC/USD`) or quote currency (for example, `ETH/XBT` to `ETH/BTC`). Futures
retain Kraken's native `XBT` format.
:::

Kraken also uses `XDG` for Dogecoin in some Spot responses. The adapter
normalizes it to `DOGE`, including in quote currency symbols.

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
| `OrderBookDepth10`  | -    | -       | Use `OrderBookDeltas` with depth `10`.   |
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

- With no side filter, Spot cancels all open orders across all symbols, while
  Futures cancels all orders for the requested instrument.
- With a side filter, both clients select matching cached orders for the
  requested instrument and cancel them individually.

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

### Spot reconciliation

**Order status reports:**

- Open orders: Fetches all currently active orders.
- Closed orders: Fetches historical orders with pagination support.
- Time-bounded queries: Supports filtering by start/end timestamps.

**Fill reports:**

- Trade history: Fetches execution history with pagination.
- Time-bounded queries: Supports filtering by start/end timestamps.
- All fill types: Market, limit, and conditional order fills.

**Margin position reports** (when `spot_account_type=Margin`):

- Open positions: Fetched from `POST /0/private/OpenPositions` and aggregated
  by (pair, side) into `PositionStatusReport` entries.
- Synthetic FLAT cleanup: If the local cache has an open spot margin position
  that no longer appears on the venue (Kraken omits closed positions from
  `OpenPositions`), the adapter emits a synthetic FLAT report on the next
  position-check tick so the engine reconciles to closed.
- Margin balances: `POST /0/private/TradeBalance` is called alongside the
  account-state refresh; used margin populates `MarginBalance.initial`, while
  equity and free margin populate the summary balance (see Spot margin trading).

### Futures reconciliation

**Order status reports:**

- Open orders: Fetches all currently active futures orders.
- Historical orders: Fetches closed and filled orders when `open_only=False`.
- Order events: Full order lifecycle history via `/api/history/v2/orders`
  endpoint.

**Fill reports:**

- Fill history: Fetches all execution reports.
- Time filtering: Client-side filtering by start/end timestamps (parses
  RFC3339 timestamps).
- All fill types: Maker and taker fills with fee information.

**Position status reports:**

- Open positions: Fetches all active futures positions.
- Real-time data: Includes unrealized funding, average price, and position size.

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
on each `position_check_interval_secs` tick. Closed positions on the venue
that still appear open in the local cache are reconciled to FLAT on the next
sweep. This path is independent of `use_spot_position_reports` (which is
wallet-derived, cash-mode-only).

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

| Option                    | Default   | Description                                                    |
| ------------------------- | --------- | -------------------------------------------------------------- |
| `product_type`            | `SPOT`    | Product type for this client (`SPOT` or `FUTURES`).            |
| `environment`             | `LIVE`    | Trading environment (`LIVE` or `DEMO`); demo only for Futures. |
| `api_key`                 | `None`    | API key for authenticated Spot data such as L3.                |
| `api_secret`              | `None`    | API secret for authenticated Spot data such as L3.             |
| `base_url`                | `None`    | Override for the Kraken REST base URL.                         |
| `ws_public_url`           | `None`    | Override for the public WebSocket URL.                         |
| `ws_private_url`          | `None`    | Override for the private WebSocket URL.                        |
| `ws_l3_url`               | `None`    | Override for the Spot L3 WebSocket URL.                        |
| `validate_l3_checksum`    | `True`    | Validate Kraken Spot L3 checksums and resync on mismatch.      |
| `proxy_url`               | `None`    | Optional proxy URL for HTTP and WebSocket transports.          |
| `timeout_secs`            | `30`      | HTTP request timeout in seconds.                               |
| `heartbeat_interval_secs` | `30`      | WebSocket heartbeat interval in seconds.                       |
| `ws_idle_timeout_ms`      | `10,000`  | Data-silence timeout for the Spot v2 WebSocket; `0` disables.  |
| `max_requests_per_second` | `None`    | Per-client request throttle; default is 5 req/s.               |
| `transport_backend`       | `Sockudo` | WebSocket transport backend.                                   |

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
| `spot_account_type`             | `CASH`    | Account type for spot trading; `MARGIN` enables leverage and reports. |
| `default_leverage`              | `None`    | Default spot margin leverage sent as `"N:1"` when set.                |
| `use_spot_position_reports`     | `False`   | Report wallet balances as positions; cash mode only.                  |
| `spot_positions_quote_currency` | `"USDT"`  | Quote currency filter for spot wallet position reports.               |
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
`KrakenExecutionClientConfig` and, for Spot L3 data, to `KrakenDataClientConfig`.
Public market data does not require credentials.

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
