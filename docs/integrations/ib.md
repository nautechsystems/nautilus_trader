# Interactive Brokers

NautilusTrader offers an adapter for integrating with the Interactive Brokers Gateway via 
[ib_insync](https://github.com/erdewit/ib_insync).

**Note**: If you are planning on using the built-in docker TWS Gateway when using the Interactive Brokers adapter,
you must ensure the `docker` package is installed. Run `poetry install --extras "ib docker"` 
or `poetry install --all-extras` inside your environment to ensure the necessary packages are installed.

## Overview

The following integration classes are available:
- `InteractiveBrokersInstrumentProvider` which allows querying Interactive Brokers for instruments.
- `InteractiveBrokersDataClient` which connects to the `Gateway` and streams market data.
- `InteractiveBrokersExecutionClient` which allows the retrieval of account information and execution of orders.

## Instruments
Interactive Brokers allows searching for instruments via the `qualifyContracts` API, which, if given enough information
can usually resolve a filter into an actual contract(s). A node can request instruments to be loaded by passing 
configuration to the `InstrumentProviderConfig` when initialising a `TradingNodeConfig` (note that while `filters`
is a dict, it must be converted to a tuple when passed to `InstrumentProviderConfig`).

At a minimum, you must specify the `secType` (security type) and `symbol` (equities etc) or `pair` (FX). See examples 
queries below for common use cases 

Example config: 

```python
config_node = TradingNodeConfig(
    data_clients={
        "IB": InteractiveBrokersDataClientConfig(
            instrument_provider=InteractiveBrokersInstrumentProviderConfig(
                load_ids={"EUR/USD.IDEALPRO", "AAPL.NASDAQ"},
                load_contracts={IBContract(secType="CONTFUT", exchange="CME", symbol="MES")},
            )
    ),
    ...
)
```

### Examples queries
- Stock: `IBContract(secType='STK', exchange='SMART', symbol='AMD', currency='USD')`
- Stock: `IBContract(secType='STK', exchange='SMART', primaryExchange='NASDAQ', symbol='INTC')`
- Forex: `InstrumentId('EUR/USD.IDEALPRO')`, `InstrumentId('USD/JPY.IDEALPRO')`
- CFD: `IBContract(secType='CFD', symbol='IBUS30')`
- Future: `InstrumentId('ES.CME')`, `IBContract(secType='CONTFUT', exchange='CME', symbol='ES', build_futures_chain=True)`
- Option: `InstrumentId('SPY251219C00395000.SMART')`, `IBContract(secType='STK', exchange='SMART', primaryExchange='ARCA', symbol='SPY', lastTradeDateOrContractMonth='20251219', build_options_chain=True)`
- Bond: `IBContract(secType='BOND', secIdType='ISIN', secId='US03076KAA60')`
- Crypto: `InstrumentId('BTC/USD.PAXOS')`


## Configuration
The most common use case is to configure a live `TradingNode` to include Interactive Brokers
data and execution clients. To achieve this, add an `IB` section to your client
configuration(s) and set the environment variables to your TWS (Traders Workstation) credentials:

```python
import os

config = TradingNodeConfig(
    data_clients={
        "IB": InteractiveBrokersDataClientConfig(
            username=os.getenv("TWS_USERNAME"),
            password=os.getenv("TWS_PASSWORD"),
            ...  # Omitted
    },
    exec_clients = {
        "IB": InteractiveBrokersExecutionClientConfig(
            username=os.getenv("TWS_USERNAME"),
            password=os.getenv("TWS_PASSWORD"),
            ...  # Omitted
    },
    ...  # Omitted
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
There are two options for supplying your credentials to the Interactive Brokers clients.
Either pass the corresponding `username` and `password` values to the config dictionaries, or
set the following environment variables: 
- `TWS_USERNAME`
- `TWS_PASSWORD`

When starting the trading node, you'll receive immediate confirmation of whether your
credentials are valid and have trading permissions.
