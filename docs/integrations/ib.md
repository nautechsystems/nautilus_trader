# Interactive Brokers

Interactive Brokers (IB) is a trading platform providing market access across a wide range of financial instruments, including stocks, options, futures, currencies, bonds, funds, and cryptocurrencies. NautilusTrader offers an adapter to integrate with IB using their [Trader Workstation (TWS) API](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/) through their Python library, [ibapi](https://github.com/nautechsystems/ibapi).

The TWS API serves as an interface to IB's standalone trading applications: TWS and IB Gateway. Both can be downloaded from the IB website. If you haven't installed TWS or IB Gateway yet, refer to the [Initial Setup](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#tws-download) guide. In NautilusTrader, you'll establish a connection to one of these applications via the `InteractiveBrokersClient`.

Alternatively, you can start with a [dockerized version](https://github.com/gnzsnz/ib-gateway-docker) of the IB Gateway, which is particularly useful when deploying trading strategies on a hosted cloud platform. This requires having [Docker](https://www.docker.com/) installed on your machine, along with the [docker](https://pypi.org/project/docker/) Python package, which NautilusTrader conveniently includes as an extra package.

:::note
The standalone TWS and IB Gateway applications require manually inputting username, password, and trading mode (live or paper) at startup. The dockerized version of the IB Gateway handles these steps programmatically.
:::

## Installation

To install the latest `nautilus_trader` package along with the `ibapi` and optional `docker` dependencies using pip:

```
pip install -U "nautilus_trader[ib,docker]"
```

To install from source using uv:

```
uv sync --extra ib --extra docker
```

:::note
Because IB does not provide wheels for `ibapi`, NautilusTrader [repackages](https://pypi.org/project/nautilus-ibapi/) it for release on PyPI.
:::

## Examples

You can find functional live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/interactive_brokers/).

## Getting Started

Before implementing your trading strategies, please ensure that either TWS (Trader Workstation) or IB Gateway is currently running. You have the option to log in to one of these standalone applications using your personal credentials or alternatively, via `DockerizedIBGateway`.

### Establish Connection to an Existing Gateway or TWS:

Should you choose to connect to a pre-existing Gateway or TWS, it is crucial that you specify the `host` and `port` parameters in both the `InteractiveBrokersDataClientConfig` and `InteractiveBrokersExecClientConfig` to guarantee a successful connection.

### Establish Connection to DockerizedIBGateway:

In this case, it's essential to supply `dockerized_gateway` with an instance of `DockerizedIBGatewayConfig` in both the `InteractiveBrokersDataClientConfig` and `InteractiveBrokersExecClientConfig`. It's important to stress, however, that `host` and `port` parameters aren't necessary in this context.
The following example provides a clear illustration of how to establish a connection to a Dockerized Gateway, which is judiciously managed internally by the Factories.

```python
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway

gateway_config = DockerizedIBGatewayConfig(
    username="test",
    password="test",
    trading_mode="paper",
)

# This may take a short while to start up, especially the first time
gateway = DockerizedIBGateway(
    config=gateway_config
)
gateway.start()

# Confirm you are logged in
print(gateway.is_logged_in(gateway.container))

# Inspect the logs
print(gateway.container.logs())
```

**Note**: To supply credentials to the Interactive Brokers Gateway, either pass the `username` and `password` to the `DockerizedIBGatewayConfig`, or set the following environment variables:
- `TWS_USERNAME`
- `TWS_PASSWORD`

## Overview

The adapter includes several major components:

- `InteractiveBrokersClient`: Executes TWS API requests using `ibapi`.
- `HistoricInteractiveBrokersClient`: Provides methods for retrieving instruments and historical data, useful for backtesting.
- `InteractiveBrokersInstrumentProvider`: Retrieves or queries instruments for trading.
- `InteractiveBrokersDataClient`: Connects to the Gateway for streaming market data.
- `InteractiveBrokersExecutionClient`: Handles account information and executes trades.

## The Interactive Brokers Client

The `InteractiveBrokersClient` serves as the central component of the IB adapter, overseeing a range of critical functions. These include establishing and maintaining connections, handling API errors, executing trades, and gathering various types of data such as market data, contract/instrument data, and account details.

To ensure efficient management of these diverse responsibilities, the `InteractiveBrokersClient` is divided into several specialized mixin classes. This modular approach enhances manageability and clarity. The key subcomponents are:

- `InteractiveBrokersClientConnectionMixin`: This class is dedicated to managing the connection with TWS/Gateway.
- `InteractiveBrokersClientErrorMixin`: It focuses on addressing all encountered errors and warnings.
- `InteractiveBrokersClientAccountMixin`: Responsible for handling requests related to account information and positions.
- `InteractiveBrokersClientContractMixin`: Handles retrieving contracts (instruments) data.
- `InteractiveBrokersClientMarketDataMixin`: Handles market data requests, subscriptions and data processing.
- `InteractiveBrokersClientOrderMixin`: Oversees all aspects of order placement and management.

:::tip
To troubleshoot TWS API incoming message issues, consider starting at the `InteractiveBrokersClient._process_message` method, which acts as the primary gateway for processing all messages received from the API.
:::

## Symbology

The InteractiveBrokersInstrumentProvider supports three methods for constructing InstrumentId instances, which can be configured via the `symbology_method` enum in `InteractiveBrokersInstrumentProviderConfig`.

### Simplified Symbology

When symbology_method is set to `IB_SIMPLIFIED` (the default setting), the system utilizes the following parsing rules for symbology:

- Forex: The format is `{symbol}/{currency}.{exchange}`, where the currency pair is constructed as `EUR/USD.IDEALPRO`.
- Stocks: The format is `{localSymbol}.{primaryExchange}`. Any spaces in localSymbol are replaced with -, e.g., `BF-B.NYSE`.
- Futures: The format is `{localSymbol}.{exchange}`. Single digit years are expanded to two digits, e.g., `ESM24.CME`.
- Options: The format is `{localSymbol}.{exchange}`, with all spaces removed from localSymbol, e.g., `AAPL230217P00155000.SMART`.
- Index: The format is `^{localSymbol}.{exchange}`, e.g., `^SPX.CBOE`.

### Raw Symbology

Setting symbology_method to `IB_RAW` enforces stricter parsing rules that align directly with the fields defined in the ibapi. The format for each security type is as follows:

- CFDs: `{localSymbol}={secType}.IBCFD`
- Commodities: `{localSymbol}={secType}.IBCMDTY`
- Default for Other Types: `{localSymbol}={secType}.{exchange}`

This configuration ensures that the symbology is explicitly defined and matched with the Interactive Brokers API requirements, providing clear and consistent instrument identification.
While this format may lack visual clarity, it is robust and supports instruments from any region,
especially those with non-standard symbology where simplified parsing may fail.

### Databento Symbology

Setting symbology_method to `DATABENTO`, the system utilized the symbology rules defined by `DatabentoInstrumentProvider`.
Note that this symbology is only compatible with venues supported by Databento and there is not automatic fall-back to other symbology methods to avoid any conflicts.

## Instruments & Contracts

In IB, a NautilusTrader `Instrument` is equivalent to a [Contract](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#contracts). Contracts can be either a [basic contract](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#contract-object) or a more [detailed](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#contract-details) version (ContractDetails). The adapter models these using `IBContract` and `IBContractDetails` classes. The latter includes critical data like order types and trading hours, which are absent in the basic contract. As a result, `IBContractDetails` can be converted to an `Instrument` while `IBContract` cannot.

To search for contract information, use the [IB Contract Information Center](https://pennies.interactivebrokers.com/cstools/contract_info/).

It's typically suggested to utilize `symbology_method=SymbologyMethod.IB_SIMPLIFIED` (which is the default setting). This provides a cleaner and more intuitive use of `InstrumentId` by employing `load_ids` in the `InteractiveBrokersInstrumentProviderConfig`, following the guidelines established in the Simplified Symbology section.
In order to load multiple Instruments, such as Options Instrument without having to specify each strike explicitly, you would need to utilize `load_contracts` with provided instances of `IBContract`.

```python
for_loading_instrument_expiry = IBContract(
    secType="IND",
    symbol="SPX",
    exchange="CBOE",
    build_options_chain=True,
    lastTradeDateOrContractMonth='20240718',
)

for_loading_instrument_range = IBContract(
    secType="IND",
    symbol="SPX",
    exchange="CBOE",
    build_options_chain=True,
    min_expiry_days=0,
    max_expiry_days=30,
)
```

> **Note**: The `secType` and `symbol` should be specified for the Underlying Contract.

Some more examples of building IBContracts:
```python
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

When developing strategies with the IB adapter, the first step usually involves acquiring historical data for backtesting. The `HistoricInteractiveBrokersClient` offers methods to request and save this data.

Here's an example of retrieving and saving instrument and bar data. A more comprehensive example is available [here](https://github.com/nautechsystems/nautilus_trader/blob/master/examples/live/interactive_brokers/historic_download.py).

```python
import datetime
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.historic import HistoricInteractiveBrokersClient
from nautilus_trader.persistence.catalog import ParquetDataCatalog


async def main():
    contract = IBContract(
        secType="STK",
        symbol="AAPL",
        exchange="SMART",
        primaryExchange="NASDAQ",
    )
    client = HistoricInteractiveBrokersClient()

    instruments = await client.request_instruments(
        contracts=[contract],
    )

    bars = await client.request_bars(
        bar_specifications=["1-HOUR-LAST", "30-MINUTE-MID"],
        end_date_time=datetime.datetime(2023, 11, 6, 16, 30),
        tz_name="America/New_York",
        duration="1 D",
        contracts=[contract],
    )

    catalog = ParquetDataCatalog("./catalog")
    catalog.write_data(instruments)
    catalog.write_data(bars)
```

## Live Trading

Engaging in live or paper trading requires constructing and running a `TradingNode`.
This node incorporates both `InteractiveBrokersDataClient` and `InteractiveBrokersExecutionClient`,
which depend on the `InteractiveBrokersInstrumentProvider` to operate.

### InstrumentProvider

The `InteractiveBrokersInstrumentProvider` class functions as a bridge for accessing financial instrument data from IB.
Configurable through `InteractiveBrokersInstrumentProviderConfig`, it enables the customization of various instrument type parameters.
Additionally, this provider offers specialized methods to build and retrieve the entire futures and options chains.

```python
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod


instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
    build_futures_chain=False,  # Set to True if fetching futures
    build_options_chain=False,  # Set to True if fetching options
    min_expiry_days=10,         # Relevant for futures/options with expiration
    max_expiry_days=60,         # Relevant for futures/options with expiration
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
    load_contracts=frozenset(
        [
            IBContract(secType='STK', symbol='SPY', exchange='SMART', primaryExchange='ARCA'),
            IBContract(secType='STK', symbol='AAPL', exchange='SMART', primaryExchange='NASDAQ')
        ]
    ),
)
```

### Integration with Databento Data Client
To integrate with `DatabentoDataClient`, set the `symbology_method` in `InteractiveBrokersInstrumentProviderConfig`
to `SymbologyMethod.DATABENTO`. This ensures seamless compatibility with Databento symbology, eliminating the need
for manual translations or mappings within your strategy.

When using this configuration:
- `InteractiveBrokersInstrumentProvider` will not publish instruments to the cache to prevent conflicts.
- Instruments Cache management must be handled exclusively by `DatabentoDataClient`.


### Data Client

`InteractiveBrokersDataClient` interfaces with IB for streaming and retrieving market data. Upon
connection, it configures the [market data type](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#delayed-market-data)
and loads instruments based on the settings in `InteractiveBrokersInstrumentProviderConfig`.
This client can subscribe to and unsubscribe from various market data types, including quotes, trades, and bars.

Configurable through `InteractiveBrokersDataClientConfig`, it enables adjustments for handling revised bars,
trading hours preferences, and market data types (e.g., `IBMarketDataTypeEnum.REALTIME` or `IBMarketDataTypeEnum.DELAYED_FROZEN`).

```python
from nautilus_trader.adapters.interactive_brokers.config import IBMarketDataTypeEnum
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig


data_client_config = InteractiveBrokersDataClientConfig(
    ibg_port=4002,
    handle_revised_bars=False,
    use_regular_trading_hours=True,
    market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,  # Default is REALTIME if not set
    instrument_provider=instrument_provider_config,
    dockerized_gateway=dockerized_gateway_config,
)
```

### Execution Client

The `InteractiveBrokersExecutionClient` facilitates executing trades, accessing account information,
and processing order and trade-related details. It encompasses a range of methods for order management,
including reporting order statuses, placing new orders, and modifying or canceling existing ones.
Additionally, it generates position reports, although fill reports are not yet implemented.

```python
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.config import RoutingConfig


exec_client_config = InteractiveBrokersExecClientConfig(
    ibg_port=4002,
    account_id="DU123456",  # Must match the connected IB Gateway/TWS
    dockerized_gateway=dockerized_gateway_config,
    instrument_provider=instrument_provider_config,
    routing=RoutingConfig(
        default=True,
    )
)
```

### Full Configuration

Setting up a complete trading environment typically involves configuring a `TradingNodeConfig`, which
includes data and execution client configurations. Additional configurations are specified in `LiveDataEngineConfig`
to accommodate IB-specific requirements. A `TradingNode` is then instantiated from these configurations,
and factories for creating `InteractiveBrokersDataClient` and `InteractiveBrokersExecutionClient` are added.
Finally, the node is built and run.

You can find additional examples here: https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/interactive_brokers


```python
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode


# ... [continuing from prior example code] ...

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={"IB": data_client_config},
    exec_clients={"IB": exec_client_config},
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,  # Use opening time as `ts_event`, as per IB standard
        validate_data_sequence=True,         # Discards bars received out of sequence
    ),
)

node = TradingNode(config=config_node)
node.add_data_client_factory("IB", InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory("IB", InteractiveBrokersLiveExecClientFactory)
node.build()
node.portfolio.set_specific_venue(IB_VENUE)

if __name__ == "__main__":
    try:
        node.run()
    finally:
        # Stop and dispose of the node with SIGINT/CTRL+C
        node.dispose()
```
