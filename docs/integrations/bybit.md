# Bybit

:::info
We are currently working on this integration guide.
:::

Founded in 2018, Bybit is one of the largest cryptocurrency exchanges in terms
of daily trading volume, and open interest of crypto assets and crypto
derivative products. This integration supports live market data ingest and order
execution with Bybit.

## Installation

To install the latest `nautilus_trader` package along with the `bybit` dependencies using pip:

```
pip install -U "nautilus_trader[bybit]"
```

To install from source using uv:

```
uv sync --extra bybit
```

## Examples

You can find functional live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/bybit/).

## Overview

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The Bybit adapter includes multiple components, which can be used together or separately depending
on the use case.

- `BybitHttpClient`: Low-level HTTP API connectivity.
- `BybitWebSocketClient`: Low-level WebSocket API connectivity.
- `BybitInstrumentProvider`: Instrument parsing and loading functionality.
- `BybitDataClient`: A market data feed manager.
- `BybitExecutionClient`: An account management and trade execution gateway.
- `BybitLiveDataClientFactory`: Factory for Bybit data clients (used by the trading node builder).
- `BybitLiveExecClientFactory`: Factory for Bybit execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## Bybit documentation

Bybit provides extensive documentation for users which can be found in the [Bybit help center](https://www.bybit.com/en/help-center).
It’s recommended you also refer to the Bybit documentation in conjunction with this NautilusTrader integration guide.

## Products

A product is an umbrella term for a group of related instrument types.

:::note
Product is also referred to as `category` in the Bybit v5 API.
:::

The following product types are supported on Bybit:

- Spot cryptocurrencies
- Perpetual contracts
- Perpetual inverse contracts
- Futures contracts
- Futures inverse contracts

Options contracts are not currently supported (will be implemented in a future version)

## Symbology

To distinguish between different product types on Bybit, Nautilus uses specific product category suffixes for symbols:

- `-SPOT`: Spot cryptocurrencies
- `-LINEAR`: Perpetual and futures contracts
- `-INVERSE`: Inverse perpetual and inverse futures contracts
- `-OPTION`: Options contracts (not currently supported)

These suffixes must be appended to the Bybit raw symbol string to identify the specific product type
for the instrument ID. For example:

- The Ether/Tether spot currency pair is identified with `-SPOT`, such as `ETHUSDT-SPOT`.
- The BTCUSDT perpetual futures contract is identified with `-LINEAR`, such as `BTCUSDT-LINEAR`.
- The BTCUSD inverse perpetual futures contract is identified with `-INVERSE`, such as `BTCUSD-INVERSE`.

## Order types

Bybit offers a flexible combination of trigger types, enabling a broader range of Nautilus orders.
All the order types listed below can be used as *either* entries or exits, except for trailing stops
(which utilize a position-related API).

|                        | Spot                 | Derivatives (Linear, Inverse, Options)  |
|------------------------|----------------------|-----------------------------------------|
| `MARKET`               | ✓                    | ✓                                       |
| `LIMIT`                | ✓                    | ✓                                       |
| `STOP_MARKET`          | ✓                    | ✓                                       |
| `STOP_LIMIT`           | ✓                    | ✓                                       |
| `MARKET_IF_TOUCHED`    | ✓                    | ✓                                       |
| `LIMIT_IF_TOUCHED`     | ✓                    | ✓                                       |
| `TRAILING_STOP_MARKET` | Not supported        | ✓                                       |

### Limitations for SPOT

The following limitations apply to SPOT products, as positions are not tracked on the venue side:

- `reduce_only` orders are not supported
- Trailing stop orders are not supported

### Trailing stops

Trailing stops on Bybit do not have a client order ID on the venue side (though there is a `venue_order_id`).
This is because trailing stops are associated with a netted position for an instrument.
Consider the following points when using trailing stops on Bybit:

- `reduce_only` instruction is available
- When the position associated with a trailing stop is closed, the trailing stop is automatically "deactivated" (closed) on the venue side
- You cannot query trailing stop orders that are not already open (the `venue_order_id` is unknown until then)
- You can manually adjust the trigger price in the GUI, which will update the Nautilus order

## Configuration

The product types for each client must be specified in the configurations.

### Data clients

If no product types are specified then all product types will be loaded and available.

### Execution clients

Because Nautilus does not support a "unified" account, the account type must be either cash **or** margin.
This means there is a limitation that you cannot specify SPOT with any of the other derivative product types.

- `CASH` account type will be used for SPOT products.
- `MARGIN` account type will be used for all other derivative products.

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

For Bybit demo clients, you can set:
- `BYBIT_DEMO_API_KEY`
- `BYBIT_DEMO_API_SECRET`

For Bybit testnet clients, you can set:
- `BYBIT_TESTNET_API_KEY`
- `BYBIT_TESTNET_API_SECRET`

:::tip
We recommend using environment variables to manage your credentials.
:::

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.
