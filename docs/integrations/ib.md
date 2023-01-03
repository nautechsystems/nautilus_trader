# Interactive Brokers

NautilusTrader offers an adapter for integrating with the Interactive Brokers Gateway via 
[ib_insync](https://github.com/erdewit/ib_insync).

**Note**: If you are planning on using the built-in docker TWS Gateway when using the Interactive Brokers adapter,
you must manually install `docker` (due to current build issues). Run a manual `pip install docker` inside your
environment to ensure the Gateway can be run. 

## Overview

The following integration classes are available:
- `InteractiveBrokersInstrumentProvider` which allows querying Interactive Brokers for instruments.
- `InteractiveBrokersDataClient` which connects to the `Gateway` and streams market data.
- `InteractiveBrokersExecutionClient` which allows the retrieval of account information and execution of orders.

## Instruments
Interactive Brokers allows searching for instruments via the `qualifyContracts` API, which, if given enough information
can usually resolve a filter into an actual contract(s). A node can request instruments to be loaded by passing 
configuration to the `InstrumentProviderConfig` when initialising a `TradingNodeConfig` (note that while `filters`
is a dict, it must be converted to a tuple when passed to `InstrumentProviderConfig`), 

At a minimum, you must specify the `secType` (security type) and `symbol` (equities etc) or `pair` (FX). See examples 
queries below for common use cases 

Example config: 

```python
config_node = TradingNodeConfig(
    data_clients={
        "IB": InteractiveBrokersDataClientConfig(
            instrument_provider=InstrumentProviderConfig(
                load_all=True,
                filters=tuple({"secType": "CASH", "symbol": "EUR", "currecy": "USD"}.items())
            )
        )
)
```

### Examples queries
- Stock: `{"secType": "STK", "symbol": "AMD", "exchange": "SMART", "currency": "USD" }`
- Stock: `{"secType": "STK", "symbol": "INTC", "exchange": "SMART", "primaryExchange": "NASDAQ", "currency": "USD"}`
- Forex: `{"secType": "CASH", "symbol": "EUR","currency": "USD", "exchange": "IDEALPRO"}`
- CFD: `{"secType": "CFD", "symbol": "IBUS30"}`
- Future: `{"secType": "FUT", "symbol": "ES", "exchange": "GLOBEX", "lastTradeDateOrContractMonth": "20180921"}`
- Option: `{"secType": "OPT", "symbol": "SPY", "exchange": "SMART", "lastTradeDateOrContractMonth": "20170721", "strike": 240, "right": "C" }`
- Bond: `{"secType": "BOND", "secIdType": 'ISIN', "secId": 'US03076KAA60'}`
- Crypto: `{"secType": "CRYPTO", "symbol": "BTC", "exchange": "PAXOS", "currency": "USD"}`


## Configuration
The most common use case is to configure a live `TradingNode` to include Interactive Brokers
data and execution clients. To achieve this, add a `IB` section to your client
configuration(s) and _set the name of the environment variables_ containing your TWS 
(Traders Workstation) credentials:

```python
config = TradingNodeConfig(
    ...,  # Omitted 
    data_clients={
        "IB": {
            "username": "TWS_USERNAME",
            "password": "TWS_PASSWORD",
        },
    },
    exec_clients={
        "IB": {
            "username": "TWS_USERNAME",
            "password": "TWS_PASSWORD",
        },
    }
)
```

Then, create a `TradingNode` and add the client factories:

```python
# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory("IB", InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory("IB", InteractiveBrokersLiveExecClientFactory)

# Finally build the node
node.build()
```

### API credentials
There are two options for supplying your credentials to the Betfair clients.
Either pass the corresponding `username` and `password` values to the config dictionaries, or
set the following environment variables: 
- `TWS_USERNAME`
- `TWS_PASSWORD`

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.


