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

| Product Type      | Data Feed | Trading | Notes                                          |
|-------------------|-----------|---------|------------------------------------------------|
| Spot              | ✓         | ✓       | Use for index prices.                          |
| Perpetual Swaps   | ✓         | ✓       | Linear and inverse contracts.                  |
| Futures           | ✓         | ✓       | Specific expiration dates.                     |
| Margin            | -         | -       | *Not yet supported*.                           |
| Options           | ✓         | -       | *Data feed supported, trading coming soon*.    |

:::info
**Options support**: While you can subscribe to options market data and receive price updates, order execution for options is not yet implemented. You can use the symbology format shown above to subscribe to options data feeds.
:::

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

OKX uses specific symbol conventions for different instrument types. All instrument IDs should include the `.OKX` suffix when referencing them (e.g., `BTC-USDT.OKX` for spot Bitcoin).

### Symbol format by instrument type

#### SPOT
Format: `{BaseCurrency}-{QuoteCurrency}`

Examples:

- `BTC-USDT` - Bitcoin against USDT (Tether)
- `BTC-USDC` - Bitcoin against USDC
- `ETH-USDT` - Ethereum against USDT
- `SOL-USDT` - Solana against USDT

To subscribe to spot Bitcoin USD in your strategy:

```python
InstrumentId.from_str("BTC-USDT.OKX")  # For USDT-quoted spot
InstrumentId.from_str("BTC-USDC.OKX")  # For USDC-quoted spot
```

#### SWAP (Perpetual Futures)

Format: `{BaseCurrency}-{QuoteCurrency}-SWAP`

Examples:

- `BTC-USDT-SWAP` - Bitcoin perpetual swap (linear, USDT-margined)
- `BTC-USD-SWAP` - Bitcoin perpetual swap (inverse, coin-margined)
- `ETH-USDT-SWAP` - Ethereum perpetual swap (linear)
- `ETH-USD-SWAP` - Ethereum perpetual swap (inverse)

Linear vs Inverse contracts:

- **Linear** (USDT-margined): Uses stablecoins like USDT as margin.
- **Inverse** (coin-margined): Uses the base cryptocurrency as margin.

#### FUTURES (Dated Futures)

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}`

Examples:

- `BTC-USD-251226` - Bitcoin futures expiring December 26, 2025
- `ETH-USD-251226` - Ethereum futures expiring December 26, 2025
- `BTC-USD-250328` - Bitcoin futures expiring March 28, 2025

Note: Futures are typically inverse contracts (coin-margined).

#### OPTIONS

Format: `{BaseCurrency}-{QuoteCurrency}-{YYMMDD}-{Strike}-{Type}`

Examples:

- `BTC-USD-250328-100000-C` - Bitcoin call option, $100,000 strike, expiring March 28, 2025
- `BTC-USD-250328-100000-P` - Bitcoin put option, $100,000 strike, expiring March 28, 2025
- `ETH-USD-250328-4000-C` - Ethereum call option, $4,000 strike, expiring March 28, 2025

Where:

- `C` = Call option
- `P` = Put option

### Common questions

**Q: How do I subscribe to spot Bitcoin USD?**
A: Use `BTC-USDT.OKX` for USDT-margined spot or `BTC-USDC.OKX` for USDC-margined spot.

**Q: What's the difference between BTC-USDT-SWAP and BTC-USD-SWAP?**
A: `BTC-USDT-SWAP` is a linear perpetual (USDT-margined), while `BTC-USD-SWAP` is an inverse perpetual (BTC-margined).

**Q: How do I know which contract type to use?**
A: Check the `contract_types` parameter in the configuration:

- For linear contracts: `OKXContractType.LINEAR`.
- For inverse contracts: `OKXContractType.INVERSE`.

## Order capability

Below are the order types, execution instructions, and time-in-force options supported
for linear perpetual swap products on OKX.

### Client order ID requirements

:::warning
OKX has specific requirements for client order IDs:

- **No hyphens allowed**: OKX does not accept hyphens (`-`) in client order IDs.
- Maximum length: 32 characters.
- Allowed characters: alphanumeric characters and underscores only.

When configuring your strategy, ensure you set:

```python
use_hyphens_in_client_order_ids=False
```

:::

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

| Time in force | Linear Perpetual Swap | Notes                                             |
|---------------|-----------------------|---------------------------------------------------|
| `GTC`         | ✓                     | Good Till Canceled.                               |
| `FOK`         | ✓                     | Fill or Kill.                                     |
| `IOC`         | ✓                     | Immediate or Cancel.                              |
| `GTD`         | ✗                     | *Not supported by OKX. Use strategy-managed GTD.* |

:::info
**GTD (Good Till Date) time in force**: OKX does not support GTD time in force through their API.
If you need GTD functionality, you should use Nautilus's strategy-managed GTD feature instead,
which will handle the order expiration by canceling the order at expiry.
:::

### Batch operations

| Operation          | Linear Perpetual Swap | Notes                                     |
|--------------------|-----------------------|-------------------------------------------|
| Batch Submit       | ✓                     | Submit multiple orders in single request. |
| Batch Modify       | ✓                     | Modify multiple orders in single request. |
| Batch Cancel       | ✓                     | Cancel multiple orders in single request. |

### Position management

| Feature           | Linear Perpetual Swap | Notes                                                |
|-------------------|-----------------------|------------------------------------------------------|
| Query positions   | ✓                     | Real-time position updates.                          |
| Position mode     | ✓                     | Net vs Long/Short mode.                              |
| Leverage control  | ✓                     | Dynamic leverage adjustment per instrument.          |
| Margin mode       | Isolated              | Currently isolated only. *Cross margin coming soon*. |

### Order querying

| Feature              | Linear Perpetual Swap | Notes                                     |
|----------------------|-----------------------|-------------------------------------------|
| Query open orders    | ✓                     | List all active orders.                   |
| Query order history  | ✓                     | Historical order data.                    |
| Order status updates | ✓                     | Real-time order state changes.            |
| Trade history        | ✓                     | Execution and fill reports.               |

### Contingent orders

| Feature             | Linear Perpetual Swap | Notes                                     |
|---------------------|-----------------------|---------------------------------------------|
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

The HTTP client implements a global default rate limit of **250 requests per second**. This is a conservative default that works across most endpoints. However, note that individual OKX endpoints have their own server-side limits:

- Sub-account order limit: 1000 requests per 2 seconds.
- Account balance: 10 requests per 2 seconds (more restrictive).
- Account instruments: 20 requests per 2 seconds.

The global limiter helps prevent hitting the overall rate limit, but endpoints with lower server-side limits may still rate-limit if accessed too frequently.

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
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,
    data_clients={
        OKX: OKXDataClientConfig(
            api_key=None,           # Will use OKX_API_KEY env var
            api_secret=None,        # Will use OKX_API_SECRET env var
            api_passphrase=None,    # Will use OKX_API_PASSPHRASE env var
            base_url_http=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(OKXInstrumentType.SWAP,),
            contract_types=(OKXContractType.LINEAR),
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
            instrument_types=(OKXInstrumentType.SWAP,),
            contract_types=(OKXContractType.LINEAR),
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
