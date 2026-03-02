# Deribit

Founded in 2016, Deribit is a cryptocurrency derivatives exchange specializing in Bitcoin and
Ethereum options and futures. It is one of the largest crypto options exchanges by volume,
and a leading platform for crypto derivatives trading.

This integration supports live market data ingest and order execution with Deribit.

## Overview

This adapter is implemented in Rust, with optional Python bindings for use in Python-based workflows.
Deribit uses JSON-RPC 2.0 over both HTTP and WebSocket transports.
WebSocket is preferred for subscriptions and real-time data.

The official Deribit API reference can be found at [docs.deribit.com](https://docs.deribit.com/).

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

| Product Type      | Data Feed | Trading | Notes                            |
|-------------------|-----------|---------|----------------------------------|
| Perpetual Futures | ✓         | ✓       | BTC-PERPETUAL, ETH-PERPETUAL.    |
| Dated Futures     | ✓         | ✓       | Futures with fixed expiry dates. |
| Options           | ✓         | ✓       | BTC and ETH options.             |
| Spot              | ✓         | ✓       | BTC_USDC, ETH_USDC pairs.        |
| Future Combos     | ✓         | ✓       | Calendar spreads for futures.    |
| Option Combos     | ✓         | ✓       | Option spread strategies.        |

## Symbology

Deribit uses specific symbol conventions for different instrument types.
All instrument IDs should include the `.DERIBIT` suffix when referencing them
(e.g., `BTC-PERPETUAL.DERIBIT` for BTC perpetual).

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

- Requires authenticated connection (safeguard against abuse).
- Use when you need every price level change for HFT or market making.
- Higher message volume.

### Aggregated feeds (batched)

Aggregated channels deliver updates in batches at a fixed interval (e.g., every 100ms).
This groups multiple order book changes into single messages.

- Available without authentication.
- Recommended for most use cases.
- Lower message volume, easier to process.
- Default interval: 100ms.

### Subscription parameters

The Nautilus adapter supports both feed types via subscription parameters:

| Parameter | Values | Notes |
|-----------|--------|-------|
| `interval` | `raw`, `100ms`, `agg2` | Default: `100ms`. `agg2` batches at ~1 second intervals. `raw` requires auth. |
| `depth` | `1`, `10`, `20` | Default: `10`. Number of price levels per side. |

```python
from nautilus_trader.model.identifiers import InstrumentId

instrument_id = InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")

# Default: 100ms aggregated feed (no authentication required)
strategy.subscribe_order_book_deltas(instrument_id)

# Raw feed (requires API credentials)
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

### Sequence gap recovery

The adapter tracks `change_id` / `prev_change_id` sequence numbers on every book update.
When a gap is detected (missed message), the adapter automatically:

1. Drops all incoming deltas for the affected instrument.
2. Unsubscribes from the book channel.
3. Resubscribes to obtain a fresh snapshot.
4. Resumes normal processing once the snapshot arrives.

During resync, the strategy will not receive stale or incomplete book updates.

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
**GTD on Deribit**: Unlike other exchanges where GTD accepts an arbitrary expiry time,
Deribit's `good_til_day` always expires at 8:00 UTC the same or next day. Custom expiry times
will be logged as warnings and the order will use the exchange's fixed expiry behavior.
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

### Post-only behavior

Deribit offers two post-only modes:

1. **Price adjustment (Deribit default)**: If a post-only order would cross the spread and execute,
   Deribit automatically adjusts the price to one tick inside the spread.
2. **Reject mode**: Order is immediately rejected if it would cross the spread.

The Nautilus adapter uses **reject mode** (`reject_post_only=true`) to ensure deterministic behavior.
If a post-only order would take liquidity, it is rejected with error code `11054`, and an `OrderRejected`
event is emitted with the `due_post_only` flag set to `true`.

This allows strategies to differentiate between:

- Orders rejected due to post-only violation (attempted to take liquidity).
- Orders rejected for other reasons (insufficient margin, invalid price, etc.).

### Order modification

The adapter uses Deribit's native `private/edit` endpoint rather than cancel-and-replace.
This provides several advantages:

| Benefit                    | Description                                                        |
|----------------------------|--------------------------------------------------------------------|
| Single request             | Faster execution, lower latency than cancel + new order.           |
| Queue priority preservation | Keeps position when only reducing quantity or keeping same price. |
| Fill history maintained    | Partial fills remain linked to the same order ID.                  |

**Queue priority rules:**

- **Decreasing quantity only**: Keeps queue position.
- **Same price**: Keeps queue position.
- **Increasing quantity or changing price**: Loses queue position (treated as new order).

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

:::note
The Nautilus adapter uses WebSocket for order submission (not REST) for lower latency.
Order operations are rate-limited by `DERIBIT_WS_ORDER_QUOTA` (5 req/sec, 20 burst).
:::

### Credit-based system details

Deribit uses a sophisticated credit-based rate limiting system where credits are replenished
continuously at a fixed rate. Each second, credits "drip" back into your sub-account's credit pool.

**Non-matching engine requests:**

| Parameter        | Value              | Notes                           |
|------------------|--------------------|---------------------------------|
| Cost per request | 500 credits        | Each API call consumes credits. |
| Maximum pool     | 50,000 credits     | Allows 100 request burst.       |
| Refill rate      | 10,000 credits/sec | ~20 sustained requests/second.  |

**Matching engine requests (default tier):**

| Parameter      | Value          | Notes                              |
|----------------|----------------|------------------------------------|
| Sustained rate | 5 requests/sec | Continuous rate limit.             |
| Burst capacity | 20 requests    | Maximum burst before throttling.   |

Higher matching engine limits are available for market makers and high-volume traders based on
7-day trading volume tiers.

The Nautilus adapter implements this using token bucket rate limiters configured as:

- `DERIBIT_HTTP_REST_QUOTA`: 20 req/sec with 100 burst (non-matching REST)
- `DERIBIT_HTTP_ORDER_QUOTA`: 5 req/sec with 20 burst (matching engine REST)
- `DERIBIT_WS_ORDER_QUOTA`: 5 req/sec with 20 burst (matching engine WebSocket)
- `DERIBIT_WS_SUBSCRIPTION_QUOTA`: 3 req/sec with 10 burst (subscribe/unsubscribe)

For more details, see the [Rate Limits article](https://support.deribit.com/hc/en-us/articles/25944617523357-Rate-Limits).

:::warning
Deribit returns error code `10028` (too_many_requests) when you exceed the allowed quota.
Repeated violations may result in temporary throttling.
:::

## Connection management

### Platform limits

| Limit                             | Value |
|-----------------------------------|-------|
| Maximum connections per IP        | 32    |
| Maximum sessions per API key      | 16    |
| Maximum API keys per (sub)account | 8     |

### Session-based authentication

The adapter uses **separate WebSocket sessions** for data and execution clients, each with its own
authentication scope:

| Client           | Session Name         | Purpose                                              |
|------------------|----------------------|------------------------------------------------------|
| Data client      | `nautilus-data`      | Market data subscriptions (raw feeds require auth).  |
| Execution client | `nautilus-execution` | Order operations (buy, sell, edit, cancel).          |

**Authentication flow:**

1. WebSocket connects to Deribit.
2. Client authenticates using `client_signature` grant type with session scope.
3. Tokens are automatically refreshed at 80% of expiry time (continuous refresh cycle).
4. On reconnection, re-authentication is retried with exponential backoff (up to 3 attempts).
   If all attempts fail, only public channel subscriptions are restored.

This session-based approach allows:

- Independent token management per client type.
- Isolated failure domains (data auth failure does not affect execution).
- Clear audit trail in Deribit's session logs.

### Best practices

The adapter follows Deribit's
[recommended connection practices](https://support.deribit.com/hc/en-us/articles/25944603459613):

1. **Uses WebSocket subscriptions** for real-time data instead of REST polling, resulting in fewer requests,
   lower latency, and reduced rate limit consumption.
2. **Authenticates all connections** when credentials are provided. Authenticated users benefit
   from higher rate limits and are less likely to be IP rate-limited.
3. **Implements heartbeats** (30 second interval) to maintain connection health and detect
   disconnections early.
4. **Handles reconnection** automatically with re-authentication and subscription recovery.

:::tip
Always provide API credentials even for public data access. Authenticated connections have higher
rate limits, and Deribit contacts authenticated clients before applying restrictions during
high-load periods.
:::

:::note
The adapter uses a 30 second heartbeat interval, which is the lower bound of Deribit's recommended
30-60 second range. More frequent heartbeats may trigger stricter rate limits.
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

### API key scopes

Each API key on Deribit is assigned a default access scope, which defines the maximum permissions.
Configure appropriate permissions when
[creating your API key](https://support.deribit.com/hc/en-us/articles/26268257333661):

| Scope              | Required For                           |
|--------------------|----------------------------------------|
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

- HTTP requests use `https://test.deribit.com`.
- WebSocket connections use `wss://test.deribit.com/ws/api/v2`.
- Loads credentials from `DERIBIT_TESTNET_API_KEY` and `DERIBIT_TESTNET_API_SECRET` environment variables.

:::note
Testnet API keys are separate from production keys. Create API keys specifically
for the testnet through the testnet interface at [test.deribit.com](https://test.deribit.com).
:::

## Configuration

### Data client configuration options

| Option                             | Default    | Description |
|------------------------------------|------------|-------------|
| `api_key`                          | `None`     | Deribit API key; loads from environment variables when omitted. |
| `api_secret`                       | `None`     | Deribit API secret; loads from environment variables when omitted. |
| `product_types`                    | `None`     | Product types to load (Future, Option, Spot, etc.). If `None`, defaults to Future. |
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
| `api_key`                | `None`     | Deribit API key; loads from environment variables when omitted. |
| `api_secret`             | `None`     | Deribit API secret; loads from environment variables when omitted. |
| `product_types`          | `None`     | Product types to load (Future, Option, Spot, etc.). If `None`, defaults to Future. |
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
from nautilus_trader.core.nautilus_pyo3 import DeribitProductType
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        DERIBIT: DeribitDataClientConfig(
            api_key=None,           # Uses DERIBIT_API_KEY env var
            api_secret=None,        # Uses DERIBIT_API_SECRET env var
            product_types=(DeribitProductType.Future,),
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,
        ),
    },
    exec_clients={
        DERIBIT: DeribitExecClientConfig(
            api_key=None,
            api_secret=None,
            product_types=(DeribitProductType.Future,),
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

### Product types

The `product_types` configuration option controls which Deribit product families are loaded.
Available options via the `DeribitProductType` enum:

- `DeribitProductType.Future` - Perpetual and dated futures.
- `DeribitProductType.Option` - Call and put options.
- `DeribitProductType.Spot` - Spot trading pairs.
- `DeribitProductType.FutureCombo` - Future spread instruments.
- `DeribitProductType.OptionCombo` - Option spread instruments.

Example loading multiple product types:

```python
from nautilus_trader.core.nautilus_pyo3 import DeribitProductType

config = DeribitDataClientConfig(
    product_types=(
        DeribitProductType.Future,
        DeribitProductType.Option,
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

## Server infrastructure

Deribit's matching engine is located in **Equinix LD4, Slough, UK**. For latency-sensitive strategies,
consider hosting in or near London. Colocation and cross-connect options are available directly
from Deribit for institutional clients.

For most users connecting via internet, the adapter's built-in retry logic, heartbeat monitoring,
and automatic reconnection handling provide reliable connectivity.

For more details, see the [Server Infrastructure article](https://support.deribit.com/hc/en-us/articles/25944617582877).

## Contributing

:::info
For additional features or to contribute to the Deribit adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
