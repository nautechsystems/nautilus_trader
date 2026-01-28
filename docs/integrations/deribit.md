# Deribit

Founded in 2016, Deribit is a cryptocurrency derivatives exchange specializing in Bitcoin and
Ethereum options and futures. It is one of the largest crypto options exchanges by volume,
and a leading platform for crypto derivatives trading.

This integration supports live market data ingest and order execution for:

- **Perpetual Futures** (e.g., BTC-PERPETUAL, ETH-PERPETUAL)
- **Dated Futures** (e.g., BTC-28MAR25, ETH-27JUN25)
- **Options** (e.g., BTC-28MAR25-100000-C)
- **Spot** (e.g., BTC_USDC, ETH_USDC)
- **Combo Instruments** (e.g., future spreads, option spreads)

## Overview

This adapter is implemented in Rust, with optional Python bindings for use in Python-based workflows.
Deribit uses JSON-RPC 2.0 over both HTTP and WebSocket transports (rather than REST).
WebSocket is preferred for subscriptions and real-time data.

The official Deribit API reference can be found at <https://docs.deribit.com/v2/>.

The Deribit adapter includes multiple components, which can be used together or separately depending
on your use case:

- `DeribitHttpClient`: Low-level HTTP API connectivity (JSON-RPC over HTTP).
- `DeribitWebSocketClient`: Low-level WebSocket API connectivity (JSON-RPC over WebSocket).
- `DeribitInstrumentProvider`: Instrument parsing and loading functionality.
- `DeribitDataClient`: Market data feed manager.
- `DeribitExecutionClient`: Account management and trade execution gateway.
- `DeribitLiveDataClientFactory`: Factory for Deribit data clients (used by the trading node builder).
- `DeribitLiveExecClientFactory`: Factory for Deribit execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as shown below),
and won't need to work directly with these lower-level components.
:::

### Product support

| Product Type     | Data Feed | Trading | Notes                                         |
|------------------|-----------|---------|-----------------------------------------------|
| Perpetual Futures| ✓         | ✓       | BTC-PERPETUAL, ETH-PERPETUAL                  |
| Dated Futures    | ✓         | ✓       | Quarterly expiration contracts                |
| Options          | ✓         | ✓       | BTC and ETH options                           |
| Spot             | ✓         | ✓       | BTC_USDC, ETH_USDC pairs                      |
| Future Combos    | ✓         | ✓       | Calendar spreads for futures                  |
| Option Combos    | ✓         | ✓       | Option spread strategies                      |

## Symbology

Deribit uses specific symbol conventions for different instrument types. All instrument IDs should include the `.DERIBIT` suffix when referencing them (e.g., `BTC-PERPETUAL.DERIBIT` for BTC perpetual).

### Perpetual Futures

Format: `{Currency}-PERPETUAL`

Examples:

- `BTC-PERPETUAL` - Bitcoin perpetual swap
- `ETH-PERPETUAL` - Ethereum perpetual swap

To subscribe to BTC perpetual in your strategy:

```python
InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
```

### Dated Futures

Format: `{Currency}-{DDMMMYY}`

Examples:

- `BTC-28MAR25` - Bitcoin futures expiring March 28, 2025
- `ETH-27JUN25` - Ethereum futures expiring June 27, 2025

```python
InstrumentId.from_str("BTC-28MAR25.DERIBIT")
```

### Options

Format: `{Currency}-{DDMMMYY}-{Strike}-{Type}`

Examples:

- `BTC-28MAR25-100000-C` - Bitcoin call option, $100,000 strike, expiring March 28, 2025
- `BTC-28MAR25-80000-P` - Bitcoin put option, $80,000 strike, expiring March 28, 2025
- `ETH-28MAR25-4000-C` - Ethereum call option, $4,000 strike

Where:

- `C` = Call option
- `P` = Put option

```python
InstrumentId.from_str("BTC-28MAR25-100000-C.DERIBIT")
```

### Spot

Format: `{BaseCurrency}_{QuoteCurrency}`

Examples:

- `BTC_USDC` - Bitcoin against USDC
- `ETH_USDC` - Ethereum against USDC

```python
InstrumentId.from_str("BTC_USDC.DERIBIT")
```

## Order book subscriptions

Deribit provides two types of order book feeds, each suited for different use cases.

### Raw feeds (tick-by-tick)

Raw channels deliver every single update as an individual message. Subscribing to a raw order book
gives you a notification for every order insertion, update, or deletion in the book.

- **Requires authenticated connection** (safeguard against abuse)
- Use when you need every price level change for HFT or market making
- Higher message volume

### Aggregated feeds (batched)

Aggregated channels deliver updates in batches at a fixed interval (e.g., every 100ms).
This groups multiple order book changes into single messages.

- Available without authentication
- Recommended for most use cases
- Lower message volume, easier to process
- Default interval: 100ms

### Subscription parameters

The Nautilus adapter supports both feed types via subscription parameters:

| Parameter | Values | Notes |
|-----------|--------|-------|
| `interval` | `raw`, `100ms`, `agg2` | Default: `100ms`. Use `raw` for tick-by-tick (requires auth) |
| `depth` | `1`, `10`, `20` | Default: `10`. Number of price levels per side |

```python
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")

# Default: 100ms aggregated feed (no authentication required)
strategy.subscribe_order_book_deltas(instrument_id)

# Raw feed: Pass interval parameter (requires API credentials)
strategy.subscribe_order_book_deltas(
    instrument_id,
    params={"interval": "raw"},
)
```

:::note
Raw order book feeds require an authenticated WebSocket connection. Ensure API credentials are
configured before subscribing to raw feeds.
:::

:::tip
For most strategies, the default 100ms aggregated feed provides sufficient granularity with
significantly lower message overhead. Only use raw feeds when tick-by-tick precision is essential.
:::

## Orders capability

Below are the order types, execution instructions, and time-in-force options supported on Deribit.

### Order types

| Order Type    | Supported | Notes                                    |
|---------------|-----------|------------------------------------------|
| `MARKET`      | ✓         | Immediate execution at market price.     |
| `LIMIT`       | ✓         | Execution at specified price or better.  |
| `STOP_MARKET` | ✓         | Conditional market order on trigger.     |
| `STOP_LIMIT`  | ✓         | Conditional limit order on trigger.      |

### Execution instructions

| Instruction    | Supported | Notes                                           |
|----------------|-----------|------------------------------------------------|
| `post_only`    | ✓         | Order will be rejected if it would take liquidity. Uses `reject_post_only=true`. |
| `reduce_only`  | ✓         | Order can only reduce an existing position.     |

### Time in force

| Time in force | Supported | Notes                                               |
|---------------|-----------|-----------------------------------------------------|
| `GTC`         | ✓         | Good Till Canceled (`good_til_cancelled`).          |
| `GTD`         | ✓         | Good Till Day - expires at 8:00 UTC (`good_til_day`). |
| `IOC`         | ✓         | Immediate or Cancel (`immediate_or_cancel`).        |
| `FOK`         | ✓         | Fill or Kill (`fill_or_kill`).                      |

:::note
**GTD on Deribit**: Unlike other exchanges where GTD accepts an arbitrary expiry time, Deribit's `good_til_day` always expires at 8:00 UTC the same or next day. Custom expiry times will be logged as warnings and the order will use the exchange's fixed expiry behavior.
:::

### Trigger types

Conditional orders (stop orders) support different trigger price sources:

| Trigger Type  | Supported | Notes                                    |
|---------------|-----------|------------------------------------------|
| `last_price`  | ✓         | Uses the last traded price (default).    |
| `mark_price`  | ✓         | Uses the mark price.                     |
| `index_price` | ✓         | Uses the underlying index price.         |

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

| Operation     | Supported | Notes                                      |
|---------------|-----------|--------------------------------------------|
| Batch Submit  | -         | *Not yet implemented*.                     |
| Batch Modify  | -         | *Not yet implemented*.                     |
| Batch Cancel  | -         | *Not yet implemented*.                     |

### Position management

| Feature           | Supported | Notes                                     |
|-------------------|-----------|-------------------------------------------|
| Query positions   | ✓         | Real-time position updates.               |
| Position mode     | -         | Deribit uses net position mode only.      |
| Leverage control  | -         | Leverage set at account level via UI.     |
| Margin mode       | -         | Portfolio margin via Deribit UI settings. |

### Order querying

| Feature              | Supported | Notes                              |
|----------------------|-----------|------------------------------------|
| Query open orders    | ✓         | List all active orders.            |
| Query order history  | ✓         | Historical order data.             |
| Order status updates | ✓         | Real-time order state changes.     |
| Trade history        | ✓         | Execution and fill reports.        |

### Contingent orders

| Feature             | Supported | Notes                              |
|---------------------|-----------|------------------------------------|
| Order lists         | -         | *Not supported*.                   |
| OCO orders          | -         | *Not supported*.                   |
| Bracket orders      | -         | *Not supported*.                   |
| Conditional orders  | ✓         | Stop market and stop limit orders. |

## Rate limiting

Deribit uses a credit-based rate limiting system. Each API request consumes credits, which are replenished
over time. The adapter enforces these quotas to prevent rate limit violations.

### REST limits

| Bucket / Key        | Limit            | Notes                                       |
|---------------------|------------------|---------------------------------------------|
| `deribit:global`    | 20 req/sec (100 burst) | Default bucket for all REST requests. |
| `deribit:orders`    | 5 req/sec (20 burst)   | Matching engine operations (buy, sell, edit, cancel). |
| `deribit:account`   | 5 req/sec              | Account information endpoints.        |

### WebSocket limits

| Operation           | Limit            | Notes                                       |
|---------------------|------------------|---------------------------------------------|
| Subscribe/unsubscribe | 3 req/sec (10 burst) | Subscription operations.              |
| Order operations    | 5 req/sec (20 burst)  | Buy, sell, edit, cancel via WebSocket. |

:::info
**Credit-based system**: Deribit's rate limiting uses credits rather than simple request counts:

- Non-matching requests: 500 credits each, max 50,000 credits, refill 10,000/sec (~20 sustained req/s)
- Matching engine requests: Separate limits that vary by account tier based on 7-day trading volume

For more details, see the official documentation: <https://docs.deribit.com/#rate-limits>
:::

:::warning
Deribit returns error code `10028` (too_many_requests) when you exceed the allowed quota.
Repeated violations may result in temporary throttling.
:::

## Authentication

Deribit uses API key authentication with HMAC-SHA256 signatures for private endpoints.

To create API credentials:

1. Log into your Deribit account at [deribit.com](https://www.deribit.com) (or [test.deribit.com](https://test.deribit.com) for testnet).
2. Navigate to **Account** → **API**.
3. Click **Add new key** and configure permissions:
   - Enable **read** for market data access
   - Enable **trade** for order execution
   - Enable **wallet** if you need account balance access
4. Note down your **Client ID** (API key) and **Client Secret** (API secret).

:::warning
Keep your API secret secure. Never share it or commit it to version control.
:::

## Testnet

Deribit provides a testnet environment for testing strategies without real funds.
To use the testnet, set `is_testnet=True` in your client configuration:

```python
config = TradingNodeConfig(
    data_clients={
        DERIBIT: DeribitDataClientConfig(
            is_testnet=True,  # Enable testnet mode
            # ... other config
        ),
    },
    exec_clients={
        DERIBIT: DeribitExecClientConfig(
            is_testnet=True,  # Enable testnet mode
            # ... other config
        ),
    },
)
```

When testnet mode is enabled:

- HTTP requests use `https://test.deribit.com`
- WebSocket connections use `wss://test.deribit.com/ws/api/v2`
- Credentials are loaded from `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET` environment variables

:::note
Testnet API keys are separate from production keys. You must create API keys specifically for the testnet through the testnet interface at [test.deribit.com](https://test.deribit.com).
:::

## Configuration

### Data client configuration options

| Option                             | Default    | Description |
|------------------------------------|------------|-------------|
| `api_key`                          | `None`     | Deribit API key; loaded from environment variables when omitted. |
| `api_secret`                       | `None`     | Deribit API secret; loaded from environment variables when omitted. |
| `instrument_kinds`                 | `None`     | Instrument kinds to load (Future, Option, Spot, etc.). If `None`, loads all kinds. |
| `base_url_http`                    | `None`     | Override for the HTTP REST base URL. |
| `base_url_ws`                      | `None`     | Override for the WebSocket base URL. |
| `is_testnet`                       | `False`    | Use Deribit testnet endpoints when `True`. |
| `http_timeout_secs`                | `60`       | Request timeout (seconds) for REST calls. |
| `max_retries`                      | `3`        | Maximum retry attempts for recoverable errors. |
| `retry_delay_initial_ms`           | `1,000`    | Initial delay (milliseconds) before retrying. |
| `retry_delay_max_ms`               | `10,000`   | Maximum delay (milliseconds) between retries. |
| `update_instruments_interval_mins` | `60`       | Interval (minutes) between instrument refreshes. |

### Execution client configuration options

| Option                   | Default    | Description |
|--------------------------|------------|-------------|
| `api_key`                | `None`     | Deribit API key; loaded from environment variables when omitted. |
| `api_secret`             | `None`     | Deribit API secret; loaded from environment variables when omitted. |
| `instrument_kinds`       | `None`     | Instrument kinds to load (Future, Option, Spot, etc.). If `None`, defaults to Future. |
| `base_url_http`          | `None`     | Override for the HTTP REST base URL. |
| `base_url_ws`            | `None`     | Override for the WebSocket base URL. |
| `is_testnet`             | `False`    | Use Deribit testnet endpoints when `True`. |
| `http_timeout_secs`      | `60`       | Request timeout (seconds) for REST calls. |
| `max_retries`            | `3`        | Maximum retry attempts for recoverable errors. |
| `retry_delay_initial_ms` | `1,000`    | Initial delay (milliseconds) before retrying. |
| `retry_delay_max_ms`     | `10,000`   | Maximum delay (milliseconds) between retries. |

### Production configuration

Below is an example configuration for a live trading node using Deribit data and execution clients:

```python
from nautilus_trader.adapters.deribit import DERIBIT
from nautilus_trader.adapters.deribit import DeribitDataClientConfig
from nautilus_trader.adapters.deribit import DeribitExecClientConfig
from nautilus_trader.adapters.deribit import DeribitLiveDataClientFactory
from nautilus_trader.adapters.deribit import DeribitLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import DeribitInstrumentKind
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        DERIBIT: DeribitDataClientConfig(
            api_key=None,           # Will use DERIBIT_API_KEY env var
            api_secret=None,        # Will use DERIBIT_API_SECRET env var
            instrument_kinds=(DeribitInstrumentKind.Future,),
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,
        ),
    },
    exec_clients={
        DERIBIT: DeribitExecClientConfig(
            api_key=None,
            api_secret=None,
            instrument_kinds=(DeribitInstrumentKind.Future,),
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,
        ),
    },
)

node = TradingNode(config=config)
node.add_data_client_factory(DERIBIT, DeribitLiveDataClientFactory)
node.add_exec_client_factory(DERIBIT, DeribitLiveExecClientFactory)
node.build()
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

### Instrument kinds

The `instrument_kinds` configuration option controls which Deribit instrument families are loaded.
Available options via the `DeribitInstrumentKind` enum:

- `DeribitInstrumentKind.Future` - Perpetual and dated futures
- `DeribitInstrumentKind.Option` - Call and put options
- `DeribitInstrumentKind.Spot` - Spot trading pairs
- `DeribitInstrumentKind.FutureCombo` - Future spread instruments
- `DeribitInstrumentKind.OptionCombo` - Option spread instruments

Example loading multiple instrument kinds:

```python
from nautilus_trader.core.nautilus_pyo3 import DeribitInstrumentKind

config = DeribitDataClientConfig(
    instrument_kinds=(
        DeribitInstrumentKind.Future,
        DeribitInstrumentKind.Option,
    ),
    # ... other config
)
```

### Base URL overrides

It's possible to override the default base URLs for both HTTP and WebSocket APIs:

| Environment | HTTP URL                   | WebSocket URL                      |
|-------------|----------------------------|------------------------------------|
| Production  | `https://www.deribit.com`  | `wss://www.deribit.com/ws/api/v2`  |
| Testnet     | `https://test.deribit.com` | `wss://test.deribit.com/ws/api/v2` |

## Contributing

:::info
For additional features or to contribute to the Deribit adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
