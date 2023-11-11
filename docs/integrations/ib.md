# Interactive Brokers

Interactive Brokers (IB) is a trading platform where you can trade stocks, options, futures, currencies, bonds, funds, and crypto. NautilusTrader provides an adapter to integrate with IB using their [Trader Workstation (TWS) API](https://interactivebrokers.github.io/tws-api/index.html) via their Python library, [ibapi](https://github.com/nautechsystems/ibapi).

The TWS API is an interface to IB's standalone trading applications, TWS and IB Gateway, which can be downloaded on IB's website. If you have not already installed TWS or IB Gateway, follow Interactive Brokers' [Initial Setup](https://interactivebrokers.github.io/tws-api/initial_setup.html) guide. You will define a connection to either of these two applications in NautilusTrader's `InteractiveBrokersClient`. 

Another (and perhaps easier way) to get started is to use a [dockerized version](https://github.com/unusualalpha/ib-gateway-docker/pkgs/container/ib-gateway) of IB Gateway, which is also what you would use when deploying your trading strategies on a hosted cloud platform. You will need [Docker](https://www.docker.com/) installed on your machine and the [docker](https://pypi.org/project/docker/) Python package, which is conveniently bundled with NautilusTrader as an extra package.

**Note**: The standalone TWS and IB Gateway applications require human intervention to specify a username, password and trading mode (live or paper trading) at startup. The dockerized IB Gateway is able to do so programmatically.

## Installation

To install the latest nautilus-trader package with the `ibapi` and optional `docker` extra dependencies using pip, run:

```
pip install -U "nautilus_trader[ib,docker]"
```

To install using poetry, run:

```
poetry add "nautilus_trader[ib,docker]"
```

**Note**: IB does not provide wheels for `ibapi`, so Nautilus [repackages]( https://pypi.org/project/nautilus-ibapi/) and releases it to PyPI.

## Getting Started

Before writing strategies, TWS / IB Gateway needs to be running. Launch either of the two standalone applications and enter your credentials, or start the dockerized IB Gateway using `InteractiveBrokersGateway` (make sure Docker is running in the background already!):

```
from nautilus_trader.adapters.interactive_brokers.gateway import InteractiveBrokersGateway

# This may take a short while to start up, especially the first time
gateway = InteractiveBrokersGateway(username="test", password="test", start=True)

# Confirm you are logged in
print(gateway.is_logged_in(gateway.container))

# Inspect the logs
print(gateway.container.logs())
```

**Note**: There are two options for supplying your credentials to the Interactive Brokers Gateway, Exec and Data clients.
Either pass the corresponding `username` and `password` values to the config dictionaries, or
set the following environment variables: 
- `TWS_USERNAME`
- `TWS_PASSWORD`

## Overview

The adapter is comprised of the following major components:
- `InteractiveBrokersClient` which uses `ibapi` to execute TWS API requests, supporting all other integration classes.
- `HistoricInteractiveBrokersClient` which provides a straightforward way to retrieve instruments and historical bar and tick data to load into the catalog (generally for backtesting).
- `InteractiveBrokersInstrumentProvider` which retrieves or queries instruments for trading.
- `InteractiveBrokersDataClient` which connects to the `Gateway` and streams market data for trading.
- `InteractiveBrokersExecutionClient` which retrieves account information and executes orders for trading.

## Instruments & Contracts

In Interactive Brokers, the concept of a NautilusTrader `Instrument` is called a [Contract](https://interactivebrokers.github.io/tws-api/contracts.html). A Contract can be represented in two ways: a [basic contract](https://interactivebrokers.github.io/tws-api/classIBApi_1_1Contract.html) or a [detailed contract](https://interactivebrokers.github.io/tws-api/classIBApi_1_1ContractDetails.html), which are defined in the adapter using the classes `IBContract` and `IBContractDetails`, respectively. Contract details include important information like supported order types and trading hours, which aren't found in the basic contract. As a result, `IBContractDetails` can be converted to an `Instrument` but `IBContract` cannot.

To find basic contract information, use the [IB Contract Information Center](https://pennies.interactivebrokers.com/cstools/contract_info/). 

Examples of `IBContracts`:
```
from nautilus_trader.adapters.interactive_brokers.common import IBContract
 
# Stock
IBContract(secType='STK', exchange='SMART', primaryExchange='ARCA', symbol='SPY')

# Bond
IBContract(secType='BOND', secIdType='ISIN', secId='US03076KAA60')

# Option
IBContract(secType='STK', exchange='SMART', primaryExchange='ARCA', symbol='SPY', lastTradeDateOrContractMonth='20251219', build_options_chain=True)

# CFD
IBContract(secType='CFD', symbol='IBUS30')

# Future
IBContract(secType='CONTFUT', exchange='CME', symbol='ES', build_futures_chain=True)

# Forex
IBContract(secType='CASH', exchange='IDEALPRO', symbol='EUR', currency='GBP')

# Crypto
IBContract(secType='CRYPTO', symbol='ETH', exchange='PAXOS', currency='USD')
```

## Historical Data & Backtesting

The first step in developing strategies using the IB adapter typically involves fetching historical data that can be used for backtesting. The `HistoricInteractiveBrokersClient` provides a number of convenience methods to request data and save it to the catalog.

The following example illustrates retrieving and saving instrument, bar, quote tick, and trade tick data. Read more about using the data for backtesting [here].

```
# Define path to existing catalog or desired new catalog path
CATALOG_PATH = "./catalog"

async def main():
    contract = IBContract(
        secType="STK",
        symbol="AAPL",
        exchange="SMART",
        primaryExchange="NASDAQ",
    )
    client = HistoricInteractiveBrokersClient(
        catalog=ParquetDataCatalog(CATALOG_PATH)
    )
    # By default, data is written to the catalog
    await client.get_instruments(contract=contract)

    # All the methods also return the retrieved objects
    # so you can introspect them or explicitly write them
    # to the catalog
    bars = await client.get_historical_bars(
        contract=contract,
        bar_type=...
        write_to_catalog=False
    )
    print(bars)
    client.catalog.write_data(bars)
    
    await client.get_historical_ticks(
        contract=contract,
        bar_type..
    )

if __name__ == "__main__":
    asyncio.run(main())
```

## Live Trading

To live trade or paper trade, a `TradingNode` needs to be built and run with a `InteractiveBrokersDataClient` and a `InteractiveBrokersExecutionClient`, which both rely on a `InteractiveBrokersInstrumentProvider`.

### InstrumentProvider

To retrieve instruments, you will need to use the `InteractiveBrokersInstrumentProvider`. This provider is also used under the hood in the `HistoricInteractiveBrokersClient` to retrieve instruments for data collection.

```
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig

gateway_config = InteractiveBrokersGatewayConfig(username="test", password="test")

# Specify instruments to retrieve in load_ids and / or load_contracts
# The following parameters should only be specified if retrieving futures or options: build_futures_chain, build_options_chain, min_expiry_days, max_expiry_days
instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    build_futures_chain=False,  # optional, only if fetching futures
    build_options_chain=False,  # optional, only if fetching futures
    min_expiry_days=10,  # optional, only if fetching futures / options
    max_expiry_days=60,  # optional, only if fetching futures / options
    load_ids=frozenset(
        [
            "EUR/USD.IDEALPRO",
            "BTC/USD.PAXOS",
            "SPY.ARCA",
            "V.NYSE",
            "YMH24.CBOT",
            "CLZ27.NYMEX",
            "ESZ27.CME",
        ],
    ),
    load_contracts=frozenset(ib_contracts),
)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id="IB-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        "IB": InteractiveBrokersDataClientConfig(
            ibg_host="127.0.0.1",
            ibg_port=4002,
            ibg_client_id=1,
            handle_revised_bars=False,
            use_regular_trading_hours=True,
            market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,  # https://interactivebrokers.github.io/tws-api/market_data_type.html
            instrument_provider=instrument_provider,
            gateway=gateway,
        ),
    },
    timeout_connection=90.0,
)

node = TradingNode(config=config_node)

node.trader.add_actor(downloader)

# Register your client factories with the node (can take user defined factories)
node.add_data_client_factory("InteractiveBrokers", InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory("InteractiveBrokers", InteractiveBrokersLiveExecClientFactory)
node.build()

# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()

```

Interactive Brokers allows searching for instruments via the `reqMatchingSymbols` API ([docs](https://interactivebrokers.github.io/tws-api/matching_symbols.html)), which, if given enough information
can usually resolve a filter into an actual contract(s). A node can request instruments to be loaded by passing 
configuration to the `InstrumentProviderConfig` when initialising a `TradingNodeConfig` (note that while `filters`
is a dict, it must be converted to a tuple when passed to `InstrumentProviderConfig`).

At a minimum, you must specify the `secType` (security type) and `symbol` (equities etc) or `pair` (FX). See examples 
queries below for common use cases 

Example config: 

```python
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.config import TradingNodeConfig

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

### Data Client
- `InteractiveBrokersDataClient` which streams market data.

### Execution Client
- `InteractiveBrokersExecutionClient` which retrieves account information and executes orders.

### Full Configuration
The most common use case is to configure a live `TradingNode` to include Interactive Brokers
data and execution clients. To achieve this, add an `IB` section to your client
configuration(s) and set the environment variables to your TWS (Traders Workstation) credentials:

```python
import os
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.config import TradingNodeConfig


config = TradingNodeConfig(
    data_clients={
        "IB": InteractiveBrokersDataClientConfig(
            username=os.getenv("TWS_USERNAME"),
            password=os.getenv("TWS_PASSWORD"),
            ...  # Omitted
    },
    exec_clients = {
        "IB": InteractiveBrokersExecClientConfig(
            username=os.getenv("TWS_USERNAME"),
            password=os.getenv("TWS_PASSWORD"),
            ...  # Omitted
    },
    ...  # Omitted
)
```

Then, create a `TradingNode` and add the client factories:

```python
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory

# Instantiate the live trading node with a configuration
node = TradingNode(config=config)

# Register the client factories with the node
node.add_data_client_factory("IB", InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory("IB", InteractiveBrokersLiveExecClientFactory)

# Finally build the node
node.build()
```
