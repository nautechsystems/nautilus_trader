# Binance

Founded in 2017, Binance is one of the largest cryptocurrency exchanges in terms
of daily trading volume, and open interest of crypto assets and crypto
derivative products. This integration supports live market data ingest and order
execution with Binance.

## Installation

To install the latest `nautilus_trader` package along with the `binance` dependencies using pip:

```
pip install -U "nautilus_trader[binance]"
```

To install from source using uv:

```
uv sync --extra binance
```

## Examples

You can find functional live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/binance/).

## Overview

The Binance integration supports the following product types:

- Spot markets (including Binance US)
- USDT-Margined Futures
- Coin-Margined Futures

:::note
Margin accounts are not fully supported at this time due to limited developer testing.
Contributions via [GitHub issue](https://github.com/nautechsystems/nautilus_trader/issues) reports
or pull requests to enhance margin account functionality are encouraged.
:::

This guide assumes a trader is setting up for both live market data feeds, and trade execution.
The Binance adapter includes multiple components, which can be used together or separately depending
on the use case.

- `BinanceHttpClient`: Low-level HTTP API connectivity.
- `BinanceWebSocketClient`: Low-level WebSocket API connectivity.
- `BinanceInstrumentProvider`: Instrument parsing and loading functionality.
- `BinanceSpotDataClient`/`BinanceFuturesDataClient`: A market data feed manager.
- `BinanceSpotExecutionClient`/`BinanceFuturesExecutionClient`: An account management and trade execution gateway.
- `BinanceLiveDataClientFactory`: Factory for Binance data clients (used by the trading node builder).
- `BinanceLiveExecClientFactory`: Factory for Binance execution clients (used by the trading node builder).

:::note
Most users will simply define a configuration for a live trading node (as below),
and won't need to necessarily work with these lower level components directly.
:::

## Data types

To provide complete API functionality to traders, the integration includes several
custom data types:

- `BinanceTicker`: Represents data returned for Binance 24-hour ticker subscriptions, including comprehensive price and statistical information.
- `BinanceBar`: Represents data for historical requests or real-time subscriptions to Binance bars, with additional volume metrics.
- `BinanceFuturesMarkPriceUpdate`: Represents mark price updates for Binance Futures subscriptions.

See the Binance [API Reference](../api_reference/adapters/binance.md) for full definitions.

## Symbology

As per the Nautilus unification policy for symbols, the native Binance symbols are used where possible including for
spot assets and futures contracts. Because NautilusTrader is capable of multi-venue + multi-account
trading, it's necessary to explicitly clarify the difference between `BTCUSDT` as the spot and margin traded
pair, and the `BTCUSDT` perpetual futures contract (this symbol is used for _both_ natively by Binance).

Therefore, Nautilus appends the suffix `-PERP` to all perpetual symbols.
E.g. for Binance Futures, the `BTCUSDT` perpetual futures contract symbol would be `BTCUSDT-PERP` within the Nautilus system boundary.

## Order types

|                        | Spot                            | Margin                          | Futures           |
|------------------------|---------------------------------|---------------------------------|-------------------|
| `MARKET`               | ✓                               | ✓                               | ✓                 |
| `LIMIT`                | ✓                               | ✓                               | ✓                 |
| `STOP_MARKET`          | Not supported                   | ✓                               | ✓                 |
| `STOP_LIMIT`           | ✓ (`post-only` not available)   | ✓ (`post-only` not available)   | ✓                 |
| `MARKET_IF_TOUCHED`    | Not supported                   | Not supported                   | ✓                 |
| `LIMIT_IF_TOUCHED`     | ✓                               | ✓                               | ✓                 |
| `TRAILING_STOP_MARKET` | Not supported                   | Not supported                   | ✓                 |

### Trailing stops

Binance uses the concept of an activation price for trailing stops, as detailed in their [documentation](https://www.binance.com/en/support/faq/what-is-a-trailing-stop-order-360042299292).
This approach is somewhat unconventional. For trailing stop orders to function on Binance, the activation price can optionally be set using the `trigger_price` value.

Note that the activation price is **not** the same as the trigger/STOP price. Binance will always calculate the trigger price for the order based on the current market price and the callback rate provided by `trailing_offset`.
The activated price is simply the price at which the order will begin trailing based on the callback rate.

When submitting trailing stop orders from your strategy, you have two options:

1. Use the `trigger_price` to manually set the activation price.
2. Leave the `trigger_price` as `None`, activating the trailing mechanism immediately.

You must also have at least *one* of the following:

- The `trigger_price` for the order is set (this will act as the Binance *activation_price*).
- (or) you have subscribed to quotes for the instrument you're submitting the order for (used to infer activation price).
- (or) you have subscribed to trades for the instrument you're submitting the order for (used to infer activation price).

## Configuration

The most common use case is to configure a live `TradingNode` to include Binance
data and execution clients. To achieve this, add a `BINANCE` section to your client
configuration(s):

```python
from nautilus_trader.live.node import TradingNode

config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "account_type": "spot",  # {spot, margin, usdt_future, coin_future}
            "base_url_http": None,  # Override with custom endpoint
            "base_url_ws": None,  # Override with custom endpoint
            "us": False,  # If client is for Binance US
        },
    },
    exec_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "account_type": "spot",  # {spot, margin, usdt_future, coin_future}
            "base_url_http": None,  # Override with custom endpoint
            "base_url_ws": None,  # Override with custom endpoint
            "us": False,  # If client is for Binance US
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.live.node import TradingNode

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory("BINANCE", BinanceLiveDataClientFactory)
node.add_exec_client_factory("BINANCE", BinanceLiveExecClientFactory)

# Finally build the node
node.build()
```

### API credentials

There are two options for supplying your credentials to the Binance clients.
Either pass the corresponding `api_key` and `api_secret` values to the configuration objects, or
set the following environment variables:

For Binance Spot/Margin live clients, you can set:
- `BINANCE_API_KEY`
- `BINANCE_API_SECRET`

For Binance Spot/Margin testnet clients, you can set:
- `BINANCE_TESTNET_API_KEY`
- `BINANCE_TESTNET_API_SECRET`

For Binance Futures live clients, you can set:
- `BINANCE_FUTURES_API_KEY`
- `BINANCE_FUTURES_API_SECRET`

For Binance Futures testnet clients, you can set:
- `BINANCE_FUTURES_TESTNET_API_KEY`
- `BINANCE_FUTURES_TESTNET_API_SECRET`

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.

### Account Type

All the Binance account types will be supported for live trading. Set the `account_type`
using the `BinanceAccountType` enum. The account type options are:

- `SPOT`
- `MARGIN` (Margin shared between open positions)
- `ISOLATED_MARGIN` (Margin assigned to a single position)
- `USDT_FUTURE` (USDT or BUSD stablecoins as collateral)
- `COIN_FUTURE` (other cryptocurrency as collateral)

:::tip
We recommend using environment variables to manage your credentials.
:::

### Base url overrides

It's possible to override the default base URLs for both HTTP Rest and
WebSocket APIs. This is useful for configuring API clusters for performance reasons,
or when Binance has provided you with specialized endpoints.

### Binance US

There is support for Binance US accounts by setting the `us` option in the configs
to `True` (this is `False` by default). All functionality available to US accounts
should behave identically to standard Binance.

### Testnets

It's also possible to configure one or both clients to connect to the Binance testnet.
Simply set the `testnet` option to `True` (this is `False` by default):

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_TESTNET_API_KEY",
            "api_secret": "YOUR_BINANCE_TESTNET_API_SECRET",
            "account_type": "spot",  # {spot, margin, usdt_future}
            "testnet": True,  # If client uses the testnet
        },
    },
    exec_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_TESTNET_API_KEY",
            "api_secret": "YOUR_BINANCE_TESTNET_API_SECRET",
            "account_type": "spot",  # {spot, margin, usdt_future}
            "testnet": True,  # If client uses the testnet
        },
    },
)
```

### Aggregated trades

Binance provides aggregated trade data endpoints as an alternative source of trades.
In comparison to the default trade endpoints, aggregated trade data endpoints can return all
ticks between a `start_time` and `end_time`.

To use aggregated trades and the endpoint features, set the `use_agg_trade_ticks` option
to `True` (this is `False` by default.)

### Parser warnings

Some Binance instruments are unable to be parsed into Nautilus objects if they
contain enormous field values beyond what can be handled by the platform.
In these cases, a _warn and continue_ approach is taken (the instrument will not
be available).

These warnings may cause unnecessary log noise, and so it's possible to
configure the provider to not log the warnings, as per the client configuration
example below:

```python
from nautilus_trader.config import InstrumentProviderConfig

instrument_provider=InstrumentProviderConfig(
    load_all=True,
    log_warnings=False,
)
```

### Futures Hedge mode

Binance Futures Hedge mode is a position mode where a trader opens positions in both long and short
directions to mitigate risk and potentially profit from market volatility.

To use Binance Future Hedge mode, you need to follow the three items below:
- 1. Before starting the strategy, ensure that hedge mode is configured on Binance.
- 2. Set the `use_reduce_only` option to `False` in BinanceExecClientConfig (this is `True` by default.)
    ```python
    config = TradingNodeConfig(
        ...,  # Omitted
        data_clients={
            "BINANCE": BinanceDataClientConfig(
                api_key=None,  # 'BINANCE_API_KEY' env var
                api_secret=None,  # 'BINANCE_API_SECRET' env var
                account_type=BinanceAccountType.USDT_FUTURE,
                base_url_http=None,  # Override with custom endpoint
                base_url_ws=None,  # Override with custom endpoint
            ),
        },
        exec_clients={
            "BINANCE": BinanceExecClientConfig(
                api_key=None,  # 'BINANCE_API_KEY' env var
                api_secret=None,  # 'BINANCE_API_SECRET' env var
                account_type=BinanceAccountType.USDT_FUTURE,
                base_url_http=None,  # Override with custom endpoint
                base_url_ws=None,  # Override with custom endpoint
                use_reduce_only=False,  # Must be disabled for Hedge mode
            ),
        }
    )
    ```

- 3. When submitting an order, use a suffix (`LONG` or `SHORT` ) in the `position_id` to indicate the position direction.
    ```python
    class EMACrossHedgeMode(Strategy):
        ...,  # Omitted
        def buy(self) -> None:
            """
            Users simple buy method (example).
            """
            order: MarketOrder = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.instrument.make_qty(self.trade_size),
                # time_in_force=TimeInForce.FOK,
            )

            # LONG suffix is recognized as a long position by Binance adapter.
            position_id = PositionId(f"{self.instrument_id}-LONG")
            self.submit_order(order, position_id)

        def sell(self) -> None:
            """
            Users simple sell method (example).
            """
            order: MarketOrder = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=self.instrument.make_qty(self.trade_size),
                # time_in_force=TimeInForce.FOK,
            )
            # SHORT suffix is recognized as a short position by Binance adapter.
            position_id = PositionId(f"{self.instrument_id}-SHORT")
            self.submit_order(order, position_id)
    ```

## Order books

Order books can be maintained at full or partial depths depending on the
subscription. WebSocket stream throttling is different between Spot and Futures exchanges,
Nautilus will use the highest streaming rate possible:

Order books can be maintained at full or partial depths based on the subscription settings.
WebSocket stream update rates differ between Spot and Futures exchanges, with Nautilus using the
highest available streaming rate:

- **Spot**: 100ms
- **Futures**: 0ms (*unthrottled*)

There is a limitation of one order book per instrument per trader instance.
As stream subscriptions may vary, the latest order book data (deltas or snapshots)
subscription will be used by the Binance data client.

Order book snapshot rebuilds will be triggered on:

- Initial subscription of the order book data
- Data websocket reconnects

The sequence of events is as follows:

- Deltas will start buffered.
- Snapshot is requested and awaited.
- Snapshot response is parsed to `OrderBookDeltas`.
- Snapshot deltas are sent to the `DataEngine`.
- Buffered deltas are iterated, dropping those where the sequence number is not greater than the last delta in the snapshot.
- Deltas will stop buffering.
- Remaining deltas are sent to the `DataEngine`.

## Binance data differences

The `ts_event` field value for `QuoteTick` objects will differ between Spot and Futures exchanges,
where the former does not provide an event timestamp, so the `ts_init` is used (which means `ts_event` and `ts_init` are identical).

## Binance specific data

It's possible to subscribe to Binance specific data streams as they become available to the
adapter over time.

:::note
Bars are not considered 'Binance specific' and can be subscribed to in the normal way.
As more adapters are built out which need for example mark price and funding rate updates, then these
methods may eventually become first-class (not requiring custom/generic subscriptions as below).
:::

### BinanceFuturesMarkPriceUpdate

You can subscribe to `BinanceFuturesMarkPriceUpdate` (including funding rating info)
data streams by subscribing in the following way from your actor or strategy:

```python
from nautilus_trader.adapters.binance.futures.types import BinanceFuturesMarkPriceUpdate
from nautilus_trader.model import DataType
from nautilus_trader.model import ClientId

# In your `on_start` method
self.subscribe_data(
    data_type=DataType(BinanceFuturesMarkPriceUpdate, metadata={"instrument_id": self.instrument.id}),
    client_id=ClientId("BINANCE"),
)
```

This will result in your actor/strategy passing these received `BinanceFuturesMarkPriceUpdate`
objects to your `on_data` method. You will need to check the type, as this
method acts as a flexible handler for all custom/generic data.

```python
from nautilus_trader.core import Data

def on_data(self, data: Data):
    # First check the type of data
    if isinstance(data, BinanceFuturesMarkPriceUpdate):
        # Do something with the data
```
