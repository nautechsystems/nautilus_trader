# dYdX

:::info
We are currently working on this integration guide.
:::

dYdX is one of the largest decentralized cryptocurrency exchanges in terms of daily trading volume
for crypto derivative products. dYdX runs on smart contracts on the Ethereum blockchain, and allows
users to trade with no intermediaries. This integration supports live market data ingestion and order
execution with dYdX v4, which is the first version of the protocol to be fully decentralized with no
central components.

## Installation

To install the latest nautilus-trader package along with the `dydx` dependencies using pip, execute:

```
pip install -U "nautilus_trader[dydx]"
```

For installation via poetry, use:

```
poetry add "nautilus_trader[dydx]"
```

## Overview

The following documentation assumes a trader is setting up for both live market
data feeds, and trade execution. The full dYdX integration consists of an assortment of components,
which can be used together or separately depending on the user's needs.

- `DYDXHttpClient`: Low-level HTTP API connectivity
- `DYDXWebSocketClient`: Low-level WebSocket API connectivity
- `DYDXAccountGRPCAPI`: Low-level gRPC API connectivity for account updates
- `DYDXInstrumentProvider`: Instrument parsing and loading functionality
- `DYDXDataClient`: A market data feed manager
- `DYDXExecutionClient`: An account management and trade execution gateway
- `DYDXLiveDataClientFactory`: Factory for dYdX data clients (used by the trading node builder)
- `DYDXLiveExecClientFactory`: Factory for dYdX execution clients (used by the trading node builder)

:::note
Most users will simply define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## Symbology

Only perpetual contracts are available on dYdX. To be consistent with other adapters and to be 
futureproof in case other products become available on dYdX, NautilusTrader appends `-PERP` for all 
available perpetual symbols. For example, the Bitcoin/USD-C perpetual futures contract is identified 
as `BTC-USD-PERP`. The quote currency for all markets is USD-C. Therefore, dYdX abbreviates it to USD.

## Order types

dYdX offers a flexible combination of trigger types, enabling a broader range of Nautilus orders. 
However, the execution engine currently only supports submitting market and limit orders. Stop orders 
and trailing stop orders can be implemented later.

## Short-term and long-term orders

dYdX makes a distinction between short-term orders and long-term orders (or stateful orders).
Short-term orders are meant to be placed immediately and belongs in the same block the order was received.
These orders stay in-memory up to 20 blocks, with only their fill amount and expiry block height being committed to state.
Short-term orders are mainly intended for use by market makers with high throughput or for market orders.

By default, all orders are sent as short-term orders. To construct long-term orders, you can attach a tag to
an order like this:

```python
from nautilus_trader.adapters.dydx.common.common import DYDXOrderTags

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
from nautilus_trader.adapters.dydx.common.common import DYDXOrderTags

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

## Configuration

The product types for each client must be specified in the configurations.

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
from nautilus_trader.adapters.dydx.factories import DYDXLiveDataClientFactory
from nautilus_trader.adapters.dydx.factories import DYDXLiveExecClientFactory
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
In these cases, a _warn and continue_ approach is taken (the instrument will not be available).

## Order books

Order books can be maintained at full depth or top-of-book quotes depending on the
subscription. The venue does not provide quote ticks, but the adapter subscribes to order
book deltas and sends new quote ticks to the `DataEngine` when there is a top-of-book price or size change.
