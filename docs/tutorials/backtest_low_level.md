# Backtest (low-level API)

**This tutorial walks through how to use a `BacktestEngine` to backtest a simple EMA cross strategy
with a TWAP execution algorithm on a simulated Binance Spot exchange using historical trade tick data.**

The following points will be covered:
- How to load raw data (external to Nautilus) using data loaders and wranglers
- How to add this data to a `BacktestEngine`
- How to add venues, strategies and execution algorithms to a `BacktestEngine`
- How to run backtests with a  `BacktestEngine`
- Post-run analysis and options for repeated runs

## Imports

We'll start with all of our imports for the remainder of this tutorial:

```python
import time
from decimal import Decimal

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.examples.algorithms.twap import TWAPExecAlgorithm
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAP
from nautilus_trader.examples.strategies.ema_cross_twap import EMACrossTWAPConfig
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
```

## Loading data

For this tutorial we'll use some stub test data which exists in the NautilusTrader repository
(this data is also used by the automated test suite to test the correctness of the platform).

Firstly, instantiate a data provider which we can use to read raw CSV trade tick data into memory as a `pd.DataFrame`.
We then need to initialize the instrument which matches the data, in this case the `ETHUSDT` spot cryptocurrency pair for Binance.
We'll use this instrument for the remainder of this backtest run.

Next, we need to wrangle this data into a list of Nautilus `TradeTick` objects, which can we later add to the `BacktestEngine`:

```python
# Load stub test data
provider = TestDataProvider()
trades_df = provider.read_csv_ticks("binance-ethusdt-trades.csv")

# Initialize the instrument which matches the data
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()

# Process into Nautilus objects
wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
ticks = wrangler.process(trades_df)
```

See the [Data](../concepts/data.md) guide for a more detailed explanation of the typical data processing components and pipeline.

## Initialize a backtest engine

Now we'll need a backtest engine, minimally you could just call `BacktestEngine()` which will instantiate
an engine with a default configuration. 

Here we also show initializing a `BacktestEngineConfig` (will only a custom `trader_id` specified)
to show the general configuration pattern:

```python
# Configure backtest engine
config = BacktestEngineConfig(trader_id="BACKTESTER-001")

# Build the backtest engine
engine = BacktestEngine(config=config)

```

See the [Configuration](../api_reference/config.md) API reference for details of all configuration options available.

## Adding data

Now we can add data to the backtest engine. First add the `Instrument` object we previously initialized, which matches our data.

Then we can add the trade ticks we wrangled earlier:
```python
# Add instrument(s)
engine.add_instrument(ETHUSDT_BINANCE)

# Add data
engine.add_data(ticks)

```

```{note}
The amount of and variety of data types is only limited by machine resources and your imagination (custom types are possible).
```

## Adding venues

We'll need a venue to trade on, which should match the *market* data being added to the engine.

In this case we'll setup a *simulated* Binance Spot exchange:

```python
# Add a trading venue (multiple venues possible)
BINANCE = Venue("BINANCE")
engine.add_venue(
    venue=BINANCE,
    oms_type=OmsType.NETTING,
    account_type=AccountType.CASH,  # Spot CASH account (not for perpetuals or futures)
    base_currency=None,  # Multi-currency account
    starting_balances=[Money(1_000_000.0, USDT), Money(10.0, ETH)],
)

```

```{note}
Multiple venues can be used for backtesting, only limited by machine resources.
```

## Adding strategies

Now we can add the trading strategies we'd like to run as part of our system.

```{note}
Multiple strategies and instruments can be used for backtesting, only limited by machine resources.
```

Firstly, initialize a strategy configuration, then use this to initialize a strategy which we can add to the engine:
```python

# Configure your strategy
strategy_config = EMACrossTWAPConfig(
    instrument_id=str(ETHUSDT_BINANCE.id),
    bar_type="ETHUSDT.BINANCE-250-TICK-LAST-INTERNAL",
    trade_size=Decimal("0.10"),
    fast_ema_period=10,
    slow_ema_period=20,
    twap_horizon_secs=10.0,
    twap_interval_secs=2.5,
)

# Instantiate and add your strategy
strategy = EMACrossTWAP(config=strategy_config)
engine.add_strategy(strategy=strategy)

```

You may notice that this strategy config includes parameters related to a TWAP execution algorithm.
This is because we can flexibly use different parameters per order submit, we still need to initialize
and add the actual `ExecAlgorithm` component which will execute the algorithm - which we'll do now.

## Adding execution algorithms

NautilusTrader allows us to build up very complex systems of custom components. Here we show just one of the custom components
available, in this case a built-in TWAP execution algorithm. It is configured and added to the engine in generally the same pattern as for strategies:

```{note}
Multiple execution algorithms can be used for backtesting, only limited by machine resources.
```

```python
# Instantiate and add your execution algorithm
exec_algorithm = TWAPExecAlgorithm()  # Using defaults
engine.add_exec_algorithm(exec_algorithm)

```

## Running backtests

Now that we have our data, venues and trading system configured - we can run a backtest!
Simply call the `.run(...)` method which will run a backtest over all available data by default:

```python
# Run the engine (from start to end of data)
engine.run()
```

See the [BacktestEngine](../api_reference/backtest.md) API reference for a complete description of all available methods and options.

## Post-run and analysis

Once the backtest is completed, a post-run tearsheet will be automatically logged using some
default statistics (or custom statistics which can be loaded, see the advanced [Portfolio statistics](../concepts/advanced/portfolio_statistics.md) guide).

Also, many resultant data and execution objects will be held in memory, which we
can use to further analyze the performance by generating various reports:

```python
# Optionally view reports
with pd.option_context(
    "display.max_rows",
    100,
    "display.max_columns",
    None,
    "display.width",
    300,
):
    print(engine.trader.generate_account_report(BINANCE))
    print(engine.trader.generate_order_fills_report())
    print(engine.trader.generate_positions_report())
```

## Repeated runs

We can also choose to reset the engine for repeated runs with different strategy and component configurations.
Calling the `.reset(...)` method will retain all loaded data and components, but reset all other stateful values
as if we had a fresh `BacktestEngine` (this avoids having to load the same data again):

```python

# For repeated backtest runs make sure to reset the engine
engine.reset()
```

Individual components (actors, strategies, execution algorithms) need to be removed and added as required.

See the [Trader](../api_reference/trading.md) API reference for a description of all methods available to achieve this.


```python
# Once done, good practice to dispose of the object if the script continues
engine.dispose()
```
