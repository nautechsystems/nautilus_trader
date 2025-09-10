# BitMEX

:::warning
The BitMEX integration is still under active development.
:::

Founded in 2014, BitMEX (Bitcoin Mercantile Exchange) is a cryptocurrency derivatives
trading platform offering spot, perpetual contracts, traditional futures, and other
advanced trading products. This integration supports live market data ingest and order
execution with BitMEX.

## Overview

This adapter is implemented in Rust, with optional Python bindings for ease of use in Python-based workflows.
**It does not require any external BitMEX client library dependencies**.

:::info
There is **no** need for additional installation steps for `bitmex`.
The core components of the adapter are compiled as a static library and automatically linked during the build process.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/bitmex/).

## Components

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The BitMEX adapter includes multiple components, which can be used together or separately depending
on the use case.

- `BitmexHttpClient`: Low-level HTTP API connectivity.
- `BitmexWebSocketClient`: Low-level WebSocket API connectivity.
- `BitmexInstrumentProvider`: Instrument parsing and loading functionality.
- `BitmexDataClient`: A market data feed manager.
- `BitmexExecutionClient`: An account management and trade execution gateway.
- `BitmexLiveDataClientFactory`: Factory for BitMEX data clients (used by the trading node builder).
- `BitmexLiveExecClientFactory`: Factory for BitMEX execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## BitMEX documentation

BitMEX provides extensive documentation for users:

- [BitMEX API Explorer](https://www.bitmex.com/app/restAPI) - Interactive API documentation.
- [BitMEX API Documentation](https://www.bitmex.com/app/apiOverview) - Complete API reference.
- [Contract Guides](https://www.bitmex.com/app/contract) - Detailed contract specifications.
- [Spot Trading Guide](https://www.bitmex.com/app/spotGuide) - Spot trading overview.
- [Perpetual Contracts Guide](https://www.bitmex.com/app/perpetualContractsGuide) - Perpetual swaps explained.
- [Futures Contracts Guide](https://www.bitmex.com/app/futuresGuide) - Traditional futures information.

It's recommended you refer to the BitMEX documentation in conjunction with this
NautilusTrader integration guide.

## Product support

| Product Type      | Data Feed | Trading | Notes                                           |
|-------------------|-----------|---------|-------------------------------------------------|
| Spot              | ✓         | ✓       | Limited pairs, unified wallet with derivatives. |
| Perpetual Swaps   | ✓         | ✓       | Inverse and linear contracts available.         |
| Futures           | ✓         | ✓       | Traditional fixed expiration contracts.         |
| Quanto Futures    | ✓         | ✓       | Settled in different currency than underlying.  |
| Options           | -         | -       | *Discontinued by BitMEX in April 2025*.         |

:::info
BitMEX discontinued their options products in April 2025 to focus on their core derivatives and spot offerings.
:::

### Spot trading

- Direct token/coin trading with immediate settlement.
- Major pairs including XBT/USDT, ETH/USDT, ETH/XBT.
- Additional altcoin pairs (LINK, SOL, UNI, APE, AXS, BMEX against USDT).

### Derivatives

- **Perpetual contracts**: Inverse (e.g., XBTUSD) and linear (e.g., ETHUSDT).
- **Traditional futures**: Fixed expiration date contracts.
- **Quanto futures**: Contracts settled in a different currency than the underlying.

:::note
While BitMEX has added spot trading capabilities, their primary focus remains on derivatives.
The platform uses a unified wallet for both spot and derivatives trading.
:::

## Symbology

BitMEX uses a specific naming convention for its trading symbols. Understanding this
convention is crucial for correctly identifying and trading instruments.

### Symbol format

BitMEX symbols typically follow these patterns:

- **Spot pairs**: Base currency + Quote currency (e.g., `XBT/USDT`, `ETH/USDT`).
- **Perpetual contracts**: Base currency + Quote currency (e.g., `XBTUSD`, `ETHUSD`).
- **Futures contracts**: Base currency + Expiry code (e.g., `XBTM24`, `ETHH25`).
- **Quanto contracts**: Special naming for non-USD settled contracts.

:::info
BitMEX uses `XBT` as the symbol for Bitcoin instead of `BTC`. This follows the ISO 4217
currency code standard where "X" denotes non-national currencies. XBT and BTC refer to
the same asset - Bitcoin.
:::

### Expiry codes

Futures contracts use standard futures month codes:

- `F` = January, `G` = February, `H` = March
- `J` = April, `K` = May, `M` = June
- `N` = July, `Q` = August, `U` = September
- `V` = October, `X` = November, `Z` = December

Followed by the year (e.g., `24` for 2024, `25` for 2025).

### NautilusTrader instrument IDs

Within NautilusTrader, BitMEX instruments are identified using the native BitMEX symbol
directly, combined with the venue identifier:

```python
from nautilus_trader.model.identifiers import InstrumentId

# Spot pairs (note: no slash in the symbol)
spot_id = InstrumentId.from_str("XBTUSDT.BITMEX")  # XBT/USDT spot
eth_spot_id = InstrumentId.from_str("ETHUSDT.BITMEX")  # ETH/USDT spot

# Perpetual contracts
perp_id = InstrumentId.from_str("XBTUSD.BITMEX")  # Bitcoin perpetual (inverse)
linear_perp_id = InstrumentId.from_str("ETHUSDT.BITMEX")  # Ethereum perpetual (linear)

# Futures contract (June 2024)
futures_id = InstrumentId.from_str("XBTM24.BITMEX")  # Bitcoin futures expiring June 2024
```

:::note
BitMEX spot symbols in NautilusTrader don't include the slash (/) that appears in the
BitMEX UI. Use `XBTUSDT` instead of `XBT/USDT`.
:::

## Order capability

BitMEX currently supports a limited set of order types in this integration,
with additional functionality being actively developed.

### Order types

| Order Type             | Supported | Notes                                         |
|------------------------|-----------|-----------------------------------------------|
| `MARKET`               | ✓         | Executed immediately at current market price. |
| `LIMIT`                | ✓         | Executed only at specified price or better.   |
| `STOP_MARKET`          | -         | *Currently under development*.                |
| `STOP_LIMIT`           | -         | *Currently under development*.                |
| `MARKET_IF_TOUCHED`    | -         | *Currently under development*.                |
| `LIMIT_IF_TOUCHED`     | -         | *Currently under development*.                |
| `TRAILING_STOP_MARKET` | -         | *Not yet implemented*.                        |

### Execution instructions

| Instruction   | Supported | Notes                                                       |
|---------------|-----------|-------------------------------------------------------------|
| `post_only`   | ✓         | Supported via `ParticipateDoNotInitiate` on `LIMIT` orders. |
| `reduce_only` | -         | *Currently under development*.                              |

:::note
Post-only orders are implemented using BitMEX's `ParticipateDoNotInitiate` execution
instruction, which ensures orders are added to the order book as maker orders only.
:::

### Time in force

| Time in force | Supported | Notes                                          |
|---------------|-----------|------------------------------------------------|
| `GTC`         | ✓         | Good Till Canceled (default).                  |
| `GTD`         | -         | *Not supported by BitMEX*.                     |
| `FOK`         | -         | *Currently under development*.                 |
| `IOC`         | -         | *Currently under development*.                 |

### Advanced order features

| Feature            | Supported | Notes                                       |
|--------------------|-----------|---------------------------------------------|
| Order Modification | -         | *Currently under development*.              |
| Bracket Orders     | -         | *Not yet implemented*.                      |
| Iceberg Orders     | -         | *Supported by BitMEX, not yet implemented*. |

### Batch operations

| Operation          | Supported | Notes                                         |
|--------------------|-----------|-----------------------------------------------|
| Batch Submit       | -         | *Not yet implemented*.                        |
| Batch Modify       | -         | *Not yet implemented*.                        |
| Batch Cancel       | -         | *Not yet implemented*.                        |

### Position management

| Feature             | Supported | Notes                                        |
|---------------------|-----------|----------------------------------------------|
| Query positions     | ✓         | Real-time position updates via WebSocket.    |
| Leverage control    | -         | *Currently under development*.               |
| Cross margin        | ✓         | Default margin mode.                         |
| Isolated margin     | -         | *Currently under development*.               |

### Order querying

| Feature             | Supported | Notes                                        |
|---------------------|-----------|----------------------------------------------|
| Query open orders   | ✓         | List all active orders.                      |
| Query order history | ✓         | Historical order data.                       |
| Order status updates| ✓         | Real-time order state changes via WebSocket. |
| Trade history       | ✓         | Execution and fill reports.                  |

## Rate limits

BitMEX implements a dual-layer rate limiting system:

### REST API limits

- **Primary rate limit**:
  - 120 requests per minute for authenticated users.
  - 30 requests per minute for unauthenticated users.
  - Uses a token bucket mechanism with continuous refill.
- **Secondary rate limit**:
  - 10 requests per second burst limit for specific endpoints (order management).
  - Applies to order placement, modification, and cancellation.
- **Order limits**:
  - 200 open orders per symbol per account.
  - 10 stop orders per symbol per account.

The adapter automatically respects these limits through built-in rate limiting with a
10 requests/second quota that handles both the burst limit and average rate requirements.

### WebSocket limits

- 20 connections per hour per IP address
- Authentication required for private data streams

### Rate limit headers

BitMEX provides rate limit information in response headers:

- `x-ratelimit-limit`: Total allowed requests
- `x-ratelimit-remaining`: Remaining requests in current window
- `x-ratelimit-reset`: Unix timestamp when limits reset
- `retry-after`: Seconds to wait if rate limited (429 response)

:::warning
Exceeding rate limits will result in HTTP 429 responses and potential temporary IP bans.
Multiple 4xx/5xx errors in quick succession may trigger longer bans.
:::

## Connection management

### HTTP Keep-Alive

The BitMEX adapter utilizes HTTP keep-alive for optimal performance:

- **Connection pooling**: Connections are automatically pooled and reused.
- **Keep-alive timeout**: 90 seconds (matches BitMEX server-side timeout).
- **Automatic reconnection**: Failed connections are automatically re-established.
- **SSL session caching**: Reduces handshake overhead for subsequent requests.

This configuration ensures low-latency communication with BitMEX servers by maintaining
persistent connections and avoiding the overhead of establishing new connections for each request.

### Request expiration

BitMEX uses an `api-expires` header for request authentication:

- Requests include a UNIX timestamp indicating when they expire.
- Default expiration window is 10 seconds from request creation.
- Prevents replay attacks and ensures request freshness.

## Configuration

### API credentials

BitMEX API credentials can be provided either directly in the configuration or via environment variables:

- `BITMEX_API_KEY`: Your BitMEX API key.
- `BITMEX_API_SECRET`: Your BitMEX API secret.

To generate API keys:

1. Log in to your BitMEX account.
2. Navigate to Account & Security → API Keys.
3. Create a new API key with appropriate permissions.
4. For testnet, use [testnet.bitmex.com](https://testnet.bitmex.com).

### Configuration examples

A typical BitMEX configuration for live trading includes both testnet and mainnet options:

```python
from nautilus_trader.adapters.bitmex.config import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex.config import BitmexExecClientConfig

# Using environment variables (recommended)
testnet_data_config = BitmexDataClientConfig(
    testnet=True,  # API credentials loaded from BITMEX_API_KEY and BITMEX_API_SECRET
)

# Using explicit credentials
mainnet_data_config = BitmexDataClientConfig(
    api_key="YOUR_API_KEY",  # Or use os.getenv("BITMEX_API_KEY")
    api_secret="YOUR_API_SECRET",  # Or use os.getenv("BITMEX_API_SECRET")
    testnet=False,
)

mainnet_exec_config = BitmexExecClientConfig(
    api_key="YOUR_API_KEY",
    api_secret="YOUR_API_SECRET",
    testnet=False,
)
```

## Trading considerations

### Contract specifications

- **Inverse contracts**: Settled in cryptocurrency (e.g., XBTUSD settled in XBT).
- **Linear contracts**: Settled in stablecoin (e.g., ETHUSDT settled in USDT).
- **Contract size**: Varies by instrument, check specifications carefully.
- **Tick size**: Minimum price increment varies by contract.

### Margin requirements

- Initial margin requirements vary by contract and market conditions.
- Maintenance margin is typically lower than initial margin.
- Liquidation occurs when equity falls below maintenance margin.

### Fees

- **Maker fees**: Typically negative (rebate) for providing liquidity.
- **Taker fees**: Positive fee for taking liquidity.
- **Funding rates**: Apply to perpetual contracts every 8 hours.

## Known limitations

The BitMEX integration is actively being developed. Current limitations include:

- Limited order type support (only MARKET and LIMIT orders for now).
- Post-only functionality pending full implementation.
- Stop orders and advanced order types not yet available.
- Batch operations not implemented.
- Margin mode switching not available.

:::note
We welcome contributions to extend the BitMEX adapter functionality. Please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md)
for more information.
:::
