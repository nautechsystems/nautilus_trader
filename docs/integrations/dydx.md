# dYdX

dYdX is one of the largest decentralized cryptocurrency exchanges in terms of daily trading volume
for crypto derivative products. dYdX runs on smart contracts on the Ethereum blockchain, and allows
users to trade with no intermediaries. This integration supports live market data ingestion and order
execution with dYdX v4, which is the first version of the protocol to be fully decentralized with no
central components.

## Installation

To install NautilusTrader with dYdX support:

```bash
uv pip install "nautilus_trader[dydx]"
```

To build from source with all extras (including dYdX):

```bash
uv sync --all-extras
```

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/dydx/).

## Overview

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The dYdX adapter includes multiple components, which can be used together or separately depending
on the use case.

- `DYDXHttpClient`: Low-level HTTP API connectivity.
- `DYDXWebSocketClient`: Low-level WebSocket API connectivity.
- `DYDXAccountGRPCAPI`: Low-level gRPC API connectivity for account updates.
- `DYDXInstrumentProvider`: Instrument parsing and loading functionality.
- `DYDXDataClient`: A market data feed manager.
- `DYDXExecutionClient`: An account management and trade execution gateway.
- `DYDXLiveDataClientFactory`: Factory for dYdX data clients (used by the trading node builder).
- `DYDXLiveExecClientFactory`: Factory for dYdX execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (as below),
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

Only perpetual contracts are available on dYdX. To be consistent with other adapters and to be
futureproof in case other products become available on dYdX, NautilusTrader appends `-PERP` for all
available perpetual symbols. For example, the Bitcoin/USD-C perpetual futures contract is identified
as `BTC-USD-PERP`. The quote currency for all markets is USD-C. Therefore, dYdX abbreviates it to USD.

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

| Feature              | Perpetuals | Notes                                          |
|--------------------|------------|------------------------------------------------|
| Query positions     | ✓          | Real-time position updates.                    |
| Position mode       | -          | Net position mode only.                       |
| Leverage control    | ✓          | Per-market leverage settings.                 |
| Margin mode         | -          | Cross margin only.                             |

### Order querying

| Feature              | Perpetuals | Notes                                          |
|----------------------|------------|------------------------------------------------|
| Query open orders    | ✓          | List all active orders.                        |
| Query order history  | ✓          | Historical order data.                         |
| Order status updates | ✓          | Real-time order state changes.                |
| Trade history        | ✓          | Execution and fill reports.                   |

### Contingent orders

| Feature             | Perpetuals | Notes                                          |
|---------------------|------------|------------------------------------------------|
| Order lists         | -          | *Not supported*.                               |
| OCO orders          | -          | *Not supported*.                               |
| Bracket orders      | -          | *Not supported*.                               |
| Conditional orders  | ✓          | Stop market and stop limit orders.           |

### Order classification

dYdX classifies orders as either **short-term** or **long-term** orders:

- **Short-term orders**: Default for all orders; intended for high-frequency trading and market orders.
- **Long-term orders**: Required for conditional orders; use `DYDXOrderTags` to specify.

## Configuration

The product types for each client must be specified in the configurations.

### Data client configuration options

| Option                           | Default | Description |
|----------------------------------|---------|-------------|
| `wallet_address`                 | `None`  | Wallet address; loaded from `DYDX_WALLET_ADDRESS`/`DYDX_TESTNET_WALLET_ADDRESS` when omitted. |
| `is_testnet`                     | `False` | Connect to the dYdX testnet when `True`. |
| `update_instruments_interval_mins` | `60`  | Interval (minutes) between instrument catalogue refreshes. |
| `max_retries`                    | `None`  | Maximum retry attempts for REST/WebSocket recovery. |
| `retry_delay_initial_ms`         | `None`  | Initial delay (milliseconds) between retries. |
| `retry_delay_max_ms`             | `None`  | Maximum delay (milliseconds) between retries. |

### Execution client configuration options

| Option                   | Default | Description |
|--------------------------|---------|-------------|
| `wallet_address`         | `None`  | Wallet address; loaded from `DYDX_WALLET_ADDRESS`/`DYDX_TESTNET_WALLET_ADDRESS` when omitted. |
| `subaccount`             | `0`     | Subaccount number (dYdX provisions subaccount `0` by default). |
| `mnemonic`               | `None`  | Mnemonic used to derive the signing key; loaded from environment when omitted. |
| `base_url_http`          | `None`  | Override for the REST base URL. |
| `base_url_ws`            | `None`  | Override for the WebSocket base URL. |
| `is_testnet`             | `False` | Connect to the dYdX testnet when `True`. |
| `max_retries`            | `None`  | Maximum retry attempts for order submission/cancel/modify calls. |
| `retry_delay_initial_ms` | `None`  | Initial delay (milliseconds) between retries. |
| `retry_delay_max_ms`     | `None`  | Maximum delay (milliseconds) between retries. |

### Execution clients

The account type must be a margin account to trade the perpetual futures contracts.

The most common use case is to configure a live `TradingNode` to include dYdX
data and execution clients. To achieve this, add a `DYDX` section to your client
configuration(s):

```python
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "DYDX": {
            "wallet_address": "YOUR_DYDX_WALLET_ADDRESS",
            "is_testnet": False,
        },
    },
    exec_clients={
        "DYDX": {
            "wallet_address": "YOUR_DYDX_WALLET_ADDRESS",
            "subaccount": "YOUR_DYDX_SUBACCOUNT_NUMBER"
            "mnemonic": "YOUR_MNEMONIC",
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

There are two options for supplying your credentials to the dYdX clients.
Either pass the corresponding `wallet_address` and `mnemonic` values to the configuration objects, or
set the following environment variables:

For dYdX live clients, you can set:

- `DYDX_WALLET_ADDRESS`
- `DYDX_MNEMONIC`

For dYdX testnet clients, you can set:

- `DYDX_TESTNET_WALLET_ADDRESS`
- `DYDX_TESTNET_MNEMONIC`

:::tip
We recommend using environment variables to manage your credentials.
:::

The data client is using the wallet address to determine the trading fees. The trading fees are used during back tests only.

### Testnets

It's also possible to configure one or both clients to connect to the dYdX testnet.
Simply set the `is_testnet` option to `True` (this is `False` by default):

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "DYDX": {
            "wallet_address": "YOUR_DYDX_WALLET_ADDRESS",
            "is_testnet": True,
        },
    },
    exec_clients={
        "DYDX": {
            "wallet_address": "YOUR_DYDX_WALLET_ADDRESS",
            "subaccount": "YOUR_DYDX_SUBACCOUNT_NUMBER"
            "mnemonic": "YOUR_MNEMONIC",
            "is_testnet": True,
        },
    },
)
```

### Parser warnings

Some dYdX instruments are unable to be parsed into Nautilus objects if they
contain enormous field values beyond what can be handled by the platform.
In these cases, a *warn and continue* approach is taken (the instrument will not be available).

## Order books

Order books can be maintained at full depth or top-of-book quotes depending on the
subscription. The venue does not provide quotes, but the adapter subscribes to order
book deltas and sends new quotes to the `DataEngine` when there is a top-of-book price or size change.

:::info
For additional features or to contribute to the dYdX adapter, please see our
[contributing guide](https://github.com/nautechsystems/nautilus_trader/blob/develop/CONTRIBUTING.md).
:::
