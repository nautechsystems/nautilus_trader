# OKX

:::warning
The OKX integration is still under active development.
:::

Founded in 2017, OKX is a leading cryptocurrency exchange offering spot, perpetual swap,
futures, and options trading. This integration supports live market data ingest and order
execution on OKX.

## Overview

This adapter is implemented in Rust, with optional Python bindings for ease of use in Python-based workflows.
**It does not require any external OKX client library dependencies**.

:::info
There is **no** need for additional installation steps for `okx`.
The core components of the adapter are compiled as a static library and automatically linked during the build process.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/okx/).

### Product support

| Product Type      | Supported | Notes                                          |
|-------------------|-----------|------------------------------------------------|
| Spot              | ✓         | Use for index prices.                          |
| Perpetual Swaps   | ✓         | Linear and inverse contracts.                  |
| Futures           | ✓         | Specific expiration dates.                     |
| Margin            | -         | *Not yet supported*.                           |
| Options           | -         | *Not yet supported*.                           |

The OKX adapter includes multiple components, which can be used separately or together depending on your use case.

- `OKXHttpClient`: Low-level HTTP API connectivity.
- `OKXWebSocketClient`: Low-level WebSocket API connectivity.
- `OKXInstrumentProvider`: Instrument parsing and loading functionality.
- `OKXDataClient`: Market data feed manager.
- `OKXExecutionClient`: Account management and trade execution gateway.
- `OKXLiveDataClientFactory`: Factory for OKX data clients (used by the trading node builder).
- `OKXLiveExecClientFactory`: Factory for OKX execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (as shown below),
and won’t need to work directly with these lower-level components.
:::

## Symbology

OKX uses native symbols such as `BTC-USDT-SWAP` for linear perpetual swap contracts.
Instruments are identified using the OKX native format.

## Order capability

Below are the order types, execution instructions, and time-in-force options supported
for linear perpetual swap products on OKX.

### Order types

| Order Type          | Linear Perpetual Swap | Notes                |
|---------------------|-----------------------|----------------------|
| `MARKET`            | ✓                     |                      |
| `LIMIT`             | ✓                     |                      |
| `STOP_MARKET`       | ✓                     |                      |
| `STOP_LIMIT`        | ✓                     |                      |
| `MARKET_IF_TOUCHED` | ✓                     |                      |
| `LIMIT_IF_TOUCHED`  | ✓                     |                      |
| `TRAILING_STOP`     | -                     | *Not yet supported*. |

### Execution instructions

| Instruction    | Linear Perpetual Swap | Notes                  |
|----------------|-----------------------|------------------------|
| `post_only`    | ✓                     | Only for LIMIT orders. |
| `reduce_only`  | ✓                     | Only for derivatives.  |

### Time in force

| Time in force | Linear Perpetual Swap | Notes                |
|---------------|-----------------------|----------------------|
| `GTC`         | ✓                     | Good Till Canceled.  |
| `FOK`         | ✓                     | Fill or Kill.        |
| `IOC`         | ✓                     | Immediate or Cancel. |

### Batch operations

| Operation          | Linear Perpetual Swap | Notes                                        |
|--------------------|-----------------------|----------------------------------------------|
| Batch Submit       | ✓                     | Submit multiple orders in single request.    |
| Batch Modify       | ✓                     | Modify multiple orders in single request.    |
| Batch Cancel       | ✓                     | Cancel multiple orders in single request.    |

### Position management

| Feature           | Linear Perpetual Swap | Notes                                        |
|-------------------|-----------------------|----------------------------------------------|
| Query positions   | ✓                     | Real-time position updates.                  |
| Position mode     | ✓                     | Net vs Long/Short mode.                      |
| Leverage control  | ✓                     | Dynamic leverage adjustment per instrument.  |
| Margin mode       | ✓                     | Cross vs Isolated margin.                    |

### Order querying

| Feature              | Linear Perpetual Swap | Notes                                     |
|----------------------|-----------------------|-------------------------------------------|
| Query open orders    | ✓                     | List all active orders.                   |
| Query order history  | ✓                     | Historical order data.                    |
| Order status updates | ✓                     | Real-time order state changes.            |
| Trade history        | ✓                     | Execution and fill reports.               |

### Contingent orders

| Feature              | Linear Perpetual Swap | Notes                                     |
|--------------------|-----------------------|---------------------------------------------|
| Order lists         | -                     | *Not supported*.                           |
| OCO orders          | ✓                     | One-Cancels-Other orders.                  |
| Bracket orders      | ✓                     | Stop loss + take profit combinations.      |
| Conditional orders  | ✓                     | Stop and limit-if-touched orders.          |

## Authentication

To use the OKX adapter, you'll need to create API credentials in your OKX account:

1. Log into your OKX account and navigate to the API management page.
2. Create a new API key with the required permissions for trading and data access.
3. Note down your API key, secret key, and passphrase.

You can provide these credentials through environment variables:

```bash
export OKX_API_KEY="your_api_key"
export OKX_API_SECRET="your_api_secret"
export OKX_API_PASSPHRASE="your_passphrase"
```

Or pass them directly in the configuration (not recommended for production).

## Rate limits

The OKX adapter implements automatic rate limiting for both HTTP and WebSocket connections to respect OKX's API limits and prevent rate limit errors.

### HTTP rate limiting

The HTTP client implements a conservative rate limit of **250 requests per second**. This limit is based on OKX's documented rate limits:

- Sub-account order limit: 1000 requests per 2 seconds.
- Account balance: 10 requests per 2 seconds.
- Account instruments: 20 requests per 2 seconds.

### WebSocket rate limiting

The WebSocket client implements keyed rate limiting with different quotas for different operation types:

- **General operations** (subscriptions): 3 requests per second.
- **Order operations** (place/cancel/amend): 250 requests per second.

This approach ensures that subscription management doesn't interfere with order execution performance while respecting OKX's connection rate limits.

### OKX API rate limits

OKX enforces various rate limits on their API endpoints:

- **REST API**: Varies by endpoint (typically 20-1000 requests per 2 seconds depending on the endpoint).
- **WebSocket**: 3 connection requests per second per IP, with subscription and order limits.
- **Trading**: Order placement has specific limits based on account level and instrument type.

For complete and up-to-date rate limit information, refer to the [OKX API documentation](https://www.okx.com/docs-v5/en/#overview-rate-limit).

## Configuration

Below is an example configuration for a live trading node using OKX data and execution clients:

```python
from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig, OKXExecClientConfig
from nautilus_trader.adapters.okx.factories import OKXLiveDataClientFactory, OKXLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig, LiveExecEngineConfig, LoggingConfig, TradingNodeConfig
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,
    data_clients={
        OKX: OKXDataClientConfig(
            api_key=None,           # from OKX_API_KEY env var
            api_secret=None,        # from OKX_API_SECRET env var
            api_passphrase=None,    # from OKX_API_PASSPHRASE env var
            base_url_http=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=("SWAP",),
            contract_types=None,
            is_demo=False,
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            api_key=None,
            api_secret=None,
            api_passphrase=None,
            base_url_http=None,
            base_url_ws=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=("SWAP",),
            contract_types=None,
            is_demo=False,
        ),
    },
)
node = TradingNode(config=config)
node.add_data_client_factory(OKX, OKXLiveDataClientFactory)
node.add_exec_client_factory(OKX, OKXLiveExecClientFactory)
node.build()
```

## Error handling

Common issues when using the OKX adapter:

- **Authentication errors**: Verify your API credentials and ensure they have the required permissions.
- **Insufficient permissions**: Verify your API key has trading permissions if executing orders.
- **Rate limit exceeded**: Reduce request frequency or implement delays between requests.
- **Invalid symbols**: Ensure you're using valid OKX instrument identifiers.

For detailed error information, check the NautilusTrader logs.

## References

See the OKX [API documentation](https://www.okx.com/docs-v5/) and the NautilusTrader [API reference](../api_reference/adapters/okx.md) for more details.
