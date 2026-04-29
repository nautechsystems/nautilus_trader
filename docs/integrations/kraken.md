# Kraken

Kraken offers spot and derivatives trading across a wide range of digital
assets. This integration connects to Kraken Pro and supports live market data
ingest and order execution for Kraken Spot and Kraken Derivatives (Futures).

## Overview

This adapter is implemented in Rust with Python bindings for ease of use in
Python-based workflows. It does not require external Kraken client libraries; the
core components are compiled as a static library and linked automatically during
the build.

This guide assumes a trader is setting up for both live market data feeds and
trade execution. The Kraken adapter includes multiple components, which can be
used together or separately depending on the use case.

- `KrakenSpotRawHttpClient` and `KrakenFuturesRawHttpClient`: Low-level HTTP
  API connectivity.
- `KrakenSpotHttpClient` and `KrakenFuturesHttpClient`: Higher-level HTTP
  clients with instrument caching and reconciliation support.
- `KrakenInstrumentProvider`: Instrument parsing and loading functionality.
- `KrakenDataClient`: Market data feed manager.
- `KrakenExecutionClient`: Account management and trade execution gateway.
- `KrakenLiveDataClientFactory`: Factory for Kraken data clients (used by the
  trading node builder).
- `KrakenLiveExecClientFactory`: Factory for Kraken execution clients (used by
  the trading node builder).

:::note
Most users will define a configuration for a live trading node (as below), and
won't need to work directly with these lower-level components.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/kraken/).

## Kraken documentation

Kraken provides detailed documentation for users:

- [Kraken API Documentation](https://docs.kraken.com/api/)
- [Kraken Spot REST API](https://docs.kraken.com/api/docs/guides/spot-rest-intro)
- [Kraken Futures REST API](https://docs.kraken.com/api/docs/futures-api)

Refer to the Kraken documentation in conjunction with this NautilusTrader
integration guide.

## Products

Kraken supports two primary product categories:

| Product Type             | Supported | Notes                                                     |
|--------------------------|-----------|-----------------------------------------------------------|
| Spot                     | ✓         | Standard cryptocurrency pairs with margin support.        |
| Futures (Perpetual)      | ✓         | Inverse (`PI_`) and USD‑margined (`PF_`) perpetual swaps. |
| Futures (Dated/Flex)     | ✓         | Fixed maturity (`FI_`) and flex (`FF_`) contracts.        |

:::note
**Dual-product deployments**: When both `SPOT` and `FUTURES` product types are
configured, the adapter queries both APIs and merges the account states. This
gives the execution engine visibility into collateral across both markets.
:::

## Bar streaming

### Supported intervals

The Kraken adapter supports real-time bar (OHLC) streaming for Spot markets via
WebSocket. The following intervals are available:

| Interval   | BarType specification |
|------------|-----------------------|
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

Kraken's WebSocket OHLC channel pushes updates for the *current* (incomplete)
bar on every trade. Unlike some exchanges (e.g., Binance), Kraken does not
provide an "is_closed" indicator to signal when a bar is complete.

To avoid emitting partial/incomplete bars, the adapter buffers the current bar
and only emits it when the next bar period begins (i.e., when a message with a
new `interval_begin` timestamp arrives). This means:

- Bars are emitted with a delay of up to one bar period.
- For 1-minute bars, the maximum delay is ~1 minute.
- The emitted bar data is complete and final.

We chose this approach over timer-based emission because:

- Timer-based emission could miss the final update before the bar closes.
- Kraken's updates are not guaranteed to arrive at exact interval boundaries.
- Buffering preserves data integrity at the cost of latency.

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

### Bitcoin symbol format (BTC vs XBT)

Kraken uses different Bitcoin symbol conventions across their APIs:

| Market  | Symbol Format | Example            | Notes                                       |
|---------|---------------|--------------------|---------------------------------------------|
| Spot    | `BTC`         | `BTC/USD.KRAKEN`   | Adapter normalizes XBT → BTC at load time.  |
| Futures | `XBT`         | `PI_XBTUSD.KRAKEN` | Uses Kraken's native XBT format.            |

:::note
Kraken's REST API returns `XBT` for Bitcoin (following ISO 4217 conventions for
supranational currencies), but their WebSocket v2 API requires the `BTC` format.
The adapter automatically normalizes spot symbols to `BTC` when loading instruments,
whether XBT appears as the base currency (e.g., `XBT/USD` → `BTC/USD`) or quote
currency (e.g., `ETH/XBT` → `ETH/BTC`). Futures retain Kraken's native `XBT` format.
:::

### Spot markets

NautilusTrader uses ISO 4217-A3 format for Kraken Spot instrument symbols,
which provides a standardized representation across exchanges. The adapter
handles translation to Kraken's native format internally.

**Instrument ID format:**

```python
InstrumentId.from_str("BTC/USD.KRAKEN")   # Spot BTC/USD
InstrumentId.from_str("ETH/USD.KRAKEN")   # Spot ETH/USD
InstrumentId.from_str("SOL/USD.KRAKEN")   # Spot SOL/USD
InstrumentId.from_str("BTC/USDT.KRAKEN")  # Spot BTC/USDT
InstrumentId.from_str("ETH/BTC.KRAKEN")   # Spot ETH/BTC (normalized from ETH/XBT)
```

### Futures markets

Kraken Futures instruments use a specific naming convention with prefixes:

- `PI_` - Perpetual Inverse contracts (e.g., `PI_XBTUSD`)
- `PF_` - Perpetual Fixed-margin contracts (e.g., `PF_XBTUSD`)
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

| Data Type              | Spot | Futures | Notes                                  |
|------------------------|------|---------|----------------------------------------|
| `QuoteTick`            | ✓    | ✓       | Derived from ticker channel.           |
| `TradeTick`            | ✓    | ✓       |                                        |
| `OrderBookDeltas`      | ✓    | ✓       | L2 order book updates.                 |
| `OrderBookDepth10`     | -    | -       | Use `OrderBookDeltas` with depth `10`. |
| `Bar`                  | ✓    | -       | Spot WS OHLC channel. See bar section. |
| `MarkPriceUpdate`      | -    | ✓       | From futures ticker feed.              |
| `IndexPriceUpdate`     | -    | ✓       | From futures ticker feed.              |
| `FundingRateUpdate`    | -    | ✓       | Perpetuals only.                       |
| `InstrumentStatus`     | ✓    | ✓       | Python adapter polls instrument refreshes. |

### Requests (historical)

| Data Type              | Spot | Futures | Notes                                  |
|------------------------|------|---------|----------------------------------------|
| `TradeTick`            | ✓    | ✓       |                                        |
| `Bar`                  | ✓    | ✓       |                                        |
| `OrderBook` (snapshot) | ✓    | ✓       | Via HTTP depth endpoint.               |
| `FundingRateUpdate`    | -    | ✓       | Client‑side start/end/limit filtering. |

## Orders capability

### Order types

| Order Type             | Spot | Futures | Notes                                         |
|------------------------|------|---------|-----------------------------------------------|
| `MARKET`               | ✓    | ✓       | Immediate execution at market price.          |
| `LIMIT`                | ✓    | ✓       | Execution at specified price or better.       |
| `STOP_MARKET`          | ✓    | ✓       | Conditional market order (stop‑loss).         |
| `MARKET_IF_TOUCHED`    | ✓    | ✓       | Conditional market order (take‑profit).       |
| `STOP_LIMIT`           | ✓    | ✓       | Conditional limit order (stop‑loss‑limit).    |
| `LIMIT_IF_TOUCHED`     | ✓    | ✓       | Maps to `take_profit` with `limit_price`.     |
| `TRAILING_STOP_MARKET` | ✓    | -       | Trailing stop with `trailing_offset`.         |
| `TRAILING_STOP_LIMIT`  | ✓    | -       | Trailing stop‑limit with `limit_offset`.      |

### Time in force

| Time in Force | Spot | Futures | Notes                                               |
|---------------|------|---------|-----------------------------------------------------|
| `GTC`         | ✓    | ✓       | Good Till Canceled.                                 |
| `GTD`         | ✓    | -       | Good Till Date (Spot only, requires `expire_time`). |
| `IOC`         | ✓    | ✓       | Immediate or Cancel.                                |
| `FOK`         | ✓    | -       | Spot limit orders only.                             |

:::note
**Market orders** are inherently immediate and do not support time-in-force.
`IOC` only applies to limit-type orders.
:::

### Execution instructions

| Instruction      | Spot | Futures | Notes                                         |
|------------------|------|---------|-----------------------------------------------|
| `post_only`      | ✓    | ✓       | Available for limit orders.                   |
| `reduce_only`    | -    | ✓       | Futures only. Reduces position, no reversal.  |
| `quote_quantity` | ✓    | -       | Spot only. Volume in quote currency (`viqc`). |
| `display_qty`    | ✓    | -       | Spot only. Iceberg orders (`displayvol`).     |

### Trigger types

Conditional orders (stop, take-profit, trailing stop) support a trigger price
reference on Spot:

| Trigger Type  | Spot | Futures | Notes                                      |
|---------------|------|---------|--------------------------------------------|
| `LAST_PRICE`  | ✓    | ✓       | Default. Last traded price.                |
| `INDEX_PRICE` | ✓    | ✓       | Broader market index price.                |
| `MARK_PRICE`  | -    | ✓       | Futures only.                              |

:::note
The adapter rejects unsupported trigger types (e.g., `BID_ASK`) at submission
time rather than silently coercing them.
:::

### Batch operations

| Operation          | Spot | Futures | Notes                                        |
|--------------------|------|---------|----------------------------------------------|
| Batch Submit       | ✓    | ✓       | Spot chunks at 15 orders. Futures chunks at 10. |
| Batch Modify       | -    | ✓       | Futures HTTP helper only. Execution uses single modify commands. |
| Batch Cancel       | ✓    | ✓       | Auto‑chunks into batches of 50.              |

:::note
**Cancel all orders**:

- Order side filtering is not supported; all orders are canceled regardless of side.
- Spot: Cancels all open orders across all symbols.
- Futures: Requires an `instrument_id`; cancels orders for that symbol only.

:::

### Position management

| Feature           | Spot | Futures | Notes                                                     |
|-------------------|------|---------|-----------------------------------------------------------|
| Query positions   | ✓*   | ✓       | *Spot: opt‑in via `use_spot_position_reports`. See below. |
| Position mode     | -    | -       | Single position per instrument.                           |
| Leverage control  | -    | ✓       | Configured per account tier.                              |
| Margin mode       | -    | ✓       | Cross margin for Futures.                                 |

### Order querying

| Feature              | Spot | Futures | Notes                                        |
|----------------------|------|---------|----------------------------------------------|
| Query open orders    | ✓    | ✓       | List all active orders.                      |
| Query order history  | ✓    | ✓       | Historical order data with pagination.       |
| Order status updates | ✓    | ✓       | Real‑time order state changes via WebSocket. |
| Trade history        | ✓    | ✓       | Execution and fill reports.                  |

### Contingent orders

| Feature             | Spot | Futures | Notes                                    |
|---------------------|------|---------|------------------------------------------|
| Order lists         | -    | -       | *Not supported*.                         |
| OCO orders          | -    | -       | *Not supported*.                         |
| Bracket orders      | -    | -       | *Not supported*.                         |
| Conditional orders  | ✓    | ✓       | Stop and take‑profit orders.             |

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

### Spot position reports

The Kraken adapter can optionally report wallet balances as position status
reports for spot instruments. This feature is disabled by default and must be
explicitly enabled via configuration.

**How it works:**

- When enabled, wallet balances are converted to `PositionStatusReport` objects.
- Positive balances are reported as `LONG` positions.
- Only instruments matching the configured quote currency are reported (default: `USDT`).
- This prevents duplicate reports when the same asset is available with multiple quote currencies (e.g., BTC/USD, BTC/USDT, BTC/EUR).

**Configuration:**

```python
exec_clients={
    KRAKEN: {
        "use_spot_position_reports": True,
        "spot_positions_quote_currency": "USDT",  # Default
    },
}
```

:::warning
**Use with caution**: Enabling spot position reports may lead to unintended
behavior if your strategy is not designed to handle spot positions. For example,
a strategy that expects to close positions may attempt to sell your wallet
holdings.
:::

## Funding rates

The adapter receives funding rate data from the
[Ticker](https://docs.kraken.com/api/docs/futures-api/websocket/ticker)
WebSocket feed, which provides `relative_funding_rate` and `next_funding_rate_time` for
perpetual futures.

The `interval` field on `FundingRateUpdate` is `None` for Kraken because the ticker feed
does not include a funding interval field and the Kraken API documentation does not
specify a fixed funding period.

## Rate limiting

The adapter implements automatic rate limiting to comply with Kraken's API requirements.

| Endpoint Type         | Limit (requests/sec) | Notes                                |
|-----------------------|----------------------|--------------------------------------|
| Spot REST (global)    | 5                    | Global rate limit for Spot API.      |
| Futures REST (global) | 5                    | Global rate limit for Futures API.   |

:::info
Kraken uses a counter-based rate limiting system with tier-dependent limits:

- **Starter tier**: 15 max counter, -0.33/sec decay
- **Intermediate tier**: 20 max counter, -0.5/sec decay
- **Pro tier**: 20 max counter, -1/sec decay

Ledger/trade history calls add +2 to the counter; other calls add +1.
:::

:::warning
Kraken may temporarily block IP addresses that exceed rate limits. The adapter
automatically queues requests when limits are approached.
:::

### Reconciliation interval guidance

The execution engine's `open_check_interval_secs` and
`position_check_interval_secs` settings create sustained REST API load that
can exhaust Kraken's counter-based rate limit, especially on the Starter tier
where the counter decays at only 0.33/sec. Each open-order check generates
1-3 REST calls (+1 or +2 counter each), and at short intervals the counter
overflows before it can decay, causing `EAPI:Rate limit exceeded` errors.

Recommended settings for Kraken:

```python
exec_engine=LiveExecEngineConfig(
    reconciliation=True,
    open_check_interval_secs=30.0,    # 30s minimum for Starter tier
    position_check_interval_secs=120.0,  # 2 minutes
)
```

Higher-tier accounts with faster counter decay can use shorter intervals.
If you see `EAPI:Rate limit exceeded` errors in the logs, increase these
intervals or reduce `max_requests_per_second` in the adapter config.

## Configuration

The product types for each client must be specified in the configurations.

### Data client configuration options

| Option                          | Default   | Description                                                             |
|---------------------------------|-----------|-------------------------------------------------------------------------|
| `api_key`                       | `None`    | API key; loaded from environment variables (see below) when omitted.    |
| `api_secret`                    | `None`    | API secret; loaded from environment variables (see below) when omitted. |
| `environment`                   | `mainnet` | Trading environment (`mainnet` or `demo`); demo only for Futures.       |
| `product_types`                 | `(SPOT,)` | Product types tuple (e.g., `(KrakenProductType.SPOT,)`).                |
| `base_url_http_spot`            | `None`    | Override for Kraken Spot REST base URL.                                 |
| `base_url_http_futures`         | `None`    | Override for Kraken Futures REST base URL.                              |
| `base_url_ws_spot`              | `None`    | Override for Kraken Spot WebSocket URL.                                 |
| `base_url_ws_futures`           | `None`    | Override for Kraken Futures WebSocket URL.                              |
| `proxy_url`                     | `None`    | Optional proxy URL for HTTP and WebSocket transports.                   |
| `update_instruments_interval_mins` | `60`   | Interval (minutes) to reload instruments; `None` to disable.            |
| `max_retries`                   | `None`    | Maximum retry attempts for REST requests.                               |
| `retry_delay_initial_ms`        | `None`    | Initial delay (milliseconds) between retries.                           |
| `retry_delay_max_ms`            | `None`    | Maximum delay (milliseconds) between retries.                           |
| `http_timeout_secs`             | `None`    | HTTP request timeout in seconds.                                        |
| `ws_heartbeat_secs`             | `30`      | WebSocket heartbeat interval in seconds.                                |
| `max_requests_per_second`       | `None`    | Override rate limit (default 5 req/s); for higher tier accounts.        |

### Execution client configuration options

| Option                          | Default   | Description                                                             |
|---------------------------------|-----------|-------------------------------------------------------------------------|
| `api_key`                       | `None`    | API key; loaded from environment variables (see below) when omitted.    |
| `api_secret`                    | `None`    | API secret; loaded from environment variables (see below) when omitted. |
| `environment`                   | `mainnet` | Trading environment (`mainnet` or `demo`); demo only for Futures.       |
| `product_types`                 | `(SPOT,)` | Product types tuple; `SPOT` uses CASH, `FUTURES` uses MARGIN account.   |
| `base_url_http_spot`            | `None`    | Override for Kraken Spot REST base URL.                                 |
| `base_url_http_futures`         | `None`    | Override for Kraken Futures REST base URL.                              |
| `base_url_ws_spot`              | `None`    | Override for Kraken Spot WebSocket URL.                                 |
| `base_url_ws_futures`           | `None`    | Override for Kraken Futures WebSocket URL.                              |
| `proxy_url`                     | `None`    | Optional proxy URL for HTTP and WebSocket transports.                   |
| `max_retries`                   | `None`    | Maximum retry attempts for order submission/cancel calls.               |
| `retry_delay_initial_ms`        | `None`    | Initial delay (milliseconds) between retries.                           |
| `retry_delay_max_ms`            | `None`    | Maximum delay (milliseconds) between retries.                           |
| `http_timeout_secs`             | `None`    | HTTP request timeout in seconds.                                        |
| `ws_heartbeat_secs`             | `30`      | WebSocket heartbeat interval in seconds.                                |
| `max_requests_per_second`       | `None`    | Override rate limit (default 5 req/s); for higher tier accounts.        |
| `use_spot_position_reports`     | `False`   | Report wallet balances as positions (see below).                        |
| `spot_positions_quote_currency` | `"USDT"`  | Quote currency filter for spot position reports.                        |

### Demo environment setup

To test with Kraken Futures demo (paper trading):

1. Sign up at [https://demo-futures.kraken.com](https://demo-futures.kraken.com)
   and generate API credentials.
2. Set environment variables with your demo credentials:
   - `KRAKEN_FUTURES_DEMO_API_KEY`
   - `KRAKEN_FUTURES_DEMO_API_SECRET`
3. Configure the adapter with `environment=KrakenEnvironment.DEMO` and
   `product_types=(KrakenProductType.FUTURES,)`.

```python
from nautilus_trader.adapters.kraken import KRAKEN
from nautilus_trader.adapters.kraken import KrakenEnvironment
from nautilus_trader.adapters.kraken import KrakenProductType

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        KRAKEN: {
            "environment": KrakenEnvironment.DEMO,
            "product_types": (KrakenProductType.FUTURES,),
        },
    },
    exec_clients={
        KRAKEN: {
            "environment": KrakenEnvironment.DEMO,
            "product_types": (KrakenProductType.FUTURES,),
        },
    },
)
```

### Production configuration

The most common use case is to configure a live `TradingNode` to include Kraken
data and execution clients. Add a `KRAKEN` section to your client
configuration(s):

```python
from nautilus_trader.adapters.kraken import KRAKEN
from nautilus_trader.adapters.kraken import KrakenEnvironment
from nautilus_trader.adapters.kraken import KrakenProductType
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        KRAKEN: {
            "environment": KrakenEnvironment.MAINNET,
            "product_types": (KrakenProductType.SPOT,),
        },
    },
    exec_clients={
        KRAKEN: {
            "environment": KrakenEnvironment.MAINNET,
            "product_types": (KrakenProductType.SPOT,),
        },
    },
)
```

### Dual-product configuration (Spot + Futures)

When trading both Spot and Futures markets, include both product types:

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        KRAKEN: {
            "environment": KrakenEnvironment.MAINNET,
            "product_types": (KrakenProductType.SPOT, KrakenProductType.FUTURES),
        },
    },
    exec_clients={
        KRAKEN: {
            "environment": KrakenEnvironment.MAINNET,
            "product_types": (KrakenProductType.SPOT, KrakenProductType.FUTURES),
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.kraken import KRAKEN
from nautilus_trader.adapters.kraken import KrakenLiveDataClientFactory
from nautilus_trader.adapters.kraken import KrakenLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory(KRAKEN, KrakenLiveDataClientFactory)
node.add_exec_client_factory(KRAKEN, KrakenLiveExecClientFactory)

# Finally build the node
node.build()
```

### API credentials

There are two options for supplying your credentials to the Kraken clients.
Either pass the corresponding `api_key` and `api_secret` values to the
configuration objects, or set the following environment variables:

| Environment Variable             | Description                              |
|----------------------------------|------------------------------------------|
| `KRAKEN_SPOT_API_KEY`            | API key for Kraken Spot (mainnet).       |
| `KRAKEN_SPOT_API_SECRET`         | API secret for Kraken Spot (mainnet).    |
| `KRAKEN_FUTURES_API_KEY`         | API key for Kraken Futures (mainnet).    |
| `KRAKEN_FUTURES_API_SECRET`      | API secret for Kraken Futures (mainnet). |
| `KRAKEN_FUTURES_DEMO_API_KEY`    | API key for Kraken Futures (demo).       |
| `KRAKEN_FUTURES_DEMO_API_SECRET` | API secret for Kraken Futures (demo).    |

:::note
**Demo environment**: Only Kraken Futures offers a demo environment
(`https://demo-futures.kraken.com`) for testing without real funds. Kraken Spot
does not have a testnet - the `environment` setting only affects Futures
connections.
:::

:::tip
We recommend using environment variables to manage your credentials.
:::

When starting the trading node, you'll receive immediate confirmation of whether
your credentials are valid and have trading permissions.

## Contributing

:::info
For additional features or to contribute to the Kraken adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
