# Hyperliquid

Hyperliquid is a decentralized perpetual futures exchange built on the Arbitrum blockchain,
offering a fully on-chain order book and matching engine. This integration supports live market
data feeds and order execution on Hyperliquid.

:::warning
The Hyperliquid integration is under active development. Some features may be incomplete.
:::

## Overview

This adapter is implemented in Rust with Python bindings. It provides direct integration
with Hyperliquid's REST and WebSocket APIs without requiring external client libraries.

The Hyperliquid adapter includes multiple components:

- `HyperliquidHttpClient`: Low-level HTTP API connectivity.
- `HyperliquidWebSocketClient`: Low-level WebSocket API connectivity.
- `HyperliquidInstrumentProvider`: Instrument parsing and loading functionality.
- `HyperliquidDataClient`: Market data feed manager.
- `HyperliquidExecutionClient`: Account management and trade execution gateway.
- `HyperliquidLiveDataClientFactory`: Factory for Hyperliquid data clients (used by the trading node builder).
- `HyperliquidLiveExecClientFactory`: Factory for Hyperliquid execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as shown below)
and won't need to work directly with these lower-level components.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/hyperliquid/).

## Testnet setup

Hyperliquid provides a testnet environment for testing strategies without risking real funds.

### Getting testnet credentials

1. Visit the [Hyperliquid testnet portal](https://app.hyperliquid-testnet.xyz/)
2. Connect your wallet (MetaMask or WalletConnect)
3. The testnet will automatically create an account for your wallet address
4. Request testnet funds from the faucet (if available)

### Exporting your private key

To use your testnet account with NautilusTrader, you need to export your wallet's private key:

**MetaMask:**

1. Click the three dots menu next to your account
2. Select "Account details"
3. Click "Show private key"
4. Enter your password and copy the private key

:::warning
**Never share your private keys**
Store private keys securely using environment variables, never commit them to version control.
:::

### Setting environment variables

Set your testnet credentials as environment variables:

```bash
export HYPERLIQUID_TESTNET_PK="your_private_key_here"
# Optional: for vault trading
export HYPERLIQUID_TESTNET_VAULT="vault_address_here"
```

The adapter will automatically load these when `testnet=True` in the configuration.

## Product support

Hyperliquid currently supports perpetual futures contracts.

| Product Type        | Data Feed | Trading | Notes                           |
|---------------------|-----------|---------|----------------------------------|
| Perpetual Futures   | ✓         | ✓       | Both PERP and SPOT instruments. |
| Spot                | ✓         | ✓       | Native spot markets.             |

:::note
All instruments on Hyperliquid are settled in USDC.
:::

## Symbology

Hyperliquid uses a specific symbol format for instruments:

### Perpetual futures

Format: `{Base}-USD-PERP`

Examples:

- `BTC-USD-PERP` - Bitcoin perpetual futures
- `ETH-USD-PERP` - Ethereum perpetual futures
- `SOL-USD-PERP` - Solana perpetual futures

To subscribe in your strategy:

```python
InstrumentId.from_str("BTC-USD-PERP.HYPERLIQUID")
InstrumentId.from_str("ETH-USD-PERP.HYPERLIQUID")
```

### Spot markets

Format: `{Base}-{Quote}-SPOT`

Examples:

- `PURR-USDC-SPOT` - PURR/USDC spot pair
- `HYPE-USDC-SPOT` - HYPE/USDC spot pair

To subscribe in your strategy:

```python
InstrumentId.from_str("PURR-USDC-SPOT.HYPERLIQUID")
```

:::note
Spot instruments may include vault tokens (prefixed with `vntls:`). These are automatically
handled by the instrument provider.
:::

## Orders capability

Hyperliquid supports a comprehensive set of order types and execution options.

### Order types

| Order Type             | Perpetuals | Spot | Notes                                  |
|------------------------|------------|------|----------------------------------------|
| `MARKET`               | ✓          | ✓    | Executed as IOC limit order.           |
| `LIMIT`                | ✓          | ✓    |                                        |
| `STOP_MARKET`          | ✓          | ✓    | Stop loss orders.                      |
| `STOP_LIMIT`           | ✓          | ✓    | Stop loss with limit execution.        |
| `MARKET_IF_TOUCHED`    | ✓          | ✓    | Take profit at market.                 |
| `LIMIT_IF_TOUCHED`     | ✓          | ✓    | Take profit with limit execution.      |

:::info
Conditional orders (stop and if-touched) are implemented using Hyperliquid's native trigger
order functionality with automatic TP/SL mode detection.
:::

### Time in force

| Time in force | Perpetuals | Spot | Notes                        |
|---------------|------------|------|------------------------------|
| `GTC`         | ✓          | ✓    | Good Till Canceled.          |
| `IOC`         | ✓          | ✓    | Immediate or Cancel.         |
| `FOK`         | -          | -    | *Not supported*.             |
| `GTD`         | -          | -    | *Not supported*.             |

### Execution instructions

| Instruction   | Perpetuals | Spot | Notes                              |
|---------------|------------|------|------------------------------------|
| `post_only`   | ✓          | ✓    | Equivalent to ALO time in force.   |
| `reduce_only` | ✓          | ✓    | Close-only orders.                 |

### Order operations

| Operation        | Perpetuals | Spot | Notes                                  |
|------------------|------------|------|----------------------------------------|
| Submit order     | ✓          | ✓    | Single order submission.               |
| Submit order list| ✓          | ✓    | Batch order submission.                |
| Modify order     | ✓          | ✓    | Price and quantity modification.       |
| Cancel order     | ✓          | ✓    | Cancel by client order ID.             |
| Cancel all orders| ✓          | ✓    | Cancel all orders for instrument/side. |
| Batch cancel     | ✓          | ✓    | Cancel multiple orders in one request. |

## Configuration

### Data client configuration options

| Option                   | Default | Description                                      |
|--------------------------|---------|--------------------------------------------------|
| `base_url_http`          | `None`  | Override for the REST base URL.                  |
| `base_url_ws`            | `None`  | Override for the WebSocket base URL.             |
| `testnet`                | `False` | Connect to the Hyperliquid testnet when `True`.  |
| `http_timeout_secs`      | `10`    | Timeout (seconds) applied to REST calls.         |
| `http_proxy_url`         | `None`  | Optional HTTP proxy URL.                         |
| `ws_proxy_url`           | `None`  | Optional WebSocket proxy URL.                    |

### Execution client configuration options

| Option                   | Default | Description                                                |
|--------------------------|---------|-------------------------------------------------------------|
| `private_key`            | `None`  | EVM private key; loaded from `HYPERLIQUID_PK` or `HYPERLIQUID_TESTNET_PK` when omitted. |
| `vault_address`          | `None`  | Vault address for delegated trading; loaded from `HYPERLIQUID_VAULT` or `HYPERLIQUID_TESTNET_VAULT` when omitted. |
| `base_url_http`          | `None`  | Override for the REST base URL.                             |
| `base_url_ws`            | `None`  | Override for the WebSocket base URL.                        |
| `testnet`                | `False` | Connect to the Hyperliquid testnet when `True`.             |
| `max_retries`            | `None`  | Maximum retry attempts for order submission/cancel/modify.  |
| `retry_delay_initial_ms` | `None`  | Initial delay (milliseconds) between retries.               |
| `retry_delay_max_ms`     | `None`  | Maximum delay (milliseconds) between retries.               |
| `http_timeout_secs`      | `10`    | Timeout (seconds) applied to REST calls.                    |
| `http_proxy_url`         | `None`  | Optional HTTP proxy URL.                                    |
| `ws_proxy_url`           | `None`  | Optional WebSocket proxy URL.                               |

### Configuration example

```python
from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    data_clients={
        HYPERLIQUID: HyperliquidDataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=True,  # Use testnet
        ),
    },
    exec_clients={
        HYPERLIQUID: HyperliquidExecClientConfig(
            private_key=None,  # Loads from HYPERLIQUID_TESTNET_PK env var
            vault_address=None,  # Optional: loads from HYPERLIQUID_TESTNET_VAULT
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=True,  # Use testnet
        ),
    },
)
```

:::note
When `testnet=True`, the adapter automatically uses testnet environment variables
(`HYPERLIQUID_TESTNET_PK` and `HYPERLIQUID_TESTNET_VAULT`) instead of mainnet variables.
:::
