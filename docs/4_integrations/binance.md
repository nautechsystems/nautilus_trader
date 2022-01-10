# Binance

Founded in 2017, Binance is one of the largest cryptocurrency exchanges in terms 
of daily trading volume, and open interest of crypto assets and crypto 
derivative products. This integration supports live market data ingest and order
execution with Binance.

```{warning}
This integration is still under construction. Please consider it to be in an
unstable beta phase and exercise caution.
```

```{note}
Binance offers different account types including `spot`, `margin` and 
`futures`. NautilusTrader currently supports `spot` account trading, with 
support for the other account types on the way.
```

## Overview
The following documentation assumes a trader is setting up for both live market 
data feeds, and trade execution. The Binance integration consists of several 
main components, which can be used together or separately depending on the users 
needs.

- `BinanceHttpClient` provides low-level HTTP API connectivity
- `BinanceWebSocketClient` provides low-level WebSocket API connectivity
- `BinanceInstrumentProvider` provides instrument parsing and loading functionality
- `BinanceDataClient` provides a market data feed manager
- `BinanceExecutionClient` provides an account management and trade execution gateway
- `BinanceLiveDataClientFactory` creation factory for Binance data clients (used by the trading node builder)
- `BinanceLiveExecutionClientFactory` creation factory for Binance execution clients (used by the trading node builder)

## Binance data types
To provide complete API functionality to traders, the integration includes several
custom data types:
- `BinanceTicker` returned when subscribing to Binance tickers (contains many prices and stats).
- `BinanceBar` returned when requesting historical, or subscribing to, Binance bars (contains extra volume information).

See the Binance [API Reference](../3_api_reference/adapters/binance.md) for full definitions.

## Configuration
The most common use case is to configure a live `TradingNode` to include Binance 
data and execution clients. To achieve this, add a `BINANCE` section to your client
configuration:

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "us": False,
        },
    },
    exec_clients={
        "BINANCE": {
            "api_key": "YOUR_BINANCE_API_KEY",
            "api_secret": "YOUR_BINANCE_API_SECRET",
            "us": False,
        },
    },
)
```

### API credentials
There are two options for supplying your credentials to the Binance clients.
Either pass the corresponding `api_key` and `api_secret` values to the config dictionaries, or
set the `BINANCE_API_KEY` and `BINANCE_API_SECRET` environment variables.
You'll receive immediate confirmation that your credentials are valid with trading
permissions when starting a trading node.

### Binance US
There is support for Binance US accounts by setting the `us` option in the configs
to `True` (this is `False` by default). All functionality available to US accounts
should behave identically to standard Binance.
