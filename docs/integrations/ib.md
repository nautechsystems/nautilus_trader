# Interactive Brokers

Interactive Brokers (IB) is a trading platform providing market access across a wide range of financial instruments, including stocks, options, futures, currencies, bonds, funds, and cryptocurrencies. NautilusTrader offers an adapter to integrate with IB using their [Trader Workstation (TWS) API](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/) through their Python library, [ibapi](https://github.com/nautechsystems/ibapi).

The TWS API serves as an interface to IB's standalone trading applications: TWS and IB Gateway. Both can be downloaded from the IB website. If you haven't installed TWS or IB Gateway yet, refer to the [Initial Setup](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#tws-download) guide. In NautilusTrader, you'll establish a connection to one of these applications via the `InteractiveBrokersClient`.

Alternatively, you can start with a [dockerized version](https://github.com/gnzsnz/ib-gateway-docker) of the IB Gateway, which is particularly useful when deploying trading strategies on a hosted cloud platform. This requires having [Docker](https://www.docker.com/) installed on your machine, along with the [docker](https://pypi.org/project/docker/) Python package, which NautilusTrader conveniently includes as an extra package.

:::note
The standalone TWS and IB Gateway applications require manually inputting username, password, and trading mode (live or paper) at startup. The dockerized version of the IB Gateway handles these steps programmatically.
:::

## Installation

To install NautilusTrader with Interactive Brokers (and Docker) support:

```bash
pip install --upgrade "nautilus_trader[ib,docker]"
```

To build from source with all extras (including IB and Docker):

```bash
uv sync --all-extras
```

:::note
Because IB does not provide wheels for `ibapi`, NautilusTrader [repackages](https://pypi.org/project/nautilus-ibapi/) it for release on PyPI.
:::

## Examples

You can find live example scripts [here](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/interactive_brokers/).

## Getting Started

Before implementing your trading strategies, please ensure that either TWS (Trader Workstation) or IB Gateway is currently running. You have the option to log in to one of these standalone applications using your personal credentials or alternatively, via `DockerizedIBGateway`.

### Connection Methods

There are two primary ways to connect to Interactive Brokers:

1. **Connect to an existing TWS or IB Gateway instance**
2. **Use the dockerized IB Gateway (recommended for automated deployments)**

### Default Ports

Interactive Brokers uses different default ports depending on the application and trading mode:

| Application | Paper Trading | Live Trading |
|-------------|---------------|--------------|
| TWS         | 7497          | 7496         |
| IB Gateway  | 4002          | 4001         |

### Establish Connection to an Existing Gateway or TWS

When connecting to a pre-existing Gateway or TWS, specify the `ibg_host` and `ibg_port` parameters in both the `InteractiveBrokersDataClientConfig` and `InteractiveBrokersExecClientConfig`:

```python
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig

# Example for TWS paper trading (default port 7497)
data_config = InteractiveBrokersDataClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=7497,
    ibg_client_id=1,
)

exec_config = InteractiveBrokersExecClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=7497,
    ibg_client_id=1,
    account_id="DU123456",  # Your paper trading account ID
)
```

### Establish Connection to DockerizedIBGateway

For automated deployments, the dockerized gateway is recommended. Supply `dockerized_gateway` with an instance of `DockerizedIBGatewayConfig` in both client configurations. The `ibg_host` and `ibg_port` parameters are not needed as they're managed automatically.

```python
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway

gateway_config = DockerizedIBGatewayConfig(
    username="your_username",  # Or set TWS_USERNAME env var
    password="your_password",  # Or set TWS_PASSWORD env var
    trading_mode="paper",      # "paper" or "live"
    read_only_api=True,        # Set to False to allow order execution
    timeout=300,               # Startup timeout in seconds
)

# This may take a short while to start up, especially the first time
gateway = DockerizedIBGateway(config=gateway_config)
gateway.start()

# Confirm you are logged in
print(gateway.is_logged_in(gateway.container))

# Inspect the logs
print(gateway.container.logs())
```

### Environment Variables

To supply credentials to the Interactive Brokers Gateway, either pass the `username` and `password` to the `DockerizedIBGatewayConfig`, or set the following environment variables:

- `TWS_USERNAME` - Your IB account username
- `TWS_PASSWORD` - Your IB account password
- `TWS_ACCOUNT` - Your IB account ID (used as fallback for `account_id`)

### Connection Management

The adapter includes robust connection management features:

- **Automatic reconnection**: Configurable via `IB_MAX_CONNECTION_ATTEMPTS` environment variable
- **Connection timeout**: Configurable via `connection_timeout` parameter (default: 300 seconds)
- **Connection watchdog**: Monitors connection health and triggers reconnection if needed
- **Graceful error handling**: Comprehensive error handling for various connection scenarios

## Overview

The Interactive Brokers adapter provides a comprehensive integration with IB's TWS API. The adapter includes several major components:

### Core Components

- **`InteractiveBrokersClient`**: The central client that executes TWS API requests using `ibapi`. Manages connections, handles errors, and coordinates all API interactions.
- **`InteractiveBrokersDataClient`**: Connects to the Gateway for streaming market data including quotes, trades, and bars.
- **`InteractiveBrokersExecutionClient`**: Handles account information, order management, and trade execution.
- **`InteractiveBrokersInstrumentProvider`**: Retrieves and manages instrument definitions, including support for options and futures chains.
- **`HistoricInteractiveBrokersClient`**: Provides methods for retrieving instruments and historical data, useful for backtesting and research.

### Supporting Components

- **`DockerizedIBGateway`**: Manages dockerized IB Gateway instances for automated deployments.
- **Configuration Classes**: Comprehensive configuration options for all components.
- **Factory Classes**: Create and configure client instances with proper dependencies.

### Supported Asset Classes

The adapter supports trading across all major asset classes available through Interactive Brokers:

- **Equities**: Stocks, ETFs, and equity options
- **Fixed Income**: Bonds and bond funds
- **Derivatives**: Futures, options, and warrants
- **Foreign Exchange**: Spot FX and FX forwards
- **Cryptocurrencies**: Bitcoin, Ethereum, and other digital assets
- **Commodities**: Physical commodities and commodity futures
- **Indices**: Index products and index options

## The Interactive Brokers Client

The `InteractiveBrokersClient` serves as the central component of the IB adapter, overseeing a range of critical functions. These include establishing and maintaining connections, handling API errors, executing trades, and gathering various types of data such as market data, contract/instrument data, and account details.

To ensure efficient management of these diverse responsibilities, the `InteractiveBrokersClient` is divided into several specialized mixin classes. This modular approach enhances manageability and clarity.

### Client Architecture

The client uses a mixin-based architecture where each mixin handles a specific aspect of the IB API:

#### Connection Management (`InteractiveBrokersClientConnectionMixin`)

- Establishes and maintains socket connections to TWS/Gateway
- Handles connection timeouts and reconnection logic
- Manages connection state and health monitoring
- Supports configurable reconnection attempts via `IB_MAX_CONNECTION_ATTEMPTS` environment variable

#### Error Handling (`InteractiveBrokersClientErrorMixin`)

- Processes all API errors and warnings
- Categorizes errors by type (client errors, connectivity issues, request errors)
- Handles subscription and request-specific error scenarios
- Provides comprehensive error logging and debugging information

#### Account Management (`InteractiveBrokersClientAccountMixin`)

- Retrieves account information and balances
- Manages position data and portfolio updates
- Handles multi-account scenarios
- Processes account-related notifications

#### Contract/Instrument Management (`InteractiveBrokersClientContractMixin`)

- Retrieves contract details and specifications
- Handles instrument searches and lookups
- Manages contract validation and verification
- Supports complex instrument types (options chains, futures chains)

#### Market Data Management (`InteractiveBrokersClientMarketDataMixin`)

- Handles real-time and historical market data subscriptions
- Processes quotes, trades, and bar data
- Manages market data type settings (real-time, delayed, frozen)
- Handles tick-by-tick data and market depth

#### Order Management (`InteractiveBrokersClientOrderMixin`)

- Processes order placement, modification, and cancellation
- Handles order status updates and execution reports
- Manages order validation and error handling
- Supports complex order types and conditions

### Key Features

- **Asynchronous Operation**: All operations are fully asynchronous using Python's asyncio
- **Robust Error Handling**: Comprehensive error categorization and handling
- **Connection Resilience**: Automatic reconnection with configurable retry logic
- **Message Processing**: Efficient message queue processing for high-throughput scenarios
- **State Management**: Proper state tracking for connections, subscriptions, and requests

:::tip
To troubleshoot TWS API incoming message issues, consider starting at the `InteractiveBrokersClient._process_message` method, which acts as the primary gateway for processing all messages received from the API.
:::

## Symbology

The `InteractiveBrokersInstrumentProvider` supports three methods for constructing `InstrumentId` instances, which can be configured via the `symbology_method` enum in `InteractiveBrokersInstrumentProviderConfig`.

### Symbology Methods

#### 1. Simplified Symbology (`IB_SIMPLIFIED`) - Default

When `symbology_method` is set to `IB_SIMPLIFIED` (the default setting), the system uses intuitive, human-readable symbology rules:

**Format Rules by Asset Class:**

- **Forex**: `{symbol}/{currency}.{exchange}`
  - Example: `EUR/USD.IDEALPRO`
- **Stocks**: `{localSymbol}.{primaryExchange}`
  - Spaces in localSymbol are replaced with hyphens
  - Example: `BF-B.NYSE`, `SPY.ARCA`
- **Futures**: `{localSymbol}.{exchange}`
  - Individual contracts use single digit years
  - Example: `ESM4.CME`, `CLZ7.NYMEX`
- **Continuous Futures**: `{symbol}.{exchange}`
  - Represents front month, automatically rolling
  - Example: `ES.CME`, `CL.NYMEX`
- **Options on Futures (FOP)**: `{localSymbol}.{exchange}`
  - Format: `{symbol}{month}{year} {right}{strike}`
  - Example: `ESM4 C4200.CME`
- **Options**: `{localSymbol}.{exchange}`
  - All spaces removed from localSymbol
  - Example: `AAPL230217P00155000.SMART`
- **Indices**: `^{localSymbol}.{exchange}`
  - Example: `^SPX.CBOE`, `^NDX.NASDAQ`
- **Bonds**: `{localSymbol}.{exchange}`
  - Example: `912828XE8.SMART`
- **Cryptocurrencies**: `{symbol}/{currency}.{exchange}`
  - Example: `BTC/USD.PAXOS`, `ETH/USD.PAXOS`

#### 2. Raw Symbology (`IB_RAW`)

Setting `symbology_method` to `IB_RAW` enforces stricter parsing rules that align directly with the fields defined in the IB API. This method provides maximum compatibility across all regions and instrument types:

**Format Rules:**

- **CFDs**: `{localSymbol}={secType}.IBCFD`
- **Commodities**: `{localSymbol}={secType}.IBCMDTY`
- **Default for Other Types**: `{localSymbol}={secType}.{exchange}`

**Examples:**

- `IBUS30=CFD.IBCFD`
- `XAUUSD=CMDTY.IBCMDTY`
- `AAPL=STK.SMART`

This configuration ensures explicit instrument identification and supports instruments from any region, especially those with non-standard symbology where simplified parsing may fail.

### MIC Venue Conversion

The adapter supports converting Interactive Brokers exchange codes to Market Identifier Codes (MIC) for standardized venue identification:

#### `convert_exchange_to_mic_venue`

When set to `True`, the adapter automatically converts IB exchange codes to their corresponding MIC codes:

```python
instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    convert_exchange_to_mic_venue=True,  # Enable MIC conversion
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
)
```

**Examples of MIC Conversion:**

- `CME` → `XCME` (Chicago Mercantile Exchange)
- `NASDAQ` → `XNAS` (Nasdaq Stock Market)
- `NYSE` → `XNYS` (New York Stock Exchange)
- `LSE` → `XLON` (London Stock Exchange)

#### `symbol_to_mic_venue`

For custom venue mapping, use the `symbol_to_mic_venue` dictionary to override default conversions:

```python
instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    convert_exchange_to_mic_venue=True,
    symbol_to_mic_venue={
        "ES": "XCME",  # All ES futures/options use CME MIC
        "SPY": "ARCX", # SPY specifically uses ARCA
    },
)
```

### Supported Instrument Formats

The adapter supports various instrument formats based on Interactive Brokers' contract specifications:

#### Futures Month Codes

- **F** = January, **G** = February, **H** = March, **J** = April
- **K** = May, **M** = June, **N** = July, **Q** = August
- **U** = September, **V** = October, **X** = November, **Z** = December

#### Supported Exchanges by Asset Class

**Futures Exchanges:**

- `CME`, `CBOT`, `NYMEX`, `COMEX`, `KCBT`, `MGE`, `NYBOT`, `SNFE`

**Options Exchanges:**

- `SMART` (IB's smart routing)

**Forex Exchanges:**

- `IDEALPRO` (IB's forex platform)

**Cryptocurrency Exchanges:**

- `PAXOS` (IB's crypto platform)

**CFD/Commodity Exchanges:**

- `IBCFD`, `IBCMDTY` (IB's internal routing)

### Choosing the Right Symbology Method

- **Use `IB_SIMPLIFIED`** (default) for most use cases - provides clean, readable instrument IDs
- **Use `IB_RAW`** when dealing with complex international instruments or when simplified parsing fails
- **Enable `convert_exchange_to_mic_venue`** when you need standardized MIC venue codes for compliance or data consistency

## Instruments & Contracts

In Interactive Brokers, a NautilusTrader `Instrument` corresponds to an IB [Contract](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#contracts). The adapter handles two types of contract representations:

### Contract Types

#### Basic Contract (`IBContract`)

- Contains essential contract identification fields
- Used for contract searches and basic operations
- Cannot be directly converted to a NautilusTrader `Instrument`

#### Contract Details (`IBContractDetails`)

- Contains comprehensive contract information including:
  - Order types supported
  - Trading hours and calendar
  - Margin requirements
  - Price increments and multipliers
  - Market data permissions
- Can be converted to a NautilusTrader `Instrument`
- Required for trading operations

### Contract Discovery

To search for contract information, use the [IB Contract Information Center](https://pennies.interactivebrokers.com/cstools/contract_info/).

### Loading Instruments

There are two primary methods for loading instruments:

#### 1. Using `load_ids` (Recommended)
Use `symbology_method=SymbologyMethod.IB_SIMPLIFIED` (default) with `load_ids` for clean, intuitive instrument identification:

```python
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod

instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
    load_ids=frozenset([
        "EUR/USD.IDEALPRO",    # Forex
        "SPY.ARCA",            # Stock
        "ESM24.CME",           # Future
        "BTC/USD.PAXOS",       # Crypto
        "^SPX.CBOE",           # Index
    ]),
)
```

#### 2. Using `load_contracts` (For Complex Instruments)
Use `load_contracts` with `IBContract` instances for complex scenarios like options/futures chains:

```python
from nautilus_trader.adapters.interactive_brokers.common import IBContract

# Load options chain for specific expiry
options_chain_expiry = IBContract(
    secType="IND",
    symbol="SPX",
    exchange="CBOE",
    build_options_chain=True,
    lastTradeDateOrContractMonth='20240718',
)

# Load options chain for date range
options_chain_range = IBContract(
    secType="IND",
    symbol="SPX",
    exchange="CBOE",
    build_options_chain=True,
    min_expiry_days=0,
    max_expiry_days=30,
)

# Load futures chain
futures_chain = IBContract(
    secType="CONTFUT",
    exchange="CME",
    symbol="ES",
    build_futures_chain=True,
)

instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    load_contracts=frozenset([
        options_chain_expiry,
        options_chain_range,
        futures_chain,
    ]),
)
```

### IBContract Examples by Asset Class

```python
from nautilus_trader.adapters.interactive_brokers.common import IBContract

# Stocks
IBContract(secType='STK', exchange='SMART', primaryExchange='ARCA', symbol='SPY')
IBContract(secType='STK', exchange='SMART', primaryExchange='NASDAQ', symbol='AAPL')

# Bonds
IBContract(secType='BOND', secIdType='ISIN', secId='US03076KAA60')
IBContract(secType='BOND', secIdType='CUSIP', secId='912828XE8')

# Individual Options
IBContract(secType='OPT', exchange='SMART', symbol='SPY',
           lastTradeDateOrContractMonth='20251219', strike=500, right='C')

# Options Chain (loads all strikes/expirations)
IBContract(secType='STK', exchange='SMART', primaryExchange='ARCA', symbol='SPY',
           build_options_chain=True, min_expiry_days=10, max_expiry_days=60)

# CFDs
IBContract(secType='CFD', symbol='IBUS30')
IBContract(secType='CFD', symbol='DE40EUR', exchange='SMART')

# Individual Futures
IBContract(secType='FUT', exchange='CME', symbol='ES',
           lastTradeDateOrContractMonth='20240315')

# Futures Chain (loads all expirations)
IBContract(secType='CONTFUT', exchange='CME', symbol='ES', build_futures_chain=True)

# Options on Futures (FOP) - Individual
IBContract(secType='FOP', exchange='CME', symbol='ES',
           lastTradeDateOrContractMonth='20240315', strike=4200, right='C')

# Options on Futures Chain (loads all strikes/expirations)
IBContract(secType='CONTFUT', exchange='CME', symbol='ES',
           build_options_chain=True, min_expiry_days=7, max_expiry_days=60)

# Forex
IBContract(secType='CASH', exchange='IDEALPRO', symbol='EUR', currency='USD')
IBContract(secType='CASH', exchange='IDEALPRO', symbol='GBP', currency='JPY')

# Cryptocurrencies
IBContract(secType='CRYPTO', symbol='BTC', exchange='PAXOS', currency='USD')
IBContract(secType='CRYPTO', symbol='ETH', exchange='PAXOS', currency='USD')

# Indices
IBContract(secType='IND', symbol='SPX', exchange='CBOE')
IBContract(secType='IND', symbol='NDX', exchange='NASDAQ')

# Commodities
IBContract(secType='CMDTY', symbol='XAUUSD', exchange='SMART')
```

### Advanced Configuration Options

```python
# Options chain with custom exchange
IBContract(
    secType="STK",
    symbol="AAPL",
    exchange="SMART",
    primaryExchange="NASDAQ",
    build_options_chain=True,
    options_chain_exchange="CBOE",  # Use CBOE for options instead of SMART
    min_expiry_days=7,
    max_expiry_days=45,
)

# Futures chain with specific months
IBContract(
    secType="CONTFUT",
    exchange="NYMEX",
    symbol="CL",  # Crude Oil
    build_futures_chain=True,
    min_expiry_days=30,
    max_expiry_days=180,
)
```

### Continuous Futures

For continuous futures contracts (using `secType='CONTFUT'`), the adapter creates instrument IDs using just the symbol and venue:

```python
# Continuous futures examples
IBContract(secType='CONTFUT', exchange='CME', symbol='ES')  # → ES.CME
IBContract(secType='CONTFUT', exchange='NYMEX', symbol='CL') # → CL.NYMEX

# With MIC venue conversion enabled
instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    convert_exchange_to_mic_venue=True,
)
# Results in:
# ES.XCME (instead of ES.CME)
# CL.XNYM (instead of CL.NYMEX)
```

**Continuous Futures vs Individual Futures:**

- **Continuous**: `ES.CME` - Represents the front month contract, automatically rolls
- **Individual**: `ESM4.CME` - Specific March 2024 contract

:::note
When using `build_options_chain=True` or `build_futures_chain=True`, the `secType` and `symbol` should be specified for the underlying contract. The adapter will automatically discover and load all related derivative contracts within the specified expiry range.
:::

## Historical Data & Backtesting

The `HistoricInteractiveBrokersClient` provides comprehensive methods for retrieving historical data from Interactive Brokers for backtesting and research purposes.

### Supported Data Types

- **Bar Data**: OHLCV bars with various aggregations (time-based, tick-based, volume-based)
- **Tick Data**: Trade ticks and quote ticks with microsecond precision
- **Instrument Data**: Complete contract specifications and trading rules

### Historical Data Client

```python
from nautilus_trader.adapters.interactive_brokers.historical.client import HistoricInteractiveBrokersClient
from ibapi.common import MarketDataTypeEnum

# Initialize the client
client = HistoricInteractiveBrokersClient(
    host="127.0.0.1",
    port=7497,
    client_id=1,
    market_data_type=MarketDataTypeEnum.DELAYED_FROZEN,  # Use delayed data if no subscription
    log_level="INFO"
)

# Connect to TWS/Gateway
await client.connect()
```

### Retrieving Instruments

```python
from nautilus_trader.adapters.interactive_brokers.common import IBContract

# Define contracts
contracts = [
    IBContract(secType="STK", symbol="AAPL", exchange="SMART", primaryExchange="NASDAQ"),
    IBContract(secType="STK", symbol="MSFT", exchange="SMART", primaryExchange="NASDAQ"),
    IBContract(secType="CASH", symbol="EUR", currency="USD", exchange="IDEALPRO"),
]

# Request instrument definitions
instruments = await client.request_instruments(contracts=contracts)
```

### Retrieving Historical Bars

```python
import datetime

# Request historical bars
bars = await client.request_bars(
    bar_specifications=[
        "1-MINUTE-LAST",    # 1-minute bars using last price
        "5-MINUTE-MID",     # 5-minute bars using midpoint
        "1-HOUR-LAST",      # 1-hour bars using last price
        "1-DAY-LAST",       # Daily bars using last price
    ],
    start_date_time=datetime.datetime(2023, 11, 1, 9, 30),
    end_date_time=datetime.datetime(2023, 11, 6, 16, 30),
    tz_name="America/New_York",
    contracts=contracts,
    use_rth=True,  # Regular Trading Hours only
    timeout=120,   # Request timeout in seconds
)
```

### Retrieving Historical Ticks

```python
# Request historical tick data
ticks = await client.request_ticks(
    tick_types=["TRADES", "BID_ASK"],  # Trade ticks and quote ticks
    start_date_time=datetime.datetime(2023, 11, 6, 9, 30),
    end_date_time=datetime.datetime(2023, 11, 6, 16, 30),
    tz_name="America/New_York",
    contracts=contracts,
    use_rth=True,
    timeout=120,
)
```

### Bar Specifications

The adapter supports various bar specifications:

#### Time-Based Bars

- `"1-SECOND-LAST"`, `"5-SECOND-LAST"`, `"10-SECOND-LAST"`, `"15-SECOND-LAST"`, `"30-SECOND-LAST"`
- `"1-MINUTE-LAST"`, `"2-MINUTE-LAST"`, `"3-MINUTE-LAST"`, `"5-MINUTE-LAST"`, `"10-MINUTE-LAST"`, `"15-MINUTE-LAST"`, `"20-MINUTE-LAST"`, `"30-MINUTE-LAST"`
- `"1-HOUR-LAST"`, `"2-HOUR-LAST"`, `"3-HOUR-LAST"`, `"4-HOUR-LAST"`, `"8-HOUR-LAST"`
- `"1-DAY-LAST"`, `"1-WEEK-LAST"`, `"1-MONTH-LAST"`

#### Price Types

- `LAST` - Last traded price
- `MID` - Midpoint of bid/ask
- `BID` - Bid price
- `ASK` - Ask price

### Complete Example

```python
import asyncio
import datetime
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.historical.client import HistoricInteractiveBrokersClient
from nautilus_trader.persistence.catalog import ParquetDataCatalog


async def download_historical_data():
    # Initialize client
    client = HistoricInteractiveBrokersClient(
        host="127.0.0.1",
        port=7497,
        client_id=5,
    )

    # Connect
    await client.connect()
    await asyncio.sleep(2)  # Allow connection to stabilize

    # Define contracts
    contracts = [
        IBContract(secType="STK", symbol="AAPL", exchange="SMART", primaryExchange="NASDAQ"),
        IBContract(secType="CASH", symbol="EUR", currency="USD", exchange="IDEALPRO"),
    ]

    # Request instruments
    instruments = await client.request_instruments(contracts=contracts)

    # Request historical bars
    bars = await client.request_bars(
        bar_specifications=["1-HOUR-LAST", "1-DAY-LAST"],
        start_date_time=datetime.datetime(2023, 11, 1, 9, 30),
        end_date_time=datetime.datetime(2023, 11, 6, 16, 30),
        tz_name="America/New_York",
        contracts=contracts,
        use_rth=True,
    )

    # Request tick data
    ticks = await client.request_ticks(
        tick_types=["TRADES"],
        start_date_time=datetime.datetime(2023, 11, 6, 14, 0),
        end_date_time=datetime.datetime(2023, 11, 6, 15, 0),
        tz_name="America/New_York",
        contracts=contracts,
    )

    # Save to catalog
    catalog = ParquetDataCatalog("./catalog")
    catalog.write_data(instruments)
    catalog.write_data(bars)
    catalog.write_data(ticks)

    print(f"Downloaded {len(instruments)} instruments")
    print(f"Downloaded {len(bars)} bars")
    print(f"Downloaded {len(ticks)} ticks")

    # Disconnect
    await client.disconnect()

# Run the example
if __name__ == "__main__":
    asyncio.run(download_historical_data())
```

### Data Limitations

Be aware of Interactive Brokers' historical data limitations:

- **Rate Limits**: IB enforces rate limits on historical data requests
- **Data Availability**: Historical data availability varies by instrument and subscription level
- **Market Data Permissions**: Some data requires specific market data subscriptions
- **Time Ranges**: Maximum lookback periods vary by bar size and instrument type

### Best Practices

1. **Use Delayed Data**: For backtesting, `MarketDataTypeEnum.DELAYED_FROZEN` is often sufficient
2. **Batch Requests**: Group multiple instruments in single requests when possible
3. **Handle Timeouts**: Set appropriate timeout values for large data requests
4. **Respect Rate Limits**: Add delays between requests to avoid hitting rate limits
5. **Validate Data**: Always check data quality and completeness before backtesting

## Live Trading

Live trading with Interactive Brokers requires setting up a `TradingNode` that incorporates both `InteractiveBrokersDataClient` and `InteractiveBrokersExecutionClient`. These clients depend on the `InteractiveBrokersInstrumentProvider` for instrument management.

### Architecture Overview

The live trading setup consists of three main components:

1. **InstrumentProvider**: Manages instrument definitions and contract details
2. **DataClient**: Handles real-time market data subscriptions
3. **ExecutionClient**: Manages orders, positions, and account information

### InstrumentProvider Configuration

The `InteractiveBrokersInstrumentProvider` serves as the bridge for accessing financial instrument data from IB. It supports loading individual instruments, options chains, and futures chains.

#### Basic Configuration

```python
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
from nautilus_trader.adapters.interactive_brokers.common import IBContract

instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
    build_futures_chain=False,  # Set to True if fetching futures chains
    build_options_chain=False,  # Set to True if fetching options chains
    min_expiry_days=10,         # Minimum days to expiry for derivatives
    max_expiry_days=60,         # Maximum days to expiry for derivatives
    convert_exchange_to_mic_venue=False,  # Use MIC codes for venue mapping
    cache_validity_days=1,      # Cache instrument data for 1 day
    load_ids=frozenset([
        # Individual instruments using simplified symbology
        "EUR/USD.IDEALPRO",     # Forex
        "BTC/USD.PAXOS",        # Cryptocurrency
        "SPY.ARCA",             # Stock ETF
        "V.NYSE",               # Individual stock
        "ESM4.CME",             # Future contract (single digit year)
        "^SPX.CBOE",            # Index
    ]),
    load_contracts=frozenset([
        # Complex instruments using IBContract
        IBContract(secType='STK', symbol='AAPL', exchange='SMART', primaryExchange='NASDAQ'),
        IBContract(secType='CASH', symbol='GBP', currency='USD', exchange='IDEALPRO'),
    ]),
)
```

#### Advanced Configuration for Derivatives

```python
# Configuration for options and futures chains
advanced_config = InteractiveBrokersInstrumentProviderConfig(
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
    build_futures_chain=True,   # Enable futures chain loading
    build_options_chain=True,   # Enable options chain loading
    min_expiry_days=7,          # Load contracts expiring in 7+ days
    max_expiry_days=90,         # Load contracts expiring within 90 days
    load_contracts=frozenset([
        # Load SPY options chain
        IBContract(
            secType='STK',
            symbol='SPY',
            exchange='SMART',
            primaryExchange='ARCA',
            build_options_chain=True,
        ),
        # Load ES futures chain
        IBContract(
            secType='CONTFUT',
            exchange='CME',
            symbol='ES',
            build_futures_chain=True,
        ),
    ]),
)
```

### Integration with External Data Providers

The Interactive Brokers adapter can be used alongside other data providers for enhanced market data coverage. When using multiple data sources:

- Use consistent symbology methods across providers
- Consider using `convert_exchange_to_mic_venue=True` for standardized venue identification
- Ensure instrument cache management is handled properly to avoid conflicts

### Data Client Configuration

The `InteractiveBrokersDataClient` interfaces with IB for streaming and retrieving real-time market data. Upon connection, it configures the [market data type](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#delayed-market-data) and loads instruments based on the `InteractiveBrokersInstrumentProviderConfig` settings.

#### Supported Data Types

- **Quote Ticks**: Real-time bid/ask prices and sizes
- **Trade Ticks**: Real-time trade prices and volumes
- **Bar Data**: Real-time OHLCV bars (1-second to 1-day intervals)
- **Market Depth**: Level 2 order book data (where available)

#### Market Data Types

Interactive Brokers supports several market data types:

- `REALTIME`: Live market data (requires market data subscriptions)
- `DELAYED`: 15-20 minute delayed data (free for most markets)
- `DELAYED_FROZEN`: Delayed data that doesn't update (useful for testing)
- `FROZEN`: Last known real-time data (when market is closed)

#### Basic Data Client Configuration

```python
from nautilus_trader.adapters.interactive_brokers.config import IBMarketDataTypeEnum
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig

data_client_config = InteractiveBrokersDataClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=7497,  # TWS paper trading port
    ibg_client_id=1,
    use_regular_trading_hours=True,  # RTH only for stocks
    market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,  # Use delayed data
    ignore_quote_tick_size_updates=False,  # Include size-only updates
    instrument_provider=instrument_provider_config,
    connection_timeout=300,  # 5 minutes
    request_timeout=60,      # 1 minute
)
```

#### Advanced Data Client Configuration

```python
# Configuration for production with real-time data
production_data_config = InteractiveBrokersDataClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=4001,  # IB Gateway live trading port
    ibg_client_id=1,
    use_regular_trading_hours=False,  # Include extended hours
    market_data_type=IBMarketDataTypeEnum.REALTIME,  # Real-time data
    ignore_quote_tick_size_updates=True,  # Reduce tick volume
    handle_revised_bars=True,  # Handle bar revisions
    instrument_provider=instrument_provider_config,
    dockerized_gateway=dockerized_gateway_config,  # If using Docker
    connection_timeout=300,
    request_timeout=60,
)
```

#### Configuration Options Explained

- **`use_regular_trading_hours`**: When `True`, only requests data during regular trading hours. Primarily affects bar data for stocks.
- **`ignore_quote_tick_size_updates`**: When `True`, filters out quote ticks where only the size changed (not price), reducing data volume.
- **`handle_revised_bars`**: When `True`, processes bar revisions from IB (bars can be updated after initial publication).
- **`connection_timeout`**: Maximum time to wait for initial connection establishment.
- **`request_timeout`**: Maximum time to wait for historical data requests.

### Execution Client Configuration

The `InteractiveBrokersExecutionClient` handles trade execution, order management, account information, and position tracking. It provides comprehensive order lifecycle management and real-time account updates.

#### Supported Functionality

- **Order Management**: Place, modify, and cancel orders
- **Order Types**: Market, limit, stop, stop-limit, trailing stop, and more
- **Account Information**: Real-time balance and margin updates
- **Position Tracking**: Real-time position updates and P&L
- **Trade Reporting**: Execution reports and fill notifications
- **Risk Management**: Pre-trade risk checks and position limits

#### Supported Order Types

The adapter supports most Interactive Brokers order types:

- **Market Orders**: `OrderType.MARKET`
- **Limit Orders**: `OrderType.LIMIT`
- **Stop Orders**: `OrderType.STOP_MARKET`
- **Stop-Limit Orders**: `OrderType.STOP_LIMIT`
- **Market-If-Touched**: `OrderType.MARKET_IF_TOUCHED`
- **Limit-If-Touched**: `OrderType.LIMIT_IF_TOUCHED`
- **Trailing Stop Market**: `OrderType.TRAILING_STOP_MARKET`
- **Trailing Stop Limit**: `OrderType.TRAILING_STOP_LIMIT`
- **Market-on-Close**: `OrderType.MARKET` with `TimeInForce.AT_THE_CLOSE`
- **Limit-on-Close**: `OrderType.LIMIT` with `TimeInForce.AT_THE_CLOSE`

#### Time-in-Force Options

- **Day Orders**: `TimeInForce.DAY`
- **Good-Till-Canceled**: `TimeInForce.GTC`
- **Immediate-or-Cancel**: `TimeInForce.IOC`
- **Fill-or-Kill**: `TimeInForce.FOK`
- **Good-Till-Date**: `TimeInForce.GTD`
- **At-the-Open**: `TimeInForce.AT_THE_OPEN`
- **At-the-Close**: `TimeInForce.AT_THE_CLOSE`

#### Basic Execution Client Configuration

```python
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.config import RoutingConfig

exec_client_config = InteractiveBrokersExecClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=7497,  # TWS paper trading port
    ibg_client_id=1,
    account_id="DU123456",  # Your IB account ID (paper or live)
    instrument_provider=instrument_provider_config,
    connection_timeout=300,
    routing=RoutingConfig(default=True),  # Route all orders through this client
)
```

#### Advanced Execution Client Configuration

```python
# Production configuration with dockerized gateway
production_exec_config = InteractiveBrokersExecClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=4001,  # IB Gateway live trading port
    ibg_client_id=1,
    account_id=None,  # Will use TWS_ACCOUNT environment variable
    instrument_provider=instrument_provider_config,
    dockerized_gateway=dockerized_gateway_config,
    connection_timeout=300,
    routing=RoutingConfig(default=True),
)
```

#### Account ID Configuration

The `account_id` parameter is crucial and must match the account logged into TWS/Gateway:

```python
# Option 1: Specify directly in config
exec_config = InteractiveBrokersExecClientConfig(
    account_id="DU123456",  # Paper trading account
    # ... other parameters
)

# Option 2: Use environment variable
import os
os.environ["TWS_ACCOUNT"] = "DU123456"
exec_config = InteractiveBrokersExecClientConfig(
    account_id=None,  # Will use TWS_ACCOUNT env var
    # ... other parameters
)
```

#### Order Tags and Advanced Features

The adapter supports IB-specific order parameters through order tags:

```python
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags

# Create order with IB-specific parameters
order_tags = IBOrderTags(
    allOrNone=True,           # All-or-none order
    ocaGroup="MyGroup1",      # One-cancels-all group
    ocaType=1,                # Cancel with block
    activeStartTime="20240315 09:30:00 EST",  # GTC activation time
    activeStopTime="20240315 16:00:00 EST",   # GTC deactivation time
    goodAfterTime="20240315 09:35:00 EST",    # Good after time
)

# Apply tags to order (implementation depends on your strategy code)
```

### Complete Trading Node Configuration

Setting up a complete trading environment involves configuring a `TradingNodeConfig` with all necessary components. Here are comprehensive examples for different scenarios.

#### Paper Trading Configuration

```python
import os
from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.adapters.interactive_brokers.config import IBMarketDataTypeEnum
from nautilus_trader.adapters.interactive_brokers.config import SymbologyMethod
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode

# Instrument provider configuration
instrument_provider_config = InteractiveBrokersInstrumentProviderConfig(
    symbology_method=SymbologyMethod.IB_SIMPLIFIED,
    load_ids=frozenset([
        "EUR/USD.IDEALPRO",
        "GBP/USD.IDEALPRO",
        "SPY.ARCA",
        "QQQ.NASDAQ",
        "AAPL.NASDAQ",
        "MSFT.NASDAQ",
    ]),
)

# Data client configuration
data_client_config = InteractiveBrokersDataClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=7497,  # TWS paper trading
    ibg_client_id=1,
    use_regular_trading_hours=True,
    market_data_type=IBMarketDataTypeEnum.DELAYED_FROZEN,
    instrument_provider=instrument_provider_config,
)

# Execution client configuration
exec_client_config = InteractiveBrokersExecClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=7497,  # TWS paper trading
    ibg_client_id=1,
    account_id="DU123456",  # Your paper trading account
    instrument_provider=instrument_provider_config,
    routing=RoutingConfig(default=True),
)

# Trading node configuration
config_node = TradingNodeConfig(
    trader_id="PAPER-TRADER-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={IB: data_client_config},
    exec_clients={IB: exec_client_config},
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,  # IB standard: use bar open time
        validate_data_sequence=True,         # Discard out-of-sequence bars
    ),
    timeout_connection=90.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)

# Create and configure the trading node
node = TradingNode(config=config_node)
node.add_data_client_factory(IB, InteractiveBrokersLiveDataClientFactory)
node.add_exec_client_factory(IB, InteractiveBrokersLiveExecClientFactory)
node.build()
node.portfolio.set_specific_venue(IB_VENUE)

if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
```

#### Live Trading with Dockerized Gateway

```python
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig

# Dockerized gateway configuration
dockerized_gateway_config = DockerizedIBGatewayConfig(
    username=os.environ.get("TWS_USERNAME"),
    password=os.environ.get("TWS_PASSWORD"),
    trading_mode="live",  # "paper" or "live"
    read_only_api=False,  # Allow order execution
    timeout=300,
)

# Data client with dockerized gateway
data_client_config = InteractiveBrokersDataClientConfig(
    ibg_client_id=1,
    use_regular_trading_hours=False,  # Include extended hours
    market_data_type=IBMarketDataTypeEnum.REALTIME,
    instrument_provider=instrument_provider_config,
    dockerized_gateway=dockerized_gateway_config,
)

# Execution client with dockerized gateway
exec_client_config = InteractiveBrokersExecClientConfig(
    ibg_client_id=1,
    account_id=os.environ.get("TWS_ACCOUNT"),  # Live account ID
    instrument_provider=instrument_provider_config,
    dockerized_gateway=dockerized_gateway_config,
    routing=RoutingConfig(default=True),
)

# Live trading node configuration
config_node = TradingNodeConfig(
    trader_id="LIVE-TRADER-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={IB: data_client_config},
    exec_clients={IB: exec_client_config},
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,
        validate_data_sequence=True,
    ),
)
```

#### Multi-Client Configuration

For advanced setups, you can configure multiple clients with different purposes:

```python
# Separate data and execution clients with different client IDs
data_client_config = InteractiveBrokersDataClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=7497,
    ibg_client_id=1,  # Data client uses ID 1
    market_data_type=IBMarketDataTypeEnum.REALTIME,
    instrument_provider=instrument_provider_config,
)

exec_client_config = InteractiveBrokersExecClientConfig(
    ibg_host="127.0.0.1",
    ibg_port=7497,
    ibg_client_id=2,  # Execution client uses ID 2
    account_id="DU123456",
    instrument_provider=instrument_provider_config,
    routing=RoutingConfig(default=True),
)
```

### Running the Trading Node

```python
def run_trading_node():
    """Run the trading node with proper error handling."""
    node = None
    try:
        # Create and build node
        node = TradingNode(config=config_node)
        node.add_data_client_factory(IB, InteractiveBrokersLiveDataClientFactory)
        node.add_exec_client_factory(IB, InteractiveBrokersLiveExecClientFactory)
        node.build()

        # Set venue for portfolio
        node.portfolio.set_specific_venue(IB_VENUE)

        # Add your strategies here
        # node.trader.add_strategy(YourStrategy())

        # Run the node
        node.run()

    except KeyboardInterrupt:
        print("Shutting down...")
    except Exception as e:
        print(f"Error: {e}")
    finally:
        if node:
            node.dispose()

if __name__ == "__main__":
    run_trading_node()
```

### Additional Configuration Options

#### Environment Variables

Set these environment variables for easier configuration:

```bash
export TWS_USERNAME="your_ib_username"
export TWS_PASSWORD="your_ib_password"
export TWS_ACCOUNT="your_account_id"
export IB_MAX_CONNECTION_ATTEMPTS="5"  # Optional: limit reconnection attempts
```

#### Logging Configuration

```python
# Enhanced logging configuration
logging_config = LoggingConfig(
    log_level="INFO",
    log_level_file="DEBUG",
    log_file_format="json",  # JSON format for structured logging
    log_component_levels={
        "InteractiveBrokersClient": "DEBUG",
        "InteractiveBrokersDataClient": "INFO",
        "InteractiveBrokersExecutionClient": "INFO",
    },
)
```

You can find additional examples here: <https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/interactive_brokers>

## Troubleshooting

### Common Connection Issues

#### Connection Refused

- **Cause**: TWS/Gateway not running or wrong port
- **Solution**: Verify TWS/Gateway is running and check port configuration
- **Default Ports**: TWS (7497/7496), IB Gateway (4002/4001)

#### Authentication Errors

- **Cause**: Incorrect credentials or account not logged in
- **Solution**: Verify username/password and ensure account is logged into TWS/Gateway

#### Client ID Conflicts

- **Cause**: Multiple clients using the same client ID
- **Solution**: Use unique client IDs for each connection

#### Market Data Permissions

- **Cause**: Insufficient market data subscriptions
- **Solution**: Use `IBMarketDataTypeEnum.DELAYED_FROZEN` for testing or subscribe to required data feeds

### Error Codes

Interactive Brokers uses specific error codes. Common ones include:

- **200**: No security definition found
- **201**: Order rejected - reason follows
- **202**: Order cancelled
- **300**: Can't find EId with ticker ID
- **354**: Requested market data is not subscribed
- **2104**: Market data farm connection is OK
- **2106**: HMDS data farm connection is OK

### Performance Optimization

#### Reduce Data Volume

```python
# Reduce quote tick volume by ignoring size-only updates
data_config = InteractiveBrokersDataClientConfig(
    ignore_quote_tick_size_updates=True,
    # ... other config
)
```

#### Connection Management

```python
# Set reasonable timeouts
config = InteractiveBrokersDataClientConfig(
    connection_timeout=300,  # 5 minutes
    request_timeout=60,      # 1 minute
    # ... other config
)
```

#### Memory Management

- Use appropriate bar sizes for your strategy
- Limit the number of simultaneous subscriptions
- Consider using historical data for backtesting instead of live data

### Best Practices

#### Security

- Never hardcode credentials in source code
- Use environment variables for sensitive information
- Use paper trading for development and testing
- Set `read_only_api=True` for data-only applications

#### Development Workflow

1. **Start with Paper Trading**: Always test with paper trading first
2. **Use Delayed Data**: Use `DELAYED_FROZEN` market data for development
3. **Implement Proper Error Handling**: Handle connection losses and API errors gracefully
4. **Monitor Logs**: Enable appropriate logging levels for debugging
5. **Test Reconnection**: Test your strategy's behavior during connection interruptions

#### Production Deployment

- Use dockerized gateway for automated deployments
- Implement proper monitoring and alerting
- Set up log aggregation and analysis
- Use real-time data subscriptions only when necessary
- Implement circuit breakers and position limits

#### Order Management

- Always validate orders before submission
- Implement proper position sizing
- Use appropriate order types for your strategy
- Monitor order status and handle rejections
- Implement timeout handling for order operations

### Debugging Tips

#### Enable Debug Logging

```python
logging_config = LoggingConfig(
    log_level="DEBUG",
    log_component_levels={
        "InteractiveBrokersClient": "DEBUG",
    },
)
```

#### Monitor Connection Status

```python
# Check connection status in your strategy
if not self.data_client.is_connected:
    self.log.warning("Data client disconnected")
```

#### Validate Instruments

```python
# Ensure instruments are loaded before trading
instruments = self.cache.instruments()
if not instruments:
    self.log.error("No instruments loaded")
```

### Support and Resources

- **IB API Documentation**: [TWS API Guide](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/)
- **NautilusTrader Examples**: [GitHub Examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/interactive_brokers)
- **IB Contract Search**: [Contract Information Center](https://pennies.interactivebrokers.com/cstools/contract_info/)
- **Market Data Subscriptions**: [IB Market Data](https://www.interactivebrokers.com/en/trading/market-data.php)
