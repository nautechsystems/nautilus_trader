# Strategies

The heart of the NautilusTrader user experience is in writing and working with
trading strategies. Defining a trading strategy is achieved by inheriting the `Strategy` class, 
and implementing the methods required by the strategy.

Using the basic building blocks of data ingest, event handling, and order management (which we will discuss
below), it's possible to implement any type of trading strategy including directional, momentum, re-balancing,
pairs, market making etc.

Refer to the `Strategy` in the [API Reference](../api_reference/trading.md) for a complete description
of all the possible functionality.

There are two main parts of a Nautilus trading strategy:
- The strategy implementation itself, defined by inheriting the `Strategy` class
- The _optional_ strategy configuration, defined by inheriting the `StrategyConfig` class

```{note}
Once a strategy is defined, the same source can be used for backtesting and live trading.
```

## Implementation
Since a trading strategy is a class which inherits from `Strategy`, you must define
a constructor where you can handle initialization. Minimally the base/super class needs to be initialized:

```python
class MyStrategy(Strategy):
    def __init__(self) -> None:
        super().__init__()  # <-- the super class must be called to initialize the strategy
```

## Configuration
The main purpose of a separate configuration class is to provide total flexibility
over where and how a trading strategy can be instantiated. This includes being able
to serialize strategies and their configurations over the wire, making distributed backtesting
and firing up remote live trading possible.

This configuration flexibility is actually opt-in, in that you can actually choose not to have
any strategy configuration beyond the parameters you choose to pass into your
strategies' constructor. However, if you would like to run distributed backtests or launch
live trading servers remotely, then you will need to define a configuration.

Here is an example configuration:

```python
from decimal import Decimal
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


class MyStrategyConfig(StrategyConfig):
    instrument_id: str
    bar_type: str
    fast_ema_period: int = 10
    slow_ema_period: int = 20
    trade_size: Decimal
    order_id_tag: str

# Here we simply add an instrument ID as a string, to 
# parameterize the instrument the strategy will trade.

class MyStrategy(Strategy):
    def __init__(self, config: MyStrategyConfig) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)


# Once a configuration is defined and instantiated, we can pass this to our 
# trading strategy to initialize.

config = MyStrategyConfig(
    instrument_id="ETHUSDT-PERP.BINANCE",
    bar_type="ETHUSDT-PERP.BINANCE-1000-TICK[LAST]-INTERNAL",
    trade_size=Decimal(1),
    order_id_tag="001",
)

strategy = MyStrategy(config=config)

```

```{note}
Even though it often makes sense to define a strategy which will trade a single
instrument. There is actually no limit to the number of instruments a single strategy
can work with.
```

### Multiple strategies
If you intend running multiple instances of the same strategy, with different
configurations (such as trading different instruments), then you will need to define
a unique `order_id_tag` for each of these strategies (as shown above).

```{note}
The platform has built-in safety measures in the event that two strategies share a
duplicated strategy ID, then an exception will be thrown that the strategy ID has already been registered.
```

The reason for this is that the system must be able to identify which strategy
various commands and events belong to. A strategy ID is made up of the
strategy class name, and the strategies `order_id_tag` separated by a hyphen. For
example the above config would result in a strategy ID of `MyStrategy-001`.

```{tip}
See the `StrategyId` [documentation](../api_reference/model/identifiers.md) for further details.
```

### Managed GTD expiry
It's possible for the strategy to manage expiry for orders with a time in force of GTD (_Good 'till Date_).
This may be desirable if the exchange/broker does not support this time in force option, or for any
reason you prefer the strategy to manage this.

To use this option, pass `manage_gtd_expiry=True` to your `StrategyConfig`. When an order is submitted with
a time in force of GTD, the strategy will automatically start an internal time alert.
Once the internal GTD time alert is reached, the order will be canceled (if not already closed).

Some venues (such as Binance Futures) support the GTD time in force, so to avoid conflicts when using
`managed_gtd_expiry` you should set `use_gtd=False` for your execution client config.
