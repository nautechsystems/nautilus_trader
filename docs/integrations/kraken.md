# Kraken

Founded in 2011, Kraken is one of the most established cryptocurrency exchanges
globally and the largest exchange in Europe by euro trading volume. The platform
offers spot and derivatives trading across a wide range of digital assets. This
integration connects to Kraken Pro and supports live market data ingest and order
execution for both Kraken Spot and Kraken Derivatives (Futures) markets.

## Overview

This adapter is implemented in Rust with Python bindings for ease of use in
Python-based workflows. It does not require external Kraken client libraries—the
core components are compiled as a static library and linked automatically during
the build.

This guide assumes a trader is setting up for both live market data feeds and
trade execution. The Kraken adapter includes multiple components, which can be
used together or separately depending on the use case.

- `KrakenRawHttpClient`: Low-level HTTP API connectivity for Spot and Futures.
- `KrakenHttpClient`: Higher-level HTTP client with instrument caching and reconciliation support.
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

## Kraken documentation

Kraken provides extensive documentation for users:

- [Kraken API Documentation](https://docs.kraken.com/api/)
- [Kraken Spot REST API](https://docs.kraken.com/api/docs/guides/spot-rest-intro)
- [Kraken Futures Documentation](https://support.kraken.com/hc/en-us/sections/360012894412-Futures-API)

Refer to the Kraken documentation in conjunction with this NautilusTrader
integration guide.

## Products

Kraken supports two primary product categories:

| Product Type        | Supported | Notes                                                |
|---------------------|-----------|------------------------------------------------------|
| Spot                | ✓         | Standard cryptocurrency pairs with margin support.   |
| Futures (Perpetual) | ✓         | Inverse and USD-margined perpetual swaps.            |

## Symbology

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
```

:::note
Kraken's native API uses different asset codes (e.g., `XBT` for Bitcoin,
`XETHZUSD` for ETH/USD). The adapter translates between NautilusTrader's
standardized format and Kraken's native format automatically.
:::

### Futures markets

Kraken Futures instruments use a specific naming convention with prefixes:

- `PI_` - Perpetual Inverse contracts (e.g., `PI_XBTUSD`)
- `PF_` - Perpetual Fixed-margin contracts (e.g., `PF_XBTUSD`)
- `FI_` - Fixed maturity Inverse contracts (e.g., `FI_XBTUSD_230929`)

**Instrument ID format:**

```python
InstrumentId.from_str("PI_XBTUSD.KRAKEN")  # Perpetual inverse BTC
InstrumentId.from_str("PI_ETHUSD.KRAKEN")  # Perpetual inverse ETH
InstrumentId.from_str("PF_XBTUSD.KRAKEN")  # Perpetual fixed-margin BTC
```

## Orders capability

### Spot

#### Order types

| Order Type             | Spot | Notes                                            |
|------------------------|------|--------------------------------------------------|
| `MARKET`               | ✓    | Immediate execution at market price.             |
| `LIMIT`                | ✓    | Execution at specified price or better.          |
| `STOP_MARKET`          | ✓    | Conditional market order (stop-loss).            |
| `MARKET_IF_TOUCHED`    | ✓    | Conditional market order (take-profit).          |
| `STOP_LIMIT`           | ✓    | Conditional limit order (stop-loss-limit).       |
| `LIMIT_IF_TOUCHED`     | ✓    | Conditional limit order (take-profit-limit).     |
| `SETTLE_POSITION`      | ✓    | Market order to close entire position.           |

#### Time in force

| Time in Force | Spot | Notes                        |
|---------------|------|------------------------------|
| `GTC`         | ✓    | Good Till Canceled.          |
| `GTD`         | ✓    | Good Till Date.              |
| `IOC`         | ✓    | Immediate or Cancel.         |
| `FOK`         | -    | *Not currently supported*.   |

#### Execution instructions

| Instruction   | Spot | Notes                              |
|---------------|------|------------------------------------|
| `post_only`   | ✓    | Available through order flags.     |
| `reduce_only` | -    | *Not supported for Spot markets*.  |

### Futures

#### Order types

| Order Type             | Futures | Notes                                            |
|------------------------|---------|--------------------------------------------------|
| `MARKET`               | ✓       | Immediate execution at market price.             |
| `LIMIT`                | ✓       | Execution at specified price or better.          |
| `STOP_MARKET`          | ✓       | Conditional market order (stop).                 |
| `MARKET_IF_TOUCHED`    | ✓       | Conditional market order (take-profit).          |
| `STOP_LIMIT`           | ✓       | Conditional limit order (stop-loss).             |

#### Time in force

| Time in Force | Futures | Notes                        |
|---------------|---------|------------------------------|
| `GTC`         | ✓       | Good Till Canceled.          |
| `GTD`         | -       | *Not supported*.             |
| `IOC`         | ✓       | Immediate or Cancel.         |

#### Execution instructions

| Instruction   | Futures | Notes                              |
|---------------|---------|---------------------------------------|
| `post_only`   | ✓       | Available for limit orders.           |
| `reduce_only` | ✓       | Reduces position only, no reversals.  |

## Reconciliation

The Kraken adapter provides comprehensive reconciliation capabilities for both
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

## Rate limiting

The adapter implements automatic rate limiting to comply with Kraken's API
requirements:

| Endpoint Type         | Limit (requests/sec) | Notes                                                |
|-----------------------|----------------------|------------------------------------------------------|
| Spot REST (global)    | 1 per second         | Conservative rate for Spot API calls.                |
| Futures REST (global) | 5 per second         | Higher rate limit for Futures API calls.             |

:::warning
Kraken may temporarily block IP addresses that exceed rate limits. The adapter
automatically queues requests when limits are approached.
:::

## Configuration

The product types for each client must be specified in the configurations.

### Data client configuration options

| Option              | Default   | Description                                                             |
|---------------------|-----------|-------------------------------------------------------------------------|
| `api_key`           | `None`    | API key; loaded from `KRAKEN_API_KEY` when omitted.                     |
| `api_secret`        | `None`    | API secret; loaded from `KRAKEN_API_SECRET` when omitted.               |
| `environment`       | `mainnet` | Trading environment (`mainnet` or `testnet`); testnet only for Futures. |
| `product_types`     | `(SPOT,)` | Product types tuple (e.g., `(KrakenProductType.SPOT,)`).                |
| `base_url_http`     | `None`    | Override for the REST base URL.                                         |
| `timeout_secs`      | `60`      | HTTP request timeout in seconds.                                        |
| `max_retries`       | `3`       | Maximum retry attempts for REST requests.                               |
| `retry_delay_ms`    | `1000`    | Initial delay (milliseconds) between retries.                           |
| `retry_delay_max_ms`| `10000`   | Maximum delay (milliseconds) between retries.                           |

### Execution client configuration options

| Option              | Default   | Description                                                             |
|---------------------|-----------|-------------------------------------------------------------------------|
| `api_key`           | `None`    | API key; loaded from `KRAKEN_API_KEY` when omitted.                     |
| `api_secret`        | `None`    | API secret; loaded from `KRAKEN_API_SECRET` when omitted.               |
| `environment`       | `mainnet` | Trading environment (`mainnet` or `testnet`); testnet only for Futures. |
| `product_types`     | `(SPOT,)` | Product types tuple (e.g., `(KrakenProductType.SPOT,)`).                |
| `base_url_http`     | `None`    | Override for the REST base URL.                                         |
| `timeout_secs`      | `60`      | HTTP request timeout in seconds.                                        |
| `max_retries`       | `3`       | Maximum retry attempts for order submission/cancel calls.               |
| `retry_delay_ms`    | `1000`    | Initial delay (milliseconds) between retries.                           |
| `retry_delay_max_ms`| `10000`   | Maximum delay (milliseconds) between retries.                           |

### Demo environment setup

To test with Kraken Futures demo (paper trading):

1. Sign up at [https://demo-futures.kraken.com](https://demo-futures.kraken.com)
   and generate API credentials.
2. Set environment variables with your demo credentials:
   - `KRAKEN_TESTNET_API_KEY`
   - `KRAKEN_TESTNET_API_SECRET`
3. Configure the adapter with `environment=KrakenEnvironment.TESTNET` and
   `product_types=(KrakenProductType.FUTURES,)`.

```python
from nautilus_trader.adapters.kraken import KRAKEN
from nautilus_trader.adapters.kraken import KrakenEnvironment
from nautilus_trader.adapters.kraken import KrakenProductType

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        KRAKEN: {
            "environment": KrakenEnvironment.TESTNET,
            "product_types": (KrakenProductType.FUTURES,),
        },
    },
    exec_clients={
        KRAKEN: {
            "environment": KrakenEnvironment.TESTNET,
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
            "api_key": "YOUR_KRAKEN_API_KEY",
            "api_secret": "YOUR_KRAKEN_API_SECRET",
            "environment": KrakenEnvironment.MAINNET,
            "product_types": (KrakenProductType.SPOT,),
        },
    },
    exec_clients={
        KRAKEN: {
            "api_key": "YOUR_KRAKEN_API_KEY",
            "api_secret": "YOUR_KRAKEN_API_SECRET",
            "environment": KrakenEnvironment.MAINNET,
            "product_types": (KrakenProductType.SPOT,),
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

For Kraken live clients, you can set:

- `KRAKEN_API_KEY`
- `KRAKEN_API_SECRET`

For Kraken Futures demo environment clients, you can set:

- `KRAKEN_TESTNET_API_KEY`
- `KRAKEN_TESTNET_API_SECRET`

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

:::info
For additional features or to contribute to the Kraken adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
