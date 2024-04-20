# Bybit

```{warning}
We are currently working on this integration guide.
```

Founded in 2018, Bybit is one of the largest cryptocurrency exchanges in terms
of daily trading volume, and open interest of crypto assets and crypto
derivative products. This integration supports live market data ingest and order
execution with Bybit.

## Overview

The following documentation assumes a trader is setting up for both live market
data feeds, and trade execution. The full Bybit integration consists of an assortment of components,
which can be used together or separately depending on the users needs.

- `BybitHttpClient` - Low-level HTTP API connectivity
- `BybitWebSocketClient` - Low-level WebSocket API connectivity
- `BybitInstrumentProvider` - Instrument parsing and loading functionality
- `BybitDataClient` - A market data feed manager
- `BybitExecutionClient` - An account management and trade execution gateway
- `BybitLiveDataClientFactory` - Factory for Bybit data clients (used by the trading node builder)
- `BybitLiveExecClientFactory` - Factory for Bybit execution clients (used by the trading node builder)

```{note}
Most users will simply define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
```

## Bybit documentation

Bybit provides extensive documentation for users which can be found in the [Bybit help center](https://www.bybit.com/en/help-center).
It's recommended you also refer to the Bybit documentation in conjunction with this NautilusTrader integration guide.

## Products

A product is an umberalla term for a group of related instrument types.

```{note}
Product is also referred to as `category` in the Bybit v5 API.
```

The following product types are supported on Bybit:

- Spot cryptocurrencies
- Perpetual contracts
- Perpetual inverse contracts
- Futures contracts
- Futures inverse contracts

Options contracts are not currently supported (will be implemented in a future version)

## Symbology

To distinguish between different product types on Bybit, the following instrument ID suffix's are used:

- `-SPOT`: spot cryptocurrencies
- `-LINEAR`: perpeutal and futures contracts
- `-INVERSE`: inverse perpetual and inverse futures contracts
- `-OPTION`: options contracts (not currently supported)

These must be appended to the Bybit raw symbol string to be able to identify the specific
product type for the instrument ID, e.g. the Ether/Tether spot currency pair is identified with:

`ETHUSDT-SPOT`

The BTCUSDT perpetual futures contract is identified with:

`BTCUSDT-LINEAR`

The BTCUSD inverse perpetual futures contract is identified with:

`BTCUSD-INVERSE`

## Order types

```{warning}
Only Market and Limit orders have been tested and are available.
The remaining order types will be added on a best effort basis going forward.
```

|                        | Spot                 | Derivatives (Linear, Inverse, Options)  |
|------------------------|----------------------|-----------------------------------------|
| `MARKET`               | ✓                    | ✓                                       |
| `LIMIT`                | ✓                    | ✓                                       |
| `STOP_MARKET`          |                      |                                         |
| `STOP_LIMIT`           |                      |                                         |
| `TRAILING_STOP_MARKET` |                      |                                         |

## Configuration

The product types for each client must be specified in the configurations.

### Data clients

For data clients, if no product types are specified then all product types will
be loaded and available.

### Execution clients

For execution clients, there is a limitation that
you cannot specify `SPOT` with any of the other derivative product types.

- `CASH` account type will be used for `SPOT` products
- `MARGIN` account type will be used for all other derivative products

The most common use case is to configure a live `TradingNode` to include Bybit
data and execution clients. To achieve this, add a `BYBIT` section to your client
configuration(s):

```python
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "BYBIT": {
            "api_key": "YOUR_BYBIT_API_KEY",
            "api_secret": "YOUR_BYBIT_API_SECRET",
            "base_url_http": None,  # Override with custom endpoint
            "product_types": [BybitProductType.LINEAR]
            "testnet": False,
        },
    },
    exec_clients={
        "BYBIT": {
            "api_key": "YOUR_BYBIT_API_KEY",
            "api_secret": "YOUR_BYBIT_API_SECRET",
            "base_url_http": None,  # Override with custom endpoint
            "product_types": [BybitProductType.LINEAR]
            "testnet": False,
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.bybit.factories import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit.factories import BybitLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory("BYBIT", BybitLiveDataClientFactory)
node.add_exec_client_factory("BYBIT", BybitLiveExecClientFactory)

# Finally build the node
node.build()
```

### API credentials

There are two options for supplying your credentials to the Bybit clients.
Either pass the corresponding `api_key` and `api_secret` values to the configuration objects, or
set the following environment variables:

For Bybit live clients, you can set:
- `BYBIT_API_KEY`
- `BYBIT_API_SECRET`

For Bybit testnet clients, you can set:
- `BYBIT_TESTNET_API_KEY`
- `BYBIT_TESTNET_API_SECRET`

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.

