# dYdX v4

dYdX is one of the largest decentralized cryptocurrency exchanges for crypto derivative products.
This integration supports live market data ingestion and order execution with dYdX v4, running on
its own Cosmos SDK application-specific blockchain (dYdX Chain) with CometBFT consensus. The order
book and matching engine run on-chain as part of the validator process. Orders are submitted as
Cosmos transactions via gRPC and settled each block. An Indexer service exposes REST and WebSocket
APIs for market data and account state.

This is the Rust-backed adapter with Python bindings. For the legacy pure-Python adapter, see
[dYdX v3](dydx_v3.md).

## Installation

:::note
No additional installation extras are required. The adapter is implemented in Rust and
compiled into the core `nautilus_trader` package automatically during the build.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/dydx/).

## Overview

This adapter is implemented in Rust with Python bindings via PyO3. It provides direct integration
with dYdX's Indexer API (REST/WebSocket) for market data and gRPC for Cosmos SDK transaction
submission, without requiring external client libraries.

### Product support

| Product Type      | Data Feed | Trading | Notes                                  |
|-------------------|-----------|---------|----------------------------------------|
| Perpetual Futures | ✓         | ✓       | All perpetuals are USDC-settled on v4. |
| Spot              | -         | -       | *Not available on dYdX v4*.            |
| Options           | -         | -       | *Not available on dYdX v4*.            |

:::note
dYdX v4 exclusively supports perpetual futures contracts. All markets are quoted in USD and settled
in USDC.
:::

## Architecture

The dYdX v4 adapter includes multiple components which can be used together or separately:

- `DydxHttpClient`: Rust-backed HTTP client for Indexer REST API queries.
- `DydxWebSocketClient`: Rust-backed WebSocket client for real-time market data and account updates.
- `DydxGrpcClient`: Rust-backed gRPC client for Cosmos SDK transaction submission.
- `DYDXv4InstrumentProvider`: Instrument parsing and loading functionality.
- `DYDXv4DataClient`: Market data feed manager.
- `DYDXv4ExecutionClient`: Account management and trade execution gateway.
- `DYDXv4LiveDataClientFactory`: Factory for dYdX v4 data clients (used by the trading node builder).
- `DYDXv4LiveExecClientFactory`: Factory for dYdX v4 execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to work with these lower level components directly.
:::

:::warning First-time account activation
A dYdX v4 trading account (sub-account 0) is created only after the wallet's first deposit or trade.
Until then, every gRPC/Indexer query returns `NOT_FOUND`, so `DYDXv4ExecutionClient.connect()` fails.

Before starting a live `TradingNode`, send any positive amount of USDC or other supported collateral
from the same wallet on the same network (mainnet/testnet). Once the transaction has finalised
(a few blocks), restart the node and the client will connect cleanly.
:::

## Troubleshooting

### `StatusCode.NOT_FOUND` account not found

**Cause:** The wallet/sub-account has never been funded and therefore does not yet exist on-chain.

**Fix:**

1. Deposit any positive amount of USDC to sub-account 0 on the correct network.
2. Wait for finality (roughly 30 seconds on mainnet, longer on testnet).
3. Restart the `TradingNode`; the connection should now succeed.

:::tip
In unattended deployments, wrap the `connect()` call in an exponential-backoff loop so the
client retries until the deposit appears.
:::

## Symbology

dYdX v4 uses specific symbol conventions for perpetual futures contracts.

### Symbol format

Format: `{Base}-USD-PERP`

All perpetuals on dYdX v4 are:

- Quoted in USD
- Settled in USDC
- Use the `.DYDX` venue suffix in Nautilus

Examples:

- `BTC-USD-PERP.DYDX` - Bitcoin perpetual futures
- `ETH-USD-PERP.DYDX` - Ethereum perpetual futures
- `SOL-USD-PERP.DYDX` - Solana perpetual futures

To subscribe in your strategy:

```python
InstrumentId.from_str("BTC-USD-PERP.DYDX")
InstrumentId.from_str("ETH-USD-PERP.DYDX")
```

:::info
The `-PERP` suffix is appended for consistency with other adapters and future-proofing. While dYdX v4
currently only supports perpetuals, this naming convention allows for potential expansion to other
product types.
:::

## Orders capability

dYdX supports perpetual futures trading with a comprehensive set of order types and execution
features. The Rust adapter automatically classifies orders as short-term or long-term based on
time-in-force and expiry, so no manual tagging is needed (unlike the legacy Python adapter).

### Order types

| Order Type             | Perpetuals | Notes                                                                   |
|------------------------|------------|-------------------------------------------------------------------------|
| `MARKET`               | ✓          | Immediate execution at best available price.                            |
| `LIMIT`                | ✓          |                                                                         |
| `STOP_MARKET`          | ✓          | Conditional order, always long-term.                                    |
| `STOP_LIMIT`           | ✓          | Conditional order, always long-term.                                    |
| `MARKET_IF_TOUCHED`    | ✓          | Take-profit market order, triggers on price touch.                      |
| `LIMIT_IF_TOUCHED`     | ✓          | Take-profit limit order, triggers on price touch.                       |
| `TRAILING_STOP_MARKET` | -          | *Not supported*.                                                        |

### Execution instructions

| Instruction   | Perpetuals | Notes                                                                        |
|---------------|------------|------------------------------------------------------------------------------|
| `post_only`   | ✓          | Supported on LIMIT, STOP_LIMIT, and LIMIT_IF_TOUCHED orders.                |
| `reduce_only` | ✓          | Supported on all order types except MARKET.                                  |

### Time in force options

| Time in force | Perpetuals | Notes               |
|---------------|------------|----------------------|
| `GTC`         | ✓          | Good Till Canceled.  |
| `GTD`         | ✓          | Good Till Date.      |
| `FOK`         | ✓          | Fill or Kill.        |
| `IOC`         | ✓          | Immediate or Cancel. |

### Advanced order features

| Feature            | Perpetuals | Notes              |
|--------------------|------------|--------------------|
| Order modification | -          | *Not supported*.   |
| Bracket/OCO orders | -          | *Not supported*.   |
| Iceberg orders     | -          | *Not supported*.   |

### Batch operations

| Operation    | Perpetuals | Notes              |
|--------------|------------|--------------------|
| Batch submit | -          | *Not supported*.   |
| Batch modify | -          | *Not supported*.   |
| Batch cancel | ✓          |                    |

### Position management

| Feature         | Perpetuals | Notes                         |
|-----------------|------------|-------------------------------|
| Query positions | ✓          | Real-time position updates.   |
| Position mode   | -          | Net position mode only.       |
| Leverage control| ✓          | Per-market leverage settings.  |
| Margin mode     | -          | Cross margin only.            |

### Order querying

| Feature              | Perpetuals | Notes                          |
|----------------------|------------|--------------------------------|
| Query open orders    | ✓          | List all active orders.        |
| Query order history  | ✓          | Historical order data.         |
| Order status updates | ✓          | Real-time order state changes. |
| Trade history        | ✓          | Execution and fill reports.    |

### Contingent orders

| Feature            | Perpetuals | Notes                                             |
|--------------------|------------|---------------------------------------------------|
| Order lists        | -          | *Not supported*.                                  |
| OCO orders         | -          | *Not supported*.                                  |
| Bracket orders     | -          | *Not supported*.                                  |
| Conditional orders | ✓          | Stop, take-profit market, and take-profit limit.  |

### Order classification

dYdX classifies orders as either **short-term** or **long-term**:

- **Short-term orders**: In-memory orders that expire by block height (max 20 blocks). Used
  for IOC/FOK orders or orders expiring within 60 seconds. Lower fees.
- **Long-term orders**: Stored on-chain with timestamp-based expiry. Used for GTC/GTD orders
  with expiry beyond 60 seconds.
- **Conditional orders**: Stop-loss and take-profit orders, always stored on-chain.

The Rust adapter automatically determines the order lifetime based on the order's time-in-force
and expiry time.

## Data subscriptions

The v4 adapter supports the following data subscriptions:

| Data type           | Subscription | Historical request | Notes                                       |
|---------------------|--------------|--------------------|---------------------------------------------|
| Trade ticks         | ✓            | ✓                  |                                             |
| Quote ticks         | ✓            | -                  | Synthesized from order book top-of-book.    |
| Order book deltas   | ✓            | ✓                  | L2 depth only. Snapshot via HTTP request.   |
| Bars                | ✓            | ✓                  | See supported resolutions below.            |
| Mark prices         | ✓            | -                  | Via markets channel.                        |
| Index prices        | ✓            | -                  | Via markets channel.                        |
| Funding rates       | ✓            | -                  | Via markets channel.                        |
| Instrument status   | ✓            | -                  | Via markets channel.                        |

### Supported bar resolutions

| Resolution | dYdX candle |
|------------|-------------|
| 1-MINUTE   | `1MIN`      |
| 5-MINUTE   | `5MINS`     |
| 15-MINUTE  | `15MINS`    |
| 30-MINUTE  | `30MINS`    |
| 1-HOUR     | `1HOUR`     |
| 4-HOUR     | `4HOURS`    |
| 1-DAY      | `1DAY`      |

## Subaccounts

dYdX v4 supports multiple subaccounts per wallet address, allowing segregation of trading strategies
and risk management within a single wallet.

### Key concepts

- Each wallet address can have multiple numbered subaccounts (0, 1, 2, ..., 127)
- Subaccount 0 is the **default** and is automatically created on first deposit
- Each subaccount maintains its own:
  - Positions
  - Open orders
  - Collateral balance
  - Margin requirements

### Configuration

Specify the subaccount number in the execution client config:

```python
config = TradingNodeConfig(
    exec_clients={
        "DYDX": DYDXv4ExecClientConfig(
            subaccount=0,  # Default subaccount
        ),
    },
)
```

:::note
Most users will use subaccount `0` (the default). Advanced users can configure multiple execution
clients for different subaccounts to implement strategy segregation or risk isolation.
:::

## Configuration

Configure the dYdX v4 adapter through the trading node configuration. Both data and execution
clients support environment variable fallbacks for credentials and network-specific settings.

### Data client configuration options

| Option                    | Default | Description                                                                              |
|---------------------------|---------|------------------------------------------------------------------------------------------|
| `is_testnet`              | `False` | Connect to dYdX testnet when `True`.                                                     |
| `bars_timestamp_on_close` | `True`  | Use bar close time for `ts_event` timestamps. Set `False` to use venue-native open time. |
| `base_url_http`           | `None`  | HTTP API endpoint override.                                                              |
| `base_url_ws`             | `None`  | WebSocket endpoint override.                                                             |
| `max_retries`             | `3`     | Maximum retry attempts for REST/WebSocket recovery.                                      |
| `retry_delay_initial_ms`  | `1000`  | Initial delay (milliseconds) between retries.                                            |
| `retry_delay_max_ms`      | `10000` | Maximum delay (milliseconds) between retries.                                            |

### Execution client configuration options

| Option                   | Default | Description                                                                           |
|--------------------------|---------|---------------------------------------------------------------------------------------|
| `subaccount`             | `0`     | Subaccount number (0-127). Subaccount 0 is the default.                               |
| `authenticator_ids`      | `None`  | List of authenticator IDs for permissioned key trading (institutional setups).         |
| `is_testnet`             | `False` | Connect to dYdX testnet when `True`.                                                  |
| `base_url_http`          | `None`  | HTTP client custom endpoint override.                                                 |
| `base_url_ws`            | `None`  | WebSocket client custom endpoint override.                                            |
| `base_url_grpc`          | `None`  | gRPC client custom endpoint override. Supports fallback with multiple URLs.           |
| `max_retries`            | `3`     | Maximum retry attempts for order operations.                                          |
| `retry_delay_initial_ms` | `1000`  | Initial delay (milliseconds) between retries.                                         |
| `retry_delay_max_ms`     | `10000` | Maximum delay (milliseconds) between retries.                                         |

:::note
Credentials (`wallet_address` and `private_key`) are resolved by the Rust layer directly from
environment variables based on the `is_testnet` setting. They are not passed through the Python
configuration.
:::

### Basic setup

Configure a live `TradingNode` to include dYdX v4 data and execution clients:

```python
from nautilus_trader.adapters.dydx_v4 import DYDXv4DataClientConfig
from nautilus_trader.adapters.dydx_v4 import DYDXv4ExecClientConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "DYDX": DYDXv4DataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,
        ),
    },
    exec_clients={
        "DYDX": DYDXv4ExecClientConfig(
            subaccount=0,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,
        ),
    },
)
```

Then, create a `TradingNode` and register the client factories:

```python
from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveDataClientFactory
from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveExecClientFactory
from nautilus_trader.adapters.dydx_v4.constants import DYDX
from nautilus_trader.live.node import TradingNode

node = TradingNode(config=config)

node.add_data_client_factory(DYDX, DYDXv4LiveDataClientFactory)
node.add_exec_client_factory(DYDX, DYDXv4LiveExecClientFactory)

node.build()
```

### API credentials

Credentials are resolved by the Rust layer from environment variables. The Python layer does
not handle credentials directly.

#### Environment variables

The adapter selects environment variables based on the `is_testnet` setting:

**Mainnet:**

- `DYDX_WALLET_ADDRESS` - Your dYdX wallet address
- `DYDX_PRIVATE_KEY` - Hex-encoded private key for signing

**Testnet:**

- `DYDX_TESTNET_WALLET_ADDRESS` - Your testnet wallet address
- `DYDX_TESTNET_PRIVATE_KEY` - Testnet private key

### Testnet configuration

Configure clients to connect to the dYdX testnet by setting `is_testnet=True`:

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "DYDX": DYDXv4DataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=True,
        ),
    },
    exec_clients={
        "DYDX": DYDXv4ExecClientConfig(
            subaccount=0,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=True,
        ),
    },
)
```

:::warning
Ensure you have testnet credentials in the appropriate environment variables
(`DYDX_TESTNET_WALLET_ADDRESS` and `DYDX_TESTNET_PRIVATE_KEY`) before connecting to testnet.
:::

### Permissioned key trading

For institutional setups with separated hot/cold wallet architectures, the adapter supports
permissioned key trading via authenticator IDs. When provided, transactions include a TxExtension
to enable trading via sub-accounts using delegated signing keys.

```python
config = DYDXv4ExecClientConfig(
    authenticator_ids=[1, 2],  # Your authenticator IDs
    is_testnet=False,
)
```

## Order books

Order books can be maintained at full depth or top-of-book quotes depending on the subscription.
The venue does not provide quotes directly. Instead, the adapter subscribes to order book deltas
and synthesizes quotes for the `DataEngine` when there is a top-of-book price or size change.
Only L2 (MBP) book type is supported.

## Contributing

:::info
For additional features or to contribute to the dYdX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
