# Hyperliquid

[Hyperliquid](https://hyperliquid.gitbook.io/hyperliquid-docs) is a decentralized perpetual futures
and spot exchange built on the Hyperliquid L1, a purpose-built blockchain optimized for trading.
HyperCore provides a fully on-chain order book and matching engine. This integration supports
live market data feeds and order execution on Hyperliquid.

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

## Builder fees

This integration is free and open source. NautilusTrader participates in the Hyperliquid
[Builder Codes](https://hyperliquid.gitbook.io/hyperliquid-docs/trading/builder-codes) program,
which routes a small 1 basis point (0.01%) fee on fills to support ongoing development and maintenance.
This fee is charged by Hyperliquid in addition to standard fees, and applies to perpetuals and spot sells only.

:::info
This builder fee is at the low end of ecosystem norms (Hyperliquid allows up to 0.1% (10 bps) for perps and 1% (100 bps) for spot).
See [Hyperliquid Builder Codes](https://hyperliquid.gitbook.io/hyperliquid-docs/trading/builder-codes)
and [Hyperliquid Fees](https://hyperliquid.gitbook.io/hyperliquid-docs/trading/fees) for details.
:::

### Checking approval status

You can check whether your wallet has approved the builder fee:

```bash
# Check by wallet address (no private key required)
python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_verify.py 0xYourWalletAddress

# Or derive address from private key env var
python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_verify.py
```

This queries the Hyperliquid API to verify your approval status.

### Approving builder fees

Before you can trade on Hyperliquid via NautilusTrader, you must approve the builder fee.
This is a **one-time** setup step per wallet address, per network.

:::warning
You must sign the approval with your **main wallet** private key (the same key used for trading).
This cannot be done with an API key or agent wallet.
:::

#### Running the approval script

The script reads your private key from environment variables (`HYPERLIQUID_PK` or `HYPERLIQUID_TESTNET_PK`).
It prompts for confirmation before submitting.

```bash
# Mainnet (uses HYPERLIQUID_PK)
python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_approve.py

# Testnet (uses HYPERLIQUID_TESTNET_PK)
HYPERLIQUID_TESTNET=true python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_approve.py
```

The script outputs confirmation of the approval. Once approved, all subsequent orders
placed through NautilusTrader include the builder fee automatically.

:::note
The approval authorizes a 0.01% (1 basis point) fee rate which applies to perpetuals and spot sells.
:::

:::note
You only need to run this script **once** per wallet per network. The approval persists until you
explicitly revoke it.
:::

### Revoking approval

If you need to revoke the builder fee approval, the script reads from the same environment
variables as the approval script (`HYPERLIQUID_PK` or `HYPERLIQUID_TESTNET_PK`).
The script prompts for confirmation before submitting.

```bash
# Mainnet (uses HYPERLIQUID_PK)
python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_revoke.py

# Testnet (uses HYPERLIQUID_TESTNET_PK)
HYPERLIQUID_TESTNET=true python nautilus_trader/adapters/hyperliquid/scripts/builder_fee_revoke.py
```

:::warning
After revoking, you will not be able to trade on Hyperliquid via NautilusTrader until you re-approve.
:::

### Troubleshooting

**API error related to builder fee approval**

If you see an error mentioning "builder fee" when placing orders, this indicates the builder fee
has not been approved for your wallet. Run the approval script as described above to resolve this.

You can verify your approval status at any time using the [verify script](#checking-approval-status).

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/hyperliquid/).

## Testnet setup

Hyperliquid provides a testnet environment for testing strategies without risking real funds.

### Getting testnet funds

To receive testnet USDC, you must first have deposited on **mainnet** using the same wallet address:

1. Visit the [Hyperliquid mainnet portal](https://app.hyperliquid.xyz/) and make a deposit with your wallet.
2. Visit the [testnet faucet](https://app.hyperliquid-testnet.xyz/drip) using the same wallet.
3. Claim 1,000 mock USDC from the faucet.

:::note
**Email wallet users**: Email login generates different addresses for mainnet vs testnet.
To use the faucet, export your email wallet from mainnet, import it into MetaMask or Rabby,
then connect the extension to testnet.
:::

### Creating a testnet account

1. Visit the [Hyperliquid testnet portal](https://app.hyperliquid-testnet.xyz/).
2. Connect your wallet (MetaMask, WalletConnect, or email).
3. The testnet automatically creates an account for your wallet address.

### Exporting your private key

To use your testnet account with NautilusTrader, you need to export your wallet's private key:

**MetaMask:**

1. Click the three dots menu next to your account.
2. Select "Account details".
3. Click "Show private key".
4. Enter your password and copy the private key.

:::warning
**Never share your private keys.**
Store private keys securely using environment variables; never commit them to version control.
:::

### Setting environment variables

Set your testnet credentials as environment variables:

```bash
export HYPERLIQUID_TESTNET_PK="your_private_key_here"
# Optional: for vault trading
export HYPERLIQUID_TESTNET_VAULT="vault_address_here"
```

The adapter automatically loads these when `testnet=True` in the configuration.

## Product support

Hyperliquid offers linear perpetual futures and native spot markets.

| Product Type        | Data Feed | Trading | Notes                      |
|---------------------|-----------|---------|----------------------------|
| Perpetual Futures   | ✓         | ✓       | USDC-settled linear perps. |
| Spot                | ✓         | ✓       | Native spot markets.       |

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

## Order books

Order books can be maintained at full depth based on WebSocket subscription.
Hyperliquid provides real-time order book updates via WebSocket streams.

Order book snapshot rebuilds are triggered on:

- Initial subscription of the order book data.
- WebSocket reconnects.

:::note
There is a limitation of one order book per instrument per trader instance.
:::

## API credentials

There are two options for supplying your credentials to the Hyperliquid clients.
Either pass the corresponding values to the configuration objects, or
set the following environment variables:

For Hyperliquid mainnet clients, you can set:

- `HYPERLIQUID_PK`
- `HYPERLIQUID_VAULT` (optional, for vault trading)

For Hyperliquid testnet clients, you can set:

- `HYPERLIQUID_TESTNET_PK`
- `HYPERLIQUID_TESTNET_VAULT` (optional, for vault trading)

:::tip
We recommend using environment variables to manage your credentials.
:::

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

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
node.add_exec_client_factory(HYPERLIQUID, HyperliquidLiveExecClientFactory)

# Finally build the node
node.build()
```

## Contributing

:::info
For additional features or to contribute to the Hyperliquid adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
