# dYdX

dYdX is one of the largest decentralized cryptocurrency exchanges for crypto derivative products.
This integration supports live market data ingestion and order execution with **dYdX v4**, the first
fully decentralized version of the protocol running on its own application-specific blockchain (dYdX Chain).
Unlike previous versions, v4 operates entirely on-chain with no central components, using Cosmos SDK
and CometBFT (formerly Tendermint) for consensus.

## Installation

To install NautilusTrader with dYdX support:

```bash
uv pip install "nautilus_trader[dydx]"
```

To build from source with all extras (including dYdX):

```bash
uv sync --all-extras
```

## Overview

This adapter is implemented in Rust with Python bindings. It provides direct integration with dYdX's
Indexer API (REST/WebSocket) for market data and gRPC for transaction submission, without requiring
external client libraries.

### Product support

| Product Type        | Data Feed | Trading | Notes                                        |
|---------------------|-----------|---------|----------------------------------------------|
| Perpetual Futures   | ✓         | ✓       | All perpetuals are USDC-settled on v4.       |
| Spot                | -         | -       | *Not available on dYdX v4*.                  |
| Options             | -         | -       | *Not available on dYdX v4*.                  |

:::note
dYdX v4 exclusively supports perpetual futures contracts. All markets are quoted in USD and settled
in USDC. The protocol does not support spot trading or options.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/dydx/).

## Architecture

The dYdX adapter includes multiple components which can be used together or separately:

- `DYDXHttpClient`: Low-level HTTP API connectivity for Indexer queries.
- `DYDXWebSocketClient`: Low-level WebSocket API connectivity for real-time data.
- `DYDXInstrumentProvider`: Instrument parsing and loading functionality.
- `DYDXDataClient`: Market data feed manager.
- `DYDXExecutionClient`: Account management and trade execution gateway.
- `DYDXLiveDataClientFactory`: Factory for dYdX data clients (used by the trading node builder).
- `DYDXLiveExecClientFactory`: Factory for dYdX execution clients (used by the trading node builder).

:::note
Most users will define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

:::warning First-time account activation
A dYdX v4 trading account (sub-account 0) is created **only after** the wallet’s first deposit or trade.
Until then, every gRPC/Indexer query returns `NOT_FOUND`, so `DYDXExecutionClient.connect()` fails.

**Action →** Before starting a live `TradingNode`, send any positive amount of USDC (≥ 1 wei) or other supported collateral from the same wallet **on the same network** (mainnet / testnet).
Once the transaction has finalised (a few blocks) restart the node; the client will connect cleanly.
:::

## Troubleshooting

### `StatusCode.NOT_FOUND` — account … /0 not found

**Cause** *The wallet/sub-account has never been funded and therefore does not yet exist on-chain.*

**Fix**

1. Deposit any positive amount of USDC to sub-account 0 on the correct network.
2. Wait for finality (≈ 30 s on mainnet, longer on testnet).
3. Restart the `TradingNode`; the connection should now succeed.

:::tip
In unattended deployments, wrap the `connect()` call in an exponential-backoff loop so the client retries until the deposit appears.
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

## Short-term and long-term orders

dYdX makes a distinction between short-term orders and long-term orders (or stateful orders).
Short-term orders are meant to be placed immediately and belongs in the same block the order was received.
These orders stay in-memory up to 20 blocks, with only their fill amount and expiry block height being committed to state.
Short-term orders are mainly intended for use by market makers with high throughput or for market orders.

By default, all orders are sent as short-term orders. To construct long-term orders, you can attach a tag to
an order like this:

```python
from nautilus_trader.adapters.dydx import DYDXOrderTags

order: LimitOrder = self.order_factory.limit(
    instrument_id=self.instrument_id,
    order_side=OrderSide.BUY,
    quantity=self.instrument.make_qty(self.trade_size),
    price=self.instrument.make_price(price),
    time_in_force=TimeInForce.GTD,
    expire_time=self.clock.utc_now() + pd.Timedelta(minutes=10),
    post_only=True,
    emulation_trigger=self.emulation_trigger,
    tags=[DYDXOrderTags(is_short_term_order=False).value],
)
```

To specify the number of blocks that an order is active:

```python
from nautilus_trader.adapters.dydx import DYDXOrderTags

order: LimitOrder = self.order_factory.limit(
    instrument_id=self.instrument_id,
    order_side=OrderSide.BUY,
    quantity=self.instrument.make_qty(self.trade_size),
    price=self.instrument.make_price(price),
    time_in_force=TimeInForce.GTD,
    expire_time=self.clock.utc_now() + pd.Timedelta(seconds=5),
    post_only=True,
    emulation_trigger=self.emulation_trigger,
    tags=[DYDXOrderTags(is_short_term_order=True, num_blocks_open=5).value],
)
```

## Market orders

Market orders require specifying a price to for price slippage protection and use hidden orders.
By setting a price for a market order, you can limit the potential price slippage. For example,
if you set the price of $100 for a market buy order, the order will only be executed if the market price
is at or below $100. If the market price is above $100, the order will not be executed.

Some exchanges, including dYdX, support hidden orders. A hidden order is an order that is not visible
to other market participants, but is still executable. By setting a price for a market order, you can
create a hidden order that will only be executed if the market price reaches the specified price.

If the market price is not specified, a default value of 0 is used.

To specify the price when creating a market order:

```python
order = self.order_factory.market(
    instrument_id=self.instrument_id,
    order_side=OrderSide.BUY,
    quantity=self.instrument.make_qty(self.trade_size),
    time_in_force=TimeInForce.IOC,
    tags=[DYDXOrderTags(is_short_term_order=True, market_order_price=Price.from_str("10_000")).value],
)
```

## Stop limit and stop market orders

Both stop limit and stop market conditional orders can be submitted. dYdX only supports long-term orders
for conditional orders.

## Orders capability

dYdX supports perpetual futures trading with a comprehensive set of order types and execution features.

### Order Types

| Order Type             | Perpetuals | Notes                                   |
|------------------------|------------|-----------------------------------------|
| `MARKET`               | ✓          | Requires price for slippage protection. Quote quantity not supported. |
| `LIMIT`                | ✓          |                                         |
| `STOP_MARKET`          | ✓          | Long-term orders only.                  |
| `STOP_LIMIT`           | ✓          | Long-term orders only.                  |
| `MARKET_IF_TOUCHED`    | -          | *Not supported*.                        |
| `LIMIT_IF_TOUCHED`     | -          | *Not supported*.                        |
| `TRAILING_STOP_MARKET` | -          | *Not supported*.                        |

### Execution Instructions

| Instruction   | Perpetuals | Notes                          |
|---------------|------------|--------------------------------|
| `post_only`   | ✓          | Supported on all order types.  |
| `reduce_only` | ✓          | Supported on all order types.  |

### Time in force options

| Time in force| Perpetuals | Notes                |
|--------------|------------|----------------------|
| `GTC`        | ✓          | Good Till Canceled.  |
| `GTD`        | ✓          | Good Till Date.      |
| `FOK`        | ✓          | Fill or Kill.        |
| `IOC`        | ✓          | Immediate or Cancel. |

### Advanced Order Features

| Feature            | Perpetuals | Notes                                          |
|--------------------|------------|------------------------------------------------|
| Order Modification | ✓          | Short-term orders only; cancel-replace method. |
| Bracket/OCO Orders | -          | *Not supported*.                               |
| Iceberg Orders     | -          | *Not supported*.                               |

### Batch operations

| Operation          | Perpetuals | Notes                                          |
|--------------------|------------|------------------------------------------------|
| Batch Submit       | -          | *Not supported*.                               |
| Batch Modify       | -          | *Not supported*.                               |
| Batch Cancel       | -          | *Not supported*.                               |

### Position management

| Feature              | Perpetuals | Notes                                        |
|--------------------|------------|------------------------------------------------|
| Query positions     | ✓          | Real-time position updates.                   |
| Position mode       | -          | Net position mode only.                       |
| Leverage control    | ✓          | Per-market leverage settings.                 |
| Margin mode         | -          | Cross margin only.                            |

### Order querying

| Feature              | Perpetuals | Notes                                        |
|----------------------|------------|----------------------------------------------|
| Query open orders    | ✓          | List all active orders.                      |
| Query order history  | ✓          | Historical order data.                       |
| Order status updates | ✓          | Real-time order state changes.               |
| Trade history        | ✓          | Execution and fill reports.                  |

### Contingent orders

| Feature             | Perpetuals | Notes                                         |
|---------------------|------------|-----------------------------------------------|
| Order lists         | -          | *Not supported*.                              |
| OCO orders          | -          | *Not supported*.                              |
| Bracket orders      | -          | *Not supported*.                              |
| Conditional orders  | ✓          | Stop market and stop limit orders.            |

### Order classification

dYdX classifies orders as either **short-term** or **long-term** orders:

- **Short-term orders**: Default for all orders; intended for high-frequency trading and market orders.
- **Long-term orders**: Required for conditional orders; use `DYDXOrderTags` to specify.

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
        "DYDX": {
            "wallet_address": "YOUR_WALLET_ADDRESS",
            "subaccount": 0,  # Default subaccount
            "mnemonic": "YOUR_MNEMONIC",
        },
    },
)
```

:::note
Most users will use subaccount `0` (the default). Advanced users can configure multiple execution
clients for different subaccounts to implement strategy segregation or risk isolation.
:::

## Configuration

Configure the dYdX adapter through the trading node configuration. Both data and execution clients
support environment variable fallbacks for credentials and network-specific settings.

### Data client configuration options

| Option                             | Default | Description |
|------------------------------------|---------|-------------|
| `wallet_address`                   | `None`  | Wallet address for fee calculation. Falls back to `DYDX_WALLET_ADDRESS` (mainnet) or `DYDX_TESTNET_WALLET_ADDRESS` (testnet). |
| `is_testnet`                       | `False` | Connect to dYdX testnet when `True`. |
| `update_instruments_interval_mins` | `60`    | Interval (minutes) between instrument catalog refreshes. |
| `max_retries`                      | `3`     | Maximum retry attempts for REST/WebSocket recovery. |
| `retry_delay_initial_ms`           | `1000`  | Initial delay (milliseconds) between retries. |
| `retry_delay_max_ms`               | `60000` | Maximum delay (milliseconds) between retries. |

### Execution client configuration options

| Option                   | Default | Description |
|--------------------------|---------|-------------|
| `wallet_address`         | `None`  | Wallet address for the account. Falls back to `DYDX_WALLET_ADDRESS` (mainnet) or `DYDX_TESTNET_WALLET_ADDRESS` (testnet). |
| `subaccount`             | `0`     | Subaccount number (0-127). Subaccount 0 is the default. |
| `mnemonic`               | `None`  | BIP-39 mnemonic for transaction signing. Falls back to `DYDX_MNEMONIC` (mainnet) or `DYDX_TESTNET_MNEMONIC` (testnet). |
| `is_testnet`             | `False` | Connect to dYdX testnet when `True`. |
| `max_retries`            | `3`     | Maximum retry attempts for order operations. |
| `retry_delay_initial_ms` | `1000`  | Initial delay (milliseconds) between retries. |
| `retry_delay_max_ms`     | `60000` | Maximum delay (milliseconds) between retries. |

:::note
**Environment variable resolution**: The adapter automatically resolves credentials from environment
variables based on the `is_testnet` setting. This allows secure credential management without
hardcoding sensitive values in configuration files.
:::

### Basic setup

Configure a live `TradingNode` to include dYdX data and execution clients by adding a `DYDX`
section to your client configurations:

```python
from nautilus_trader.config import TradingNodeConfig

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "DYDX": {
            "wallet_address": "dydx1...",  # Or use environment variable
            "is_testnet": False,
        },
    },
    exec_clients={
        "DYDX": {
            "wallet_address": "dydx1...",  # Or use environment variable
            "subaccount": 0,
            "mnemonic": "word1 word2 ...",  # Or use environment variable
            "is_testnet": False,
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.dydx import DYDXLiveDataClientFactory
from nautilus_trader.adapters.dydx import DYDXLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory("DYDX", DYDXLiveDataClientFactory)
node.add_exec_client_factory("DYDX", DYDXLiveExecClientFactory)

# Finally build the node
node.build()
```

### API credentials

The dYdX adapter supports two methods for supplying credentials:

1. **Direct configuration**: Pass `wallet_address` and `mnemonic` in the config
2. **Environment variables**: Set environment variables (recommended for security)

#### Environment variables

The adapter automatically selects environment variables based on the `is_testnet` setting:

**Mainnet:**

- `DYDX_WALLET_ADDRESS` - Your dYdX wallet address
- `DYDX_MNEMONIC` - BIP-39 mnemonic phrase for signing

**Testnet:**

- `DYDX_TESTNET_WALLET_ADDRESS` - Your testnet wallet address
- `DYDX_TESTNET_MNEMONIC` - Testnet mnemonic phrase

:::tip
Use environment variables for credential management. This keeps sensitive information out of
configuration files and source control.
:::

:::note
The data client uses the wallet address to determine trading fees for backtesting purposes. This
does not affect live trading fee calculation, which is determined by the blockchain state.
:::

### Testnet configuration

Configure clients to connect to the dYdX testnet by setting `is_testnet: True`:

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "DYDX": {
            "is_testnet": True,  # Will use DYDX_TESTNET_* environment variables
        },
    },
    exec_clients={
        "DYDX": {
            "subaccount": 0,
            "is_testnet": True,  # Will use DYDX_TESTNET_* environment variables
        },
    },
)
```

:::warning
Ensure you have testnet credentials in the appropriate environment variables (`DYDX_TESTNET_WALLET_ADDRESS`
and `DYDX_TESTNET_MNEMONIC`) before connecting to testnet.
:::

### Parser warnings

Some dYdX instruments are unable to be parsed into Nautilus objects if they
contain enormous field values beyond what can be handled by the platform.
In these cases, a *warn and continue* approach is taken (the instrument will not be available).

## Order books

Order books can be maintained at full depth or top-of-book quotes depending on the
subscription. The venue does not provide quotes, but the adapter subscribes to order
book deltas and sends new quotes to the `DataEngine` when there is a top-of-book price or size change.

## Contributing

:::info
For additional features or to contribute to the dYdX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
