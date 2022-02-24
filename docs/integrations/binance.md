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

```{notes}
Most users will simply define a configuration for a live trading node (as below), 
and won't need to necessarily work with these lower level components individually.
```

## Binance data types
To provide complete API functionality to traders, the integration includes several
custom data types:
- `BinanceSpotTicker` returned when subscribing to Binance SPOT 24hr tickers (contains many prices and stats).
- `BinanceBar` returned when requesting historical, or subscribing to, Binance bars (contains extra volume information).

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
Either pass the corresponding `api_key` and `api_secret` values to the config dictionaries, or
set the following environment variables for live clients: 
- `BINANCE_API_KEY`
- `BINANCE_API_SECRET`

Or for clients connecting to testnets, you can set:
- `BINANCE_TESTNET_API_KEY`
- `BINANCE_TESTNET_API_SECRET`

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.

### Account Type
All the Binance account types will be supported for live trading. Set the account type
through the `account_type` option as a string. The account type options are:
- `spot`
- `margin`
- `futures_usdt` (USDT or BUSD stablecoins as collateral)
- `futures_coin` (other cryptocurrency as collateral)

```{note}
Binance does not currently offer a testnet for COIN-M futures.
```

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
