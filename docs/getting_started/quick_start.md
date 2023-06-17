# Quick Start

This guide explains how to get up and running with NautilusTrader backtesting with some
FX data. The Nautilus maintainers have pre-loaded some test data using the standard Nautilus persistence 
format (Parquet) for this guide.

For more details on how to load data into Nautilus, see [Backtest Example](../guides/backtest_example.md).

## Running in docker
A self-contained dockerized jupyter notebook server is available for download, which does not require any setup or 
installation. This is the fastest way to get up and running to try out Nautilus. Bear in mind that any data will be 
deleted when the container is deleted. 

- To get started, install docker:
  - Go to [docker.com](https://docs.docker.com/get-docker/) and follow the instructions 
- From a terminal, download the latest image
  - `docker pull ghcr.io/nautechsystems/jupyterlab:develop`
- Run the docker container, exposing the jupyter port: 
  - `docker run -p 8888:8888 ghcr.io/nautechsystems/jupyterlab:develop`
- Open your web browser to `localhost:{port}`
  - https://localhost:8888

```{warning}
NautilusTrader currently exceeds the rate limit for Jupyter notebook logging (stdout output),
this is why `log_level` in the examples is set to "ERROR". If you lower this level to see
more logging then the notebook will hang during cell execution. A fix is currently
being investigated which involves either raising the configured rate limits for
Jupyter, or throttling the log flushing from Nautilus.
https://github.com/jupyterlab/jupyterlab/issues/12845
https://github.com/deshaw/jupyterlab-limit-output
```

## Getting the sample data

To save time, we have prepared a script to load sample data into the Nautilus format for use with this example. 
First, download and load the data by running the next cell (this should take ~ 1-2 mins):

```bash
!apt-get update && apt-get install curl -y
!curl https://raw.githubusercontent.com/nautechsystems/nautilus_data/main/scripts/hist_data_to_catalog.py | python - 
```

## Connecting to the ParquetDataCatalog

If everything worked correctly, you should be able to see a single EUR/USD instrument in the catalog:

```python
from nautilus_trader.persistence.catalog import ParquetDataCatalog

# You can also use `ParquetDataCatalog.from_env()` which will use the `NAUTILUS_PATH` environment variable 
# catalog = ParquetDataCatalog.from_env()
catalog = ParquetDataCatalog("./catalog")
catalog.instruments()
```

## Writing a trading strategy

NautilusTrader includes a handful of indicators built-in, in this example we will use a MACD indicator to 
build a simple trading strategy. 

You can read more about [MACD here](https://www.investopedia.com/terms/m/macd.asp), so this 
indicator merely serves as an example without any expected alpha. There is also a way of
registering indicators to receive certain data types, however in this example we manually pass the received
`QuoteTick` to the indicator in the `on_quote_tick` method.

```python
from typing import Optional
from nautilus_trader.core.message import Event
from nautilus_trader.indicators.macd import MovingAverageConvergenceDivergence
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.strategy import Strategy, StrategyConfig


class MACDConfig(StrategyConfig):
    instrument_id: str
    fast_period: int = 12
    slow_period: int = 26
    trade_size: int = 1_000_000
    entry_threshold: float = 0.00010


class MACDStrategy(Strategy):
    def __init__(self, config: MACDConfig) -> None:
        super().__init__(config=config)
        # Our "trading signal"
        self.macd = MovingAverageConvergenceDivergence(
            fast_period=config.fast_period, slow_period=config.slow_period, price_type=PriceType.MID
        )
        # We copy some config values onto the class to make them easier to reference later on
        self.entry_threshold = config.entry_threshold
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.trade_size = Quantity.from_int(config.trade_size)

        # Convenience
        self.position: Optional[Position] = None

    def on_start(self):
        self.subscribe_quote_ticks(instrument_id=self.instrument_id)

    def on_stop(self):
        self.close_all_positions(self.instrument_id)
        self.unsubscribe_quote_ticks(instrument_id=self.instrument_id)

    def on_quote_tick(self, tick: QuoteTick):
        # You can register indicators to receive quote tick updates automatically,
        # here we manually update the indicator to demonstrate the flexibility available.
        self.macd.handle_quote_tick(tick)

        if not self.macd.initialized:
            return  # Wait for indicator to warm up
        
        # self._log.info(f"{self.macd.value=}:%5d")
        self.check_for_entry()
        self.check_for_exit()

    def on_event(self, event):
        if isinstance(event, PositionOpened):
            self.position = self.cache.position(event.position_id)

    def check_for_entry(self):
        # If MACD line is above our entry threshold, we should be LONG
        if self.macd.value > self.entry_threshold:
            if self.position and self.position.side == PositionSide.LONG:
                return  # Already LONG

            order = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=self.trade_size,
            )
            self.submit_order(order)
        # If MACD line is below our entry threshold, we should be SHORT
        elif self.macd.value < -self.entry_threshold:
            if self.position and self.position.side == PositionSide.SHORT:
                return  # Already SHORT

            order = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=self.trade_size,
            )
            self.submit_order(order)

    def check_for_exit(self):
        # If MACD line is above zero then exit if we are SHORT
        if self.macd.value >= 0.0:
            if self.position and self.position.side == PositionSide.SHORT:
                self.close_position(self.position)
        # If MACD line is below zero then exit if we are LONG
        else:
            if self.position and self.position.side == PositionSide.LONG:
                self.close_position(self.position)

    def on_dispose(self):
        pass  # Do nothing else

```

## Configuring Backtests

Now that we have a trading strategy and data, we can begin to configure a backtest run! Nautilus uses a `BacktestNode` 
to orchestrate backtest runs, which requires some setup. This may seem a little complex at first, 
however this is necessary for the capabilities that Nautilus strives for.

To configure a `BacktestNode`, we first need to create an instance of a `BacktestRunConfig`, configuring the 
following (minimal) aspects of the backtest:

- `engine` - The engine for the backtest representing our core system, which will also contain our strategies
- `venues` - The simulated venues (exchanges or brokers) available in the backtest
- `data` - The input data we would like to perform the backtest on

There are many more configurable features which will be described later in the docs, for now this will get us up and running.

## Venue

First, we create a venue configuration. For this example we will create a simulated FX ECN. 
A venue needs a name which acts as an ID (in this case `SIM`), as well as some basic configuration, e.g. 
the account type (`CASH` vs `MARGIN`), an optional base currency, and starting balance(s).

```{note}
FX trading is typically done on margin with Non-Deliverable Forward, Swap or CFD type instruments.
```

```python
from nautilus_trader.config import BacktestVenueConfig

venue = BacktestVenueConfig(
    name="SIM",
    oms_type="NETTING",
    account_type="MARGIN",
    base_currency="USD",
    starting_balances=["1_000_000 USD"]
)
```

## Instruments

Second, we need to know about the instruments that we would like to load data for, we can use the `ParquetDataCatalog` for this:

```python
instruments = catalog.instruments(as_nautilus=True)
instruments
```

## Data

Next, we need to configure the data for the backtest. Nautilus is built to be very flexible when it 
comes to loading data for backtests, however this also means some configuration is required.

For each tick type (and instrument), we add a `BacktestDataConfig`. In this instance we are simply 
adding the `QuoteTick`(s) for our EUR/USD instrument:

```python
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.model.data import QuoteTick

data = BacktestDataConfig(
    catalog_path=str(catalog.path),
    data_cls=QuoteTick,
    instrument_id=str(instruments[0].id),
    end_time="2020-01-10",
)
```

## Engine

Then, we need a `BacktestEngineConfig` which represents the configuration of our core trading system.
Here we need to pass our trading strategies, we can also adjust the log level 
and configure many other components (however, it's also fine to use the defaults):

Strategies are added via the `ImportableStrategyConfig`, which allows importing strategies from arbitrary files or 
user packages. In this instance, our `MACDStrategy` is defined in the current module, which python refers to as `__main__`.

```python
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import ImportableStrategyConfig
from nautilus_trader.config import LoggingConfig

engine = BacktestEngineConfig(
    strategies=[
        ImportableStrategyConfig(
            strategy_path="__main__:MACDStrategy",
            config_path="__main__:MACDConfig",
            config=dict(
              instrument_id=instruments[0].id.value,
              fast_period=12,
              slow_period=26,
            ),
        )
    ],
    logging=LoggingConfig(log_level="ERROR"),  # Lower to `INFO` to see more logging about orders, events, etc.
)
```

## Running a backtest

We can now pass our various config pieces to the `BacktestRunConfig`. This object now contains the 
full configuration for our backtest.


```python
from nautilus_trader.config import BacktestRunConfig


config = BacktestRunConfig(
    engine=engine,
    venues=[venue],
    data=[data],
)
```

The `BacktestNode` class will orchestrate the backtest run. The reason for this separation between 
configuration and execution is the `BacktestNode` allows running multiple configurations (different 
parameters or batches of data). 

We are now ready to run some backtests!

```python
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.results import BacktestResult


node = BacktestNode(configs=[config])

 # Runs one or many configs synchronously
results: list[BacktestResult] = node.run()
```

Now that the run is complete, we can also directly query for the `BacktestEngine`(s) used internally by the `BacktestNode`
by using the run configs ID. 

The engine(s) can provide additional reports and information.

```python
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.model.identifiers import Venue

engine: BacktestEngine = node.get_engine(config.id)

engine.trader.generate_order_fills_report()
```

```python
engine.trader.generate_positions_report()
```

```python
engine.trader.generate_account_report(Venue("SIM"))
```
