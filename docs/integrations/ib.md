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

## Getting started

Before implementing your trading strategies, make sure that either TWS (Trader Workstation) or IB Gateway is running. You can log in to one of these standalone applications with your credentials, or connect programmatically via `DockerizedIBGateway`.

### Connection methods

There are two primary ways to connect to Interactive Brokers:

1. **Connect to an existing TWS or IB Gateway instance**
2. **Use the dockerized IB Gateway (recommended for automated deployments)**

### Default ports

Interactive Brokers uses different default ports depending on the application and trading mode:

| Application | Paper Trading | Live Trading |
|-------------|---------------|--------------|
| TWS         | 7497          | 7496         |
| IB Gateway  | 4002          | 4001         |

### Establish connection to an existing gateway or TWS

When connecting to a pre-existing gateway or TWS, specify the `ibg_host` and `ibg_port` parameters in both the `InteractiveBrokersDataClientConfig` and `InteractiveBrokersExecClientConfig`:

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

### Establish connection to Dockerized IB Gateway

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

### Environment variables

To supply credentials to the Interactive Brokers Gateway, either pass the `username` and `password` to the `DockerizedIBGatewayConfig`, or set the following environment variables:

- `TWS_USERNAME`: Your IB account username.
- `TWS_PASSWORD`: Your IB account password.
- `TWS_ACCOUNT`: Your IB account ID (used as the fallback for `account_id`).

### Connection management

The adapter includes robust connection management features:

- **Automatic reconnection**: Configure retries with the `IB_MAX_CONNECTION_ATTEMPTS` environment variable.
- **Connection timeout**: Adjust the timeout with the `connection_timeout` parameter (default: 300 seconds).
- **Connection watchdog**: Monitor connection health and trigger reconnection automatically when required.
- **Graceful error handling**: Handle diverse connection scenarios with comprehensive error classification.

## Overview

The Interactive Brokers adapter provides a comprehensive integration with IB's TWS API. The adapter includes several major components:

### Core components

- **`InteractiveBrokersClient`**: The central client that executes TWS API requests using `ibapi`. Manages connections, handles errors, and coordinates all API interactions.
- **`InteractiveBrokersDataClient`**: Connects to the Gateway for streaming market data including quotes, trades, and bars.
- **`InteractiveBrokersExecutionClient`**: Handles account information, order management, and trade execution.
- **`InteractiveBrokersInstrumentProvider`**: Retrieves and manages instrument definitions, including support for options and futures chains.
- **`HistoricInteractiveBrokersClient`**: Provides methods for retrieving instruments and historical data, useful for backtesting and research.

### Supporting components

- **`DockerizedIBGateway`**: Manages dockerized IB Gateway instances for automated deployments.
- **Configuration classes**: Provide comprehensive configuration options for all components.
- **Factory classes**: Create and configure client instances with the necessary dependencies.

### Supported asset classes

The adapter supports trading across all major asset classes available through Interactive Brokers:

- **Equities**: Stocks, ETFs, and equity options.
- **Fixed income**: Bonds and bond funds.
- **Derivatives**: Futures, options, and warrants.
- **Foreign exchange**: Spot FX and FX forwards.
- **Cryptocurrencies**: Bitcoin, Ethereum, and other digital assets.
- **Commodities**: Physical commodities and commodity futures.
- **Indices**: Index products and index options.

## The Interactive Brokers client

The `InteractiveBrokersClient` serves as the central component of the IB adapter, overseeing a range of critical functions. These include establishing and maintaining connections, handling API errors, executing trades, and gathering various types of data such as market data, contract/instrument data, and account details.

To ensure efficient management of these diverse responsibilities, the `InteractiveBrokersClient` is divided into several specialized mixin classes. This modular approach enhances manageability and clarity.

### Client architecture

The client uses a mixin-based architecture where each mixin handles a specific aspect of the IB API:

#### Connection management (`InteractiveBrokersClientConnectionMixin`)

- Establishes and maintains socket connections to TWS/Gateway.
- Handles connection timeouts and reconnection logic.
- Manages connection state and health monitoring.
- Supports configurable reconnection attempts via `IB_MAX_CONNECTION_ATTEMPTS` environment variable.

#### Error handling (`InteractiveBrokersClientErrorMixin`)

- Processes all API errors and warnings.
- Categorizes errors by type (client errors, connectivity issues, request errors).
- Handles subscription and request-specific error scenarios.
- Provides comprehensive error logging and debugging information.

#### Account management (`InteractiveBrokersClientAccountMixin`)

- Retrieves account information and balances.
- Manages position data and portfolio updates.
- Handles multi-account scenarios.
- Processes account-related notifications.

#### Contract/instrument management (`InteractiveBrokersClientContractMixin`)

- Retrieves contract details and specifications.
- Handles instrument searches and lookups.
- Manages contract validation and verification.
- Supports complex instrument types (options chains, futures chains).

#### Market data management (`InteractiveBrokersClientMarketDataMixin`)

- Handles real-time and historical market data subscriptions.
- Processes quotes, trades, and bar data.
- Manages market data type settings (real-time, delayed, frozen).
- Handles tick-by-tick data and market depth.

#### Order management (`InteractiveBrokersClientOrderMixin`)

- Processes order placement, modification, and cancellation.
- Handles order status updates and execution reports.
- Manages order validation and error handling.
- Supports complex order types and conditions.

### Key features

- **Asynchronous operation**: All operations are fully asynchronous using Python's asyncio.
- **Robust error handling**: Comprehensive error categorization and handling.
- **Connection resilience**: Automatic reconnection with configurable retry logic.
- **Message processing**: Efficient message queue processing for high-throughput scenarios.
- **State management**: Proper state tracking for connections, subscriptions, and requests.

:::tip
To troubleshoot TWS API incoming message issues, consider starting at the `InteractiveBrokersClient._process_message` method, which acts as the primary gateway for processing all messages received from the API.
:::

## Symbology

The `InteractiveBrokersInstrumentProvider` supports three methods for constructing `InstrumentId` instances, which can be configured via the `symbology_method` enum in `InteractiveBrokersInstrumentProviderConfig`.

### Symbology methods

#### 1. Simplified symbology (`IB_SIMPLIFIED`) - default

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

#### 2. Raw symbology (`IB_RAW`)

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

### MIC venue conversion

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

### Supported instrument formats

The adapter supports various instrument formats based on Interactive Brokers' contract specifications:

#### Futures month codes

- **F** = January, **G** = February, **H** = March, **J** = April
- **K** = May, **M** = June, **N** = July, **Q** = August
- **U** = September, **V** = October, **X** = November, **Z** = December

#### Supported exchanges by asset class

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

### Choosing the right symbology method

- **Use `IB_SIMPLIFIED`** (default) for most use cases - provides clean, readable instrument IDs
- **Use `IB_RAW`** when dealing with complex international instruments or when simplified parsing fails
- **Enable `convert_exchange_to_mic_venue`** when you need standardized MIC venue codes for compliance or data consistency

## Instruments and contracts

In Interactive Brokers, a NautilusTrader `Instrument` corresponds to an IB [Contract](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#contracts). The adapter handles two types of contract representations:

### Contract types

#### Basic contract (`IBContract`)

- Contains essential contract identification fields
- Used for contract searches and basic operations
- Cannot be directly converted to a NautilusTrader `Instrument`

#### Contract details (`IBContractDetails`)

- Contains comprehensive contract information including:
  - Order types supported
  - Trading hours and calendar
  - Margin requirements
  - Price increments and multipliers
  - Market data permissions
- Can be converted to a NautilusTrader `Instrument`
- Required for trading operations

### Contract discovery

To search for contract information, use the [IB Contract Information Center](https://pennies.interactivebrokers.com/cstools/contract_info/).

### Loading instruments

There are two primary methods for loading instruments:

#### 1. Using `load_ids` (recommended)

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

#### 2. Using `load_contracts` (for complex instruments)

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

### IBContract examples by asset class

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

### Advanced configuration options

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

### Continuous futures

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

## Option spreads

Interactive Brokers supports option spreads through BAG contracts, which combine multiple option legs into a single tradeable instrument. NautilusTrader provides comprehensive support for creating, loading, and trading option spreads.

### Creating option spread instrument IDs

Option spreads are created using the `InstrumentId.new_spread()` method, which combines individual option legs with their respective ratios:

```python
from nautilus_trader.model.identifiers import InstrumentId

# Create individual option instrument IDs
call_leg = InstrumentId.from_str("SPY C400.SMART")
put_leg = InstrumentId.from_str("SPY P390.SMART")

# Create a 1:1 call spread (long call, short call)
call_spread_id = InstrumentId.new_spread([
    (call_leg, 1),   # Long 1 contract
    (put_leg, -1),   # Short 1 contract
])

# Create a 1:2 ratio spread
ratio_spread_id = InstrumentId.new_spread([
    (call_leg, 1),   # Long 1 contract
    (put_leg, 2),    # Long 2 contracts
])
```

### Dynamic spread loading

Option spreads must be requested before they can be traded or subscribed to for market data. Use the `request_instrument()` method to dynamically load spread instruments:

```python
# In your strategy's on_start method
def on_start(self):
    # Request the spread instrument
    self.request_instrument(spread_id)

def on_instrument(self, instrument):
    # Handle the loaded spread instrument
    self.log.info(f"Loaded spread: {instrument.id}")

    # Now you can subscribe to market data
    self.subscribe_quote_ticks(instrument.id)

    # And place orders
    order = self.order_factory.market(
        instrument_id=instrument.id,
        order_side=OrderSide.BUY,
        quantity=instrument.make_qty(1),
        time_in_force=TimeInForce.DAY,
    )
    self.submit_order(order)
```

### Spread trading requirements

1. **Load individual legs first**: Ensure the individual option legs are available before creating spreads.
2. **Request the spread instrument**: Use `request_instrument()` to load the spread before trading.
3. **Subscribe to market data**: Request quote ticks after the spread is loaded.
4. **Place orders**: Any order type can be used once the spread is available.

## Historical data and backtesting

The `HistoricInteractiveBrokersClient` provides comprehensive methods for retrieving historical data from Interactive Brokers for backtesting and research purposes.

### Supported data types

- **Bar data**: OHLCV bars with time, tick, and volume aggregations.
- **Tick data**: Trade ticks and quote ticks with microsecond precision.
- **Instrument data**: Complete contract specifications and trading rules.

### Historical data client

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

### Retrieving instruments

#### Basic instrument retrieval

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

#### Option chain retrieval with catalog storage

You can download entire option chains using `request_instruments` in your strategy, with the added benefit of saving the data to the catalog using `update_catalog=True`:

```python
# In your strategy's on_start method
def on_start(self):
    self.request_instruments(
        venue=IB_VENUE,
        update_catalog=True,
        params={
            "update_catalog": True,
            "ib_contracts": (
                # SPY options
                {
                    "secType": "STK",
                    "symbol": "SPY",
                    "exchange": "SMART",
                    "primaryExchange": "ARCA",
                    "build_options_chain": True,
                    "min_expiry_days": 7,
                    "max_expiry_days": 30,
                },
                # QQQ options
                {
                    "secType": "STK",
                    "symbol": "QQQ",
                    "exchange": "SMART",
                    "primaryExchange": "NASDAQ",
                    "build_options_chain": True,
                    "min_expiry_days": 7,
                    "max_expiry_days": 30,
                },
                # ES futures options
                {
                    "secType": "CONTFUT",
                    "exchange": "CME",
                    "symbol": "ES",
                    "build_options_chain": True,
                    "min_expiry_days": 0,
                    "max_expiry_days": 60,
                },
            ),
        },
    )
```

### Retrieving historical bars

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

### Retrieving historical ticks

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

### Bar specifications

The adapter supports various bar specifications:

#### Time-based bars

- `"1-SECOND-LAST"`, `"5-SECOND-LAST"`, `"10-SECOND-LAST"`, `"15-SECOND-LAST"`, `"30-SECOND-LAST"`
- `"1-MINUTE-LAST"`, `"2-MINUTE-LAST"`, `"3-MINUTE-LAST"`, `"5-MINUTE-LAST"`, `"10-MINUTE-LAST"`, `"15-MINUTE-LAST"`, `"20-MINUTE-LAST"`, `"30-MINUTE-LAST"`
- `"1-HOUR-LAST"`, `"2-HOUR-LAST"`, `"3-HOUR-LAST"`, `"4-HOUR-LAST"`, `"8-HOUR-LAST"`
- `"1-DAY-LAST"`, `"1-WEEK-LAST"`, `"1-MONTH-LAST"`

#### Price types

- `LAST` - Last traded price
- `MID` - Midpoint of bid/ask
- `BID` - Bid price
- `ASK` - Ask price

### Complete example

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

### Data limitations

Be aware of Interactive Brokers' historical data limitations:

- **Rate Limits**: IB enforces rate limits on historical data requests
- **Data Availability**: Historical data availability varies by instrument and subscription level
- **Market Data Permissions**: Some data requires specific market data subscriptions
- **Time Ranges**: Maximum lookback periods vary by bar size and instrument type

### Best practices

1. **Use Delayed Data**: For backtesting, `MarketDataTypeEnum.DELAYED_FROZEN` is often sufficient
2. **Batch Requests**: Group multiple instruments in single requests when possible
3. **Handle Timeouts**: Set appropriate timeout values for large data requests
4. **Respect Rate Limits**: Add delays between requests to avoid hitting rate limits
5. **Validate Data**: Always check data quality and completeness before backtesting

:::warning
Interactive Brokers enforces pacing limits; excessive historical-data or order requests trigger pacing violations and IB can disable the API session for several minutes.
:::

## Live trading

Live trading with Interactive Brokers requires setting up a `TradingNode` that incorporates both `InteractiveBrokersDataClient` and `InteractiveBrokersExecutionClient`. These clients depend on the `InteractiveBrokersInstrumentProvider` for instrument management.

### Architecture overview

The live trading setup consists of three main components:

1. **InstrumentProvider**: Manages instrument definitions and contract details
2. **DataClient**: Handles real-time market data subscriptions
3. **ExecutionClient**: Manages orders, positions, and account information

### InstrumentProvider configuration

The `InteractiveBrokersInstrumentProvider` serves as the bridge for accessing financial instrument data from IB. It supports loading individual instruments, options chains, and futures chains.

#### Basic configuration

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

#### Advanced configuration for derivatives

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

### Integration with external data providers

The Interactive Brokers adapter can be used alongside other data providers for enhanced market data coverage. When using multiple data sources:

- Use consistent symbology methods across providers
- Consider using `convert_exchange_to_mic_venue=True` for standardized venue identification
- Ensure instrument cache management is handled properly to avoid conflicts

### Data client configuration

The `InteractiveBrokersDataClient` interfaces with IB for streaming and retrieving real-time market data. Upon connection, it configures the [market data type](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/#delayed-market-data) and loads instruments based on the `InteractiveBrokersInstrumentProviderConfig` settings.

#### Supported data types

- **Quote Ticks**: Real-time bid/ask prices and sizes
- **Trade Ticks**: Real-time trade prices and volumes
- **Bar Data**: Real-time OHLCV bars (1-second to 1-day intervals)
- **Market Depth**: Level 2 order book data (where available)

#### Market data types

Interactive Brokers supports several market data types:

- `REALTIME`: Live market data (requires market data subscriptions)
- `DELAYED`: 15-20 minute delayed data (free for most markets)
- `DELAYED_FROZEN`: Delayed data that doesn't update (useful for testing)
- `FROZEN`: Last known real-time data (when market is closed)

#### Basic data client configuration

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

#### Advanced data client configuration

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

### Data client configuration options

| Option                          | Default                                         | Description |
|---------------------------------|-------------------------------------------------|-------------|
| `instrument_provider`           | `InteractiveBrokersInstrumentProviderConfig()`  | Instrument provider settings controlling which contracts load at startup. |
| `ibg_host`                      | `127.0.0.1`                                     | Hostname or IP for TWS/IB Gateway. |
| `ibg_port`                      | `None`                                          | Port for TWS/IB Gateway (`7497`/`7496` for TWS, `4002`/`4001` for IBG). |
| `ibg_client_id`                 | `1`                                             | Unique client identifier used when connecting to TWS/IB Gateway. |
| `use_regular_trading_hours`     | `True`                                          | Request bars limited to regular trading hours when `True`. |
| `market_data_type`              | `REALTIME`                                      | Market data feed type (`REALTIME`, `DELAYED`, `DELAYED_FROZEN`, etc.). |
| `ignore_quote_tick_size_updates`| `False`                                         | Suppress quote ticks where only size changes when `True`. |
| `dockerized_gateway`            | `None`                                          | Optional `DockerizedIBGatewayConfig` for containerized setups. |
| `connection_timeout`            | `300`                                           | Seconds to wait for the initial API connection. |
| `request_timeout`               | `60`                                            | Seconds to wait for historical data requests before timing out. |

#### Notes

- **`use_regular_trading_hours`**: When `True`, only requests data during regular trading hours. Primarily affects bar data for stocks.
- **`ignore_quote_tick_size_updates`**: When `True`, filters out quote ticks where only the size changed (not price), reducing data volume.
- **`handle_revised_bars`**: When `True`, processes bar revisions from IB (bars can be updated after initial publication).
- **`connection_timeout`**: Maximum time to wait for initial connection establishment.
- **`request_timeout`**: Maximum time to wait for historical data requests.

### Execution client configuration options

| Option                                  | Default                                         | Description |
|-----------------------------------------|-------------------------------------------------|-------------|
| `instrument_provider`                   | `InteractiveBrokersInstrumentProviderConfig()`  | Instrument provider settings controlling which contracts load at startup. |
| `ibg_host`                              | `127.0.0.1`                                     | Hostname or IP for TWS/IB Gateway. |
| `ibg_port`                              | `None`                                          | Port for TWS/IB Gateway (`7497`/`7496` for TWS, `4002`/`4001` for IBG). |
| `ibg_client_id`                         | `1`                                             | Unique client identifier used when connecting to TWS/IB Gateway. |
| `account_id`                            | `None`                                          | Interactive Brokers account identifier (falls back to `TWS_ACCOUNT` env var). |
| `dockerized_gateway`                    | `None`                                          | Optional `DockerizedIBGatewayConfig` for containerized setups. |
| `connection_timeout`                    | `300`                                           | Seconds to wait for the initial API connection. |
| `fetch_all_open_orders`                 | `False`                                         | When `True`, pulls open orders for every API client ID (not just this session). |
| `track_option_exercise_from_position_update` | `False`                                    | Subscribe to real-time position updates to detect option exercises when `True`. |

### Execution client configuration

The `InteractiveBrokersExecutionClient` handles trade execution, order management, account information, and position tracking. It provides comprehensive order lifecycle management and real-time account updates.

#### Supported functionality

- **Order Management**: Place, modify, and cancel orders
- **Order Types**: Market, limit, stop, stop-limit, trailing stop, and more
- **Account Information**: Real-time balance and margin updates
- **Position Tracking**: Real-time position updates and P&L
- **Trade Reporting**: Execution reports and fill notifications
- **Risk Management**: Pre-trade risk checks and position limits

#### Supported order types

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

#### Time in force options

- **Day Orders**: `TimeInForce.DAY`
- **Good-Till-Canceled**: `TimeInForce.GTC`
- **Immediate-or-Cancel**: `TimeInForce.IOC`
- **Fill-or-Kill**: `TimeInForce.FOK`
- **Good-Till-Date**: `TimeInForce.GTD`
- **At-the-Open**: `TimeInForce.AT_THE_OPEN`
- **At-the-Close**: `TimeInForce.AT_THE_CLOSE`

#### Batch operations

| Operation          | Supported | Notes                                        |
|--------------------|-----------|----------------------------------------------|
| Batch Submit       | ✓         | Submit multiple orders in single request.    |
| Batch Modify       | ✓         | Modify multiple orders in single request.    |
| Batch Cancel       | ✓         | Cancel multiple orders in single request.    |

#### Position management

| Feature              | Supported | Notes                                        |
|--------------------|-----------|----------------------------------------------|
| Query positions     | ✓         | Real-time position updates.                  |
| Position mode       | ✓         | Net vs separate long/short positions.       |
| Leverage control    | ✓         | Account-level margin requirements.          |
| Margin mode         | ✓         | Portfolio vs individual margin.             |

#### Order querying

| Feature              | Supported | Notes                                        |
|--------------------|-----------|----------------------------------------------|
| Query open orders   | ✓         | List all active orders.                      |
| Query order history | ✓         | Historical order data.                       |
| Order status updates| ✓         | Real-time order state changes.              |
| Trade history       | ✓         | Execution and fill reports.                 |

#### Contingent orders

| Feature              | Supported | Notes                                        |
|--------------------|-----------|----------------------------------------------|
| Order lists         | ✓         | Atomic multi-order submission.               |
| OCO orders          | ✓         | One-Cancels-Other with customizable OCA types (1, 2, 3). |
| Bracket orders      | ✓         | Parent-child order relationships. |
| Conditional orders  | ✓         | Advanced order conditions and triggers.     |

#### Basic execution client configuration

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

#### Advanced execution client configuration

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

#### Account ID configuration

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

#### Order tags and advanced features

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

# Apply tags to an order
order = order_factory.limit(
    instrument_id=instrument.id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(100),
    price=instrument.make_price(100.0),
    tags=[order_tags.value],
)
```

#### OCA (one-cancels-all) orders

The adapter provides comprehensive support for OCA orders through explicit configuration using `IBOrderTags`:

### Basic OCA configuration

All OCA functionality must be explicitly configured using `IBOrderTags`:

```python
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags

# Create OCA configuration
oca_tags = IBOrderTags(
    ocaGroup="MY_OCA_GROUP",
    ocaType=1,  # Type 1: Cancel All with Block (recommended)
)

# Apply to bracket orders
bracket_order = order_factory.bracket(
    instrument_id=instrument.id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(100),
    tp_price=instrument.make_price(110.0),
    sl_trigger_price=instrument.make_price(90.0),
    tp_tags=[oca_tags.value],  # Must explicitly add OCA tags
    sl_tags=[oca_tags.value],  # Must explicitly add OCA tags
)
```

### Advanced OCA configuration

You can specify different OCA types and behaviors using `IBOrderTags`:

```python
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags

# Create custom OCA configuration
custom_oca_tags = IBOrderTags(
    ocaGroup="MY_CUSTOM_GROUP",
    ocaType=2,  # Use Type 2: Reduce with Block
)

# Apply to individual orders
order = order_factory.limit(
    instrument_id=instrument.id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(100),
    price=instrument.make_price(100.0),
    tags=[custom_oca_tags.value],
)
```

### OCA types

Interactive Brokers supports three OCA types:

| Type | Name | Behavior | Use Case |
|------|------|----------|----------|
| **1** | Cancel All with Block | Cancel all remaining orders with block protection | **Default** - Safest option, prevents overfills |
| **2** | Reduce with Block | Proportionally reduce remaining orders with block protection | Partial fills with overfill protection |
| **3** | Reduce without Block | Proportionally reduce remaining orders without block protection | Fastest execution, higher overfill risk |

#### Multiple orders in same OCA group

```python
# Create multiple orders with the same OCA group
oca_tags = IBOrderTags(
    ocaGroup="MULTI_ORDER_GROUP",
    ocaType=3,  # Use Type 3: Reduce without Block
)

order1 = order_factory.limit(
    instrument_id=instrument.id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(50),
    price=instrument.make_price(99.0),
    tags=[oca_tags.value],
)

order2 = order_factory.limit(
    instrument_id=instrument.id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(50),
    price=instrument.make_price(101.0),
    tags=[oca_tags.value],
)
```

### OCA configuration requirements

OCA functionality is **only** available through explicit configuration:

1. **IBOrderTags Required** - OCA settings must be explicitly specified in order tags
2. **No Automatic Detection** - `ContingencyType.OCO` and `ContingencyType.OUO` do not automatically create OCA groups
3. **Manual Configuration** - All OCA groups and types must be manually specified

### Conditional orders

The adapter supports Interactive Brokers conditional orders through the `conditions` parameter in `IBOrderTags`. Conditional orders allow you to specify criteria that must be met before an order is transmitted or cancelled.

#### Supported condition types

- **Price Conditions**: Trigger based on price movements of a specific instrument
- **Time Conditions**: Trigger at a specific date and time
- **Volume Conditions**: Trigger based on trading volume thresholds
- **Execution Conditions**: Trigger when trades occur for a specific instrument
- **Margin Conditions**: Trigger based on account margin levels
- **Percent Change Conditions**: Trigger based on percentage price changes

#### Basic conditional order example

```python
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags

# Create a price condition: trigger when SPY goes above $250
price_condition = {
    "type": "price",
    "conId": 265598,  # SPY contract ID
    "exchange": "SMART",
    "isMore": True,  # Trigger when price is greater than threshold
    "price": 250.00,
    "triggerMethod": 0,  # Default trigger method
    "conjunction": "and",
}

# Create order tags with condition
order_tags = IBOrderTags(
    conditions=[price_condition],
    conditionsCancelOrder=False,  # Transmit order when condition is met
)

# Apply to order
order = order_factory.limit(
    instrument_id=instrument.id,
    order_side=OrderSide.BUY,
    quantity=instrument.make_qty(100),
    price=instrument.make_price(251.00),
    tags=[order_tags.value],
)
```

#### Multiple conditions with logic

```python
# Create multiple conditions with AND/OR logic
conditions = [
    {
        "type": "price",
        "conId": 265598,
        "exchange": "SMART",
        "isMore": True,
        "price": 250.00,
        "triggerMethod": 0,
        "conjunction": "and",  # AND with next condition
    },
    {
        "type": "time",
        "time": "20250315-09:30:00",
        "isMore": True,
        "conjunction": "or",  # OR with next condition
    },
    {
        "type": "volume",
        "conId": 265598,
        "exchange": "SMART",
        "isMore": True,
        "volume": 10000000,
        "conjunction": "and",
    },
]

order_tags = IBOrderTags(
    conditions=conditions,
    conditionsCancelOrder=False,
)
```

#### Condition parameters

**Price Condition:**

- `conId`: Contract ID of the instrument to monitor
- `exchange`: Exchange to monitor (e.g., "SMART", "NASDAQ")
- `isMore`: True for >=, False for <=
- `price`: Price threshold
- `triggerMethod`: 0=Default, 1=DoubleBidAsk, 2=Last, 3=DoubleLast, 4=BidAsk, 7=LastBidAsk, 8=MidPoint

**Time Condition:**

- `time`: Time string in UTC format "YYYYMMDD-HH:MM:SS" (e.g., "20250315-09:30:00")
- `isMore`: True for after time, False for before time

**Volume Condition:**

- `conId`: Contract ID of the instrument to monitor
- `exchange`: Exchange to monitor
- `isMore`: True for >=, False for <=
- `volume`: Volume threshold

**Execution Condition:**

- `symbol`: Symbol to monitor for trades
- `secType`: Security type (e.g., "STK", "OPT", "FUT")
- `exchange`: Exchange to monitor

**Margin Condition:**

- `percent`: Margin cushion percentage threshold
- `isMore`: True for >=, False for <=

**Percent Change Condition:**

- `conId`: Contract ID of the instrument to monitor
- `exchange`: Exchange to monitor
- `isMore`: True for >=, False for <=
- `changePercent`: Percentage change threshold

#### Complete example: all condition types

```python
# Example showing all 6 supported condition types
from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags

# 1. Price Condition - trigger when ES futures > 6000
price_condition = {
    "type": "price",
    "conId": 495512563,  # ES futures contract ID
    "exchange": "CME",
    "isMore": True,
    "price": 6000.0,
    "triggerMethod": 0,
    "conjunction": "and",
}

# 2. Time Condition - trigger at specific time
time_condition = {
    "type": "time",
    "time": "20250315-09:30:00",  # UTC format
    "isMore": True,
    "conjunction": "and",
}

# 3. Volume Condition - trigger when volume > 100,000
volume_condition = {
    "type": "volume",
    "conId": 495512563,
    "exchange": "CME",
    "isMore": True,
    "volume": 100000,
    "conjunction": "and",
}

# 4. Execution Condition - trigger when SPY trades
execution_condition = {
    "type": "execution",
    "symbol": "SPY",
    "secType": "STK",
    "exchange": "SMART",
    "conjunction": "and",
}

# 5. Margin Condition - trigger when margin cushion > 75%
margin_condition = {
    "type": "margin",
    "percent": 75,
    "isMore": True,
    "conjunction": "and",
}

# 6. Percent Change Condition - trigger when price changes > 5%
percent_change_condition = {
    "type": "percent_change",
    "conId": 495512563,
    "exchange": "CME",
    "changePercent": 5.0,
    "isMore": True,
    "conjunction": "and",
}

# Use any combination of conditions
order_tags = IBOrderTags(
    conditions=[price_condition, time_condition],  # Multiple conditions
    conditionsCancelOrder=False,  # Transmit when conditions met
)
```

#### Order behavior

Set `conditionsCancelOrder` to control what happens when conditions are met:

- `False`: Transmit the order when conditions are satisfied
- `True`: Cancel the order when conditions are satisfied

#### Implementation notes

- **All 6 condition types are fully supported** and tested with live Interactive Brokers orders
- **Price conditions** work correctly despite a known bug in the ibapi library where `PriceCondition.__str__` is incorrectly decorated as a property
- **Time conditions** use UTC format with dash separator (`YYYYMMDD-HH:MM:SS`) for reliable parsing
- **Conjunction logic** allows complex condition combinations using "and"/"or" operators

### Complete trading node configuration

Setting up a complete trading environment involves configuring a `TradingNodeConfig` with all necessary components. Here are comprehensive examples for different scenarios.

#### Paper trading configuration

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

## Live trading with Dockerized gateway

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

### Multi-client configuration

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

### Running the trading node

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

### Additional configuration options

#### Environment variables

Set these environment variables for easier configuration:

```bash
export TWS_USERNAME="your_ib_username"
export TWS_PASSWORD="your_ib_password"
export TWS_ACCOUNT="your_account_id"
export IB_MAX_CONNECTION_ATTEMPTS="5"  # Optional: limit reconnection attempts
```

#### Logging configuration

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

### Common connection issues

#### Connection refused

- **Cause**: TWS/Gateway not running or wrong port
- **Solution**: Verify TWS/Gateway is running and check port configuration
- **Default Ports**: TWS (7497/7496), IB Gateway (4002/4001)

#### Authentication errors

- **Cause**: Incorrect credentials or account not logged in
- **Solution**: Verify username/password and ensure account is logged into TWS/Gateway

#### Client ID conflicts

- **Cause**: Multiple clients using the same client ID
- **Solution**: Use unique client IDs for each connection

#### Market data permissions

- **Cause**: Insufficient market data subscriptions
- **Solution**: Use `IBMarketDataTypeEnum.DELAYED_FROZEN` for testing or subscribe to required data feeds

### Error codes

Interactive Brokers uses specific error codes. Common ones include:

- **200**: No security definition found
- **201**: Order rejected - reason follows
- **202**: Order cancelled
- **300**: Can't find EId with ticker ID
- **354**: Requested market data is not subscribed
- **2104**: Market data farm connection is OK
- **2106**: HMDS data farm connection is OK

### Performance optimization

#### Reduce data volume

```python
# Reduce quote tick volume by ignoring size-only updates
data_config = InteractiveBrokersDataClientConfig(
    ignore_quote_tick_size_updates=True,
    # ... other config
)
```

#### Connection management

```python
# Set reasonable timeouts
config = InteractiveBrokersDataClientConfig(
    connection_timeout=300,  # 5 minutes
    request_timeout=60,      # 1 minute
    # ... other config
)
```

#### Memory management

- Use appropriate bar sizes for your strategy
- Limit the number of simultaneous subscriptions
- Consider using historical data for backtesting instead of live data

### Best practices

#### Security

- Never hardcode credentials in source code
- Use environment variables for sensitive information
- Use paper trading for development and testing
- Set `read_only_api=True` for data-only applications

#### Development workflow

1. **Start with Paper Trading**: Always test with paper trading first
2. **Use Delayed Data**: Use `DELAYED_FROZEN` market data for development
3. **Implement Proper Error Handling**: Handle connection losses and API errors gracefully
4. **Monitor Logs**: Enable appropriate logging levels for debugging
5. **Test Reconnection**: Test your strategy's behavior during connection interruptions

#### Production deployment

- Use dockerized gateway for automated deployments
- Implement proper monitoring and alerting
- Set up log aggregation and analysis
- Use real-time data subscriptions only when necessary
- Implement circuit breakers and position limits

#### Order management

- Always validate orders before submission
- Implement proper position sizing
- Use appropriate order types for your strategy
- Monitor order status and handle rejections
- Implement timeout handling for order operations

### Debugging tips

#### Enable debug logging

```python
logging_config = LoggingConfig(
    log_level="DEBUG",
    log_component_levels={
        "InteractiveBrokersClient": "DEBUG",
    },
)
```

#### Monitor connection status

```python
# Check connection status in your strategy
if not self.data_client.is_connected:
    self.log.warning("Data client disconnected")
```

#### Validate instruments

```python
# Ensure instruments are loaded before trading
instruments = self.cache.instruments()
if not instruments:
    self.log.error("No instruments loaded")
```

### Support and resources

- **IB API Documentation**: [TWS API Guide](https://ibkrcampus.com/ibkr-api-page/trader-workstation-api/)
- **NautilusTrader Examples**: [GitHub Examples](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/live/interactive_brokers)
- **IB Contract Search**: [Contract Information Center](https://pennies.interactivebrokers.com/cstools/contract_info/)
- **Market Data Subscriptions**: [IB Market Data](https://www.interactivebrokers.com/en/trading/market-data.php)
