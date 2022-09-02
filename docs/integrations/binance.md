# Binance

Founded in 2017, Binance is one of the largest cryptocurrency exchanges in terms 
of daily trading volume, and open interest of crypto assets and crypto 
derivative products. This integration supports live market data ingest and order
execution with Binance.

```{warning}
This integration is still under construction. Please consider it to be in an
unstable beta phase and exercise caution.
```

## Overview
The following documentation assumes a trader is setting up for both live market 
data feeds, and trade execution. The full Binance integration consists of an assortment of components, 
which can be used together or separately depending on the users needs.

- `BinanceHttpClient` provides low-level HTTP API connectivity
- `BinanceWebSocketClient` provides low-level WebSocket API connectivity
- `BinanceInstrumentProvider` provides instrument parsing and loading functionality
- `BinanceDataClient` provides a market data feed manager
- `BinanceExecutionClient` provides an account management and trade execution gateway
- `BinanceLiveDataClientFactory` creation factory for Binance data clients (used by the trading node builder)
- `BinanceLiveExecClientFactory` creation factory for Binance execution clients (used by the trading node builder)

```{note}
Most users will simply define a configuration for a live trading node (as below), 
and won't need to necessarily work with these lower level components individually.
```

## Binance data types
To provide complete API functionality to traders, the integration includes several
custom data types:
- `BinanceTicker` returned when subscribing to Binance 24hr tickers (contains many prices and stats).
- `BinanceBar` returned when requesting historical, or subscribing to, Binance bars (contains extra volume information).
- `BinanceFuturesMarkPriceUpdate` returned when subscribing to Binance Futures mark price updates.

See the Binance [API Reference](../api_reference/adapters/binance.md) for full definitions.

## Symbology
As per the Nautilus unification policy for symbols, the native Binance symbols are used where possible including for
spot assets and futures contracts. However, because NautilusTrader is capable of multi-venue + multi-account
trading, it's necessary to explicitly clarify the difference between `BTCUSDT` as the spot and margin traded
pair, and the `BTCUSDT` perpetual futures contract (this symbol is used for _both_ natively by Binance). Therefore, NautilusTrader appends `-PERP` to all native perpetual symbols.
E.g. for Binance Futures, the said instruments symbol is `BTCUSDT-PERP` within the Nautilus system boundary.

```{note}
This convention of appending `-PERP` to perpetual futures is also adopted by [FTX](ftx.md).
```

## Configuration
The most common use case is to configure a live `TradingNode` to include Binance 
data and execution clients. To achieve this, add a `BINANCE` section to your client
configuration(s):

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "account_type": "spot",  # {spot, margin, futures_usdt, futures_coin}
            "base_url_http": None,  # Override with custom endpoint
            "base_url_ws": None,  # Override with custom endpoint
            "us": False,  # If client is for Binance US
        },
    },
    exec_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "account_type": "spot",  # {spot, margin, futures_usdt, futures_coin}
            "base_url_http": None,  # Override with custom endpoint
            "base_url_ws": None,  # Override with custom endpoint
            "us": False,  # If client is for Binance US
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
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
- `MARGIN`
- `FUTURES_USDT` (USDT or BUSD stablecoins as collateral)
- `FUTURES_COIN` (other cryptocurrency as collateral)

### Base URL overrides
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
            "account_type": "spot",  # {spot, margin, futures_usdt}
            "testnet": True,  # If client uses the testnet
        },
    },
    exec_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_TESTNET_API_KEY",
            "api_secret": "YOUR_BINANCE_TESTNET_API_SECRET",
            "account_type": "spot",  # {spot, margin, futures_usdt}
            "testnet": True,  # If client uses the testnet
        },
    },
)
```

### Parser warnings
Some Binance instruments are unable to be parsed into Nautilus objects if they 
contain enormous field values beyond what can be handled by the platform. 
In these cases, a _warn and continue_ approach is taken (the instrument will not 
be available).

These warnings may cause unnecessary log noise, and so it's possible to
configure the provider to not log the warnings, as per the client configuration
example below:

```python
instrument_provider=InstrumentProviderConfig(
    load_all=True, 
    log_warnings=False,
)
```

## Binance specific data
It's possible to subscribe to Binance specific data streams as they become available to the
adapter over time.

```{note}
Tickers and bars are not considered 'Binance specific' and can be subscribed to in the normal way.
However, as more adapters are built out which need for example mark price and funding rate updates, then these
methods may eventually become first-class (not requiring custom/generic subscriptions as below).
```

### BinanceFuturesMarkPriceUpdate
You can subscribe to `BinanceFuturesMarkPriceUpdate` (included funding rating info) 
data streams by subscribing in the following way from your actor or strategy:

```python
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
def on_data(self, data: Data):
    # First check the type of data
    if isinstance(data, BinanceFuturesMarkPriceUpdate):
        # Do something with the data
```
