# FTX

FTX is one of the highest volume exchanges for crypto futures and derivatives markets. 
This integration supports live market data ingest and order execution with FTX.

```{warning}
This integration is still under construction. Please consider it to be in an
unstable beta phase and exercise caution.
```

## Overview
The following documentation assumes a trader is setting up for both live market 
data feeds, and trade execution. The full FTX integration consists of an assortment of components, 
which can be used together or separately depending on the users needs.

- `FTXHttpClient` provides low-level HTTP API connectivity
- `FTXWebSocketClient` provides low-level WebSocket API connectivity
- `FTXInstrumentProvider` provides instrument parsing and loading functionality
- `FTXDataClient` provides a market data feed manager
- `FTXExecutionClient` provides an account management and trade execution gateway
- `FTXLiveDataClientFactory` creation factory for FTX data clients (used by the trading node builder)
- `FTXLiveExecClientFactory` creation factory for FTX execution clients (used by the trading node builder)

```{note}
Most users will simply define a configuration for a live trading node (as below), 
and won't need to necessarily work with these lower level components individually.
```

## Data types
To provide complete API functionality to traders, the integration includes several
custom data types:
- `FTXTicker` returned when subscribing to FTX tickers (like a `QuoteTick` which 
also includes `last_price`).

See the FTX [API Reference](../api_reference/adapters/ftx.md) for full definitions.

## Order types
The FTX integration makes the following order types available:
- `MARKET`
- `LIMIT`
- `STOP_MARKET`
- `STOP_LIMIT`
- `MARKET_IF_TOUCHED` (behaviour identical to `STOP_MARKET`)
- `LIMIT_IF_TOUCHED` (behaviour identical to `STOP_LIMIT`)
- `TRAILING_STOP` (does not take a `trigger_price`, just a `trailing_offset`)

## Configuration
The most common use case is to configure a live `TradingNode` to include FTX
data and execution clients. To achieve this, add an `FTX` section to your client
configuration(s):

```python
config = TradingNodeConfig(
    ...,  # Omitted
    data_clients={
        "FTX": {
            "api_key": "YOUR_FTX_API_KEY",
            "api_secret": "YOUR_FTX_API_SECRET",
            "subaccount": "YOUR_FTX_SUBACCOUNT",  # optional
            "us": False,
        },
    },
    exec_clients={
        "FTX": {
            "api_key": "YOUR_FTX_API_KEY",
            "api_secret": "YOUR_FTX_API_SECRET",
            "subaccount": "YOUR_FTX_SUBACCOUNT",  # optional
            "us": False,
        },
    },
)
```

Then, create a `TradingNode` and add the client factories:

```python
# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory("FTX", FTXLiveDataClientFactory)
node.add_exec_client_factory("FTX", FTXLiveExecClientFactory)

# Finally build the node
node.build()
```

### API credentials
There are two options for supplying your credentials to the FTX clients.
Either pass the corresponding `api_key` and `api_secret` values to the config dictionaries, or
set the following environment variables: 
- `FTX_API_KEY`
- `FTX_API_SECRET`

When starting the trading node, you'll receive immediate confirmation of whether your 
credentials are valid and have trading permissions.

### Sub-accounts
There are two options to enable trading through a sub-account, by setting the
`subaccount` value in the config dictionary, or set the `FTX_SUBACCOUNT`
environment variable.

### FTX US
There is support for FTX US accounts by setting the `us` option in the configs
to `True` (this is `False` by default). All functionality available to US accounts
should behave identically to standard FTX.

### Parser warnings
Some FTX instruments are unable to be parsed into Nautilus objects if they 
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

### USD high precision
While the base currency of FTX accounting is USD, they use a precision of 8 rather
than the default 2. Its possibly to override the default USD currency with an
FTX version with the higher precision by setting the `override_usd` flag to
`True` for either the `FTXDataClientConfig` or `FTXExecClientConfig`.
