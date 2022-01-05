---
jupyter:
  jupytext:
    formats: ipynb,md
    text_representation:
      extension: .md
      format_name: markdown
      format_version: '1.3'
      jupytext_version: 1.13.5
  kernelspec:
    display_name: Python (nautilus_trader_gh)
    language: python
    name: nautilus_trader_gh
---

### Quick start

This section explains how to get up and running with Nautilus Trader by running some backtests on some Forex data. The Nautilus maintainers have pre-loaded some existing data into the nautilus storage format (parquet) for this guide.

For more details on how to load other data into Nautilus, see [Backtest Example](../2_user_guide/3_backtest_example.md)


#### Getting the sample data

We have prepared some sample data in the nautilus parquet format for use with this example. First, download and load the data (this should take ~60s):

```python


def download(url):
    import requests 
    filename = url.rsplit("/", maxsplit=1)[1]
    with open(filename, 'wb') as f:
        f.write(requests.get(url).content)


# Download raw data
download("https://raw.githubusercontent.com/nautechsystems/nautilus_data/main/raw_data/fx_hist_data/DAT_ASCII_EURUSD_T_202001.csv.gz")

# Download processing script
download("https://raw.githubusercontent.com/nautechsystems/nautilus_data/main/scripts/hist_data_to_catalog.py")

from hist_data_to_catalog import load_fx_hist_data, os, shutil
load_fx_hist_data(
    filename="DAT_ASCII_EURUSD_T_202001.csv.gz",
    currency="EUR/USD",
    catalog_path="EUDUSD202001",
)

# Cleanup files
os.unlink('hist_data_to_catalog.py')
os.unlink("DAT_ASCII_EURUSD_T_202001.csv.gz")
```

### Connecting to the DataCatalog

If everything worked correctly, you should be able to see a single EURUSD instrument in the catalog

```python
from nautilus_trader.persistence.catalog import DataCatalog
```

```python
catalog = DataCatalog("EUDUSD202001/")
```

```python
catalog.instruments()
```

### Writing a trading strategy

Nautilus includes a handful of indicators built-in, in this example we will use a MACD indicator to build a simple trading strategy. You can read more about [MACD here](https://www.investopedia.com/terms/m/macd.asp), but this indicator merely serves as an example without any expected alpha.

```python
from nautilus_trader.trading.strategy import TradingStrategy, TradingStrategyConfig
from nautilus_trader.indicators.macd import MovingAverageConvergenceDivergence
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events.position import PositionEvent
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position



class MACDConfig(TradingStrategyConfig):
    instrument_id: str
    fast_period: int
    slow_period: int
    trade_size: int = 1000
    entry_threshold: float = 0.00010


class MACDStrategy(TradingStrategy):
    def __init__(self, config: MACDConfig):
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

    def on_quote_tick(self, tick: QuoteTick):
        # Update our MACD
        self.macd.handle_quote_tick(tick)
        if self.macd.value:
            # self._log.info(f"{self.macd.value=}:%5d")
            self.check_for_entry()
            self.check_for_exit()
        if self.position:
            assert self.position.quantity <= 1000

    def on_event(self, event):
        if isinstance(event, PositionEvent):
            self.position = self.cache.position(event.position_id)

    def check_for_entry(self):
        if self.cache.positions():
            # If we have a position, do not enter again
            return

        # We have no position, check if we are above or below our MACD entry threshold
        if abs(self.macd.value) > self.entry_threshold:
            self._log.info(f"Entering trade, {self.macd.value=}, {self.entry_threshold=}")
            # We're above (to sell) or below (to buy) our entry threshold, with no position: enter a trade
            side = OrderSide.BUY if self.macd.value < -self.entry_threshold else OrderSide.SELL
            order = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=side,
                quantity=self.trade_size,
            )
            self.submit_order(order)

    def check_for_exit(self):
        if not self.cache.positions():
            # If we don't have a position, return early
            return

        # We have a position, check if we have crossed back over the MACD 0 line (and therefore close position)
        if (self.position.is_long and self.macd.value > 0) or (self.position.is_short and self.macd.value < 0):
            self._log.info(f"Exiting trade, {self.macd.value=}")
            # We've crossed back over 0 line - close the position.
            # Opposite to trade entry, except only sell our position size (we may not have been full filled)
            side = OrderSide.SELL if self.position.is_long else OrderSide.BUY
            order = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=side,
                quantity=self.position.quantity,
            )
            self.submit_order(order)
```

<!-- #region pycharm={"name": "#%% md\n"} -->
### Configuing Backtests

Now that we have a trading strategy and data, we can run a backtest! Nautilus uses a `BacktestEngine` to configure and run backtests, and requires some setup. This may seem a little complex at first, but this is necessary for the correctness that Nautilus strives for.

To configure a `BacktestEngine`, we create an instance of a `BacktestRunConfig`, configuring the following (minimal) aspects of the backtest:
- `data` - The input data we would like to perform the backtest on
- `venues` - the simulated venues (exchanges or brokers) available in the backtest
- `strategies` - the strategy or strategies we would like to run for the backtest

There are many more configurable features which will be described later in the docs, for now this will get us up and running.
<!-- #endregion -->

<!-- #region pycharm={"name": "#%% md\n"} -->
#### Venue

First, we create a venue. For this example we will create a simulated venue for Oanda, a Forex broker. A venue needs a name, as well as some basic configuration; the account type (cash vs margin), the base currency and starting balance.
<!-- #endregion -->

```python jupyter={"outputs_hidden": false} pycharm={"name": "#%%\n"}
from nautilus_trader.backtest.config import BacktestVenueConfig

oanda_venue = BacktestVenueConfig(
    name="SIM",
    oms_type='NETTING',
    account_type='CASH',
    base_currency="USD",
    starting_balances=['100_000 USD']
)
```

<!-- #region -->
#### Instruments


Second, we need to know about the instruments that we would like to load data for, we can use the `DataCatalog` for this:
<!-- #endregion -->

```python
instruments = catalog.instruments(as_nautilus=True)
instruments
```

<!-- #region jupyter={"outputs_hidden": false} pycharm={"name": "#%% md\n"} -->
#### Data

Next, we need to configure the data for the backtest. Nautilus is built to be very flexible when it comes to loading data for backtests, but this also means configuration is required.

For each tick type (and instrument), we add a `BacktestDataConfig`. In this instance we are simply adding the `QuoteTick`s for our `EURUSD` instrument
<!-- #endregion -->

```python jupyter={"outputs_hidden": false} pycharm={"name": "#%%\n"}
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.backtest.config import BacktestDataConfig

data = [
    BacktestDataConfig(
        catalog_path=str(catalog.path),
        data_cls_path=f"{QuoteTick.__module__}.{QuoteTick.__name__}",
        instrument_id=str(instruments[0].id),
        end_time='2020-01-05',
    )
]
```

<!-- #region pycharm={"name": "#%% md\n"} -->
#### Engine

Then, we need a `BacktestEngineConfig` which allows configuring the log level and other components, but is fine to leave with its defaults
<!-- #endregion -->

```python jupyter={"outputs_hidden": false} pycharm={"name": "#%%\n"}
from nautilus_trader.backtest.config import BacktestEngineConfig

engine = BacktestEngineConfig(log_level='ERROR') # Lower to `INFO` to see more logging about orders, events, etc.
```

#### Strategies

And finally is our actual trading strategy(s).

```python
macd_config = MACDConfig(
    instrument_id=instruments[0].id.value,
    fast_period=12,
    slow_period=26,
)

macd_strategy = MACDStrategy(config=macd_config)
```

## Running a backtest

We can now pass our various config pieces to the `BacktestRunConfig` - this object now contains the full configuration for our backtest, we are ready to run some backtests!

The `BacktestNode` class _actually_ runs the backtest. The reason for this separation between configuration and execution is the `BacktestNode` allows running multiple configurations (different parameters or batches of data), as well as parallelisation via the excellent [dask](https://dask.org/] library.

```python pycharm={"name": "#%%\n"} tags=[]
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.backtest.config import BacktestRunConfig
from nautilus_trader.trading.config import ImportableStrategyConfig

config = BacktestRunConfig(
    venues=[oanda_venue],
    strategies=[macd_strategy],
    data=data,
    engine=engine,
)

node = BacktestNode()

 # run_sync runs one or many configs synchronously
[result] = node.run_sync(
    run_configs=[config], 
    return_engine=True # Return the full BacktestEngine (which contains much more detailed information) rather than the standard `BacktestResult`
)
```

```python
result.cache.orders()[:5]
```

```python
result.cache.positions()[:5]
```
