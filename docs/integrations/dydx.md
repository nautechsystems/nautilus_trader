# dYdX

:::info
We are currently working on this integration guide.
:::

dYdX is one of the largest decentralized cryptocurrency exchanges in terms of daily trading volume
for crypto derivative products. dYdX runs on smart contracts on the Ethereum blockchain, and allows
users to trade with no intermediaries. This integration supports live market data ingestion and order
execution with dYdX v4, which is the first version of the protocol to be fully decentralized with no
central components.

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

Order books can be maintained at full depths or top of the book quotes depending on the
subscription options. The venue does not provide quote ticks, but the adapter subscribes to order
book deltas and sends new quote ticks to the `DataEngine` when the best bid or ask price or size changes.
