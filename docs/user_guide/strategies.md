# Strategies

The heart of the NautilusTrader user experience is in writing and working with
trading strategies, by inheriting `TradingStrategy` and implementing its methods.

Please refer to the [API Reference](../api_reference/trading.md#strategy) for a complete description
of all the possible functionality.

There are two main pieces to a Nautilus trading strategy:
- The strategy implementation itself, defined by inheriting `TradingStrategy`
- The _optional_ strategy configuration, defined by inheriting `TradingStrategyConfig`

```{note}
Once a strategy is defined, the same source can be used for backtesting and live trading.
```

## Configuration
The main purpose of a separate configuration class is to provide total flexibility
over where and how a trading strategy can be instantiated. This includes being able
to serialize strategies and their configurations over the wire, making distributed backtesting
and firing up remote live trading possible.

This configuration flexibility is actually opt in, in that you can actually choose not to have
any strategy configuration beyond the parameters you choose to pass into your
strategies constructor. If you would like to run distributed backtests or launch
live trading servers remotely, then you will need to define a configuration.

Here is an example configuration:

```python
from decimal import Decimal
from nautilus_trader.trading.config import TradingStrategyConfig


class MyStrategy(TradingStrategyConfig):
    instrument_id: str
    bar_type: str
    fast_ema_period: int = 10
    slow_ema_period: int = 20
    trade_size: Decimal
    order_id_tag: str

config = MyStrategy(
    instrument_id="ETH-PERP.FTX",
    bar_type="ETH-PERP.FTX-1000-TICK[LAST]-INTERNAL",
    trade_size=Decimal(1),
    order_id_tag="001",
)
```

### Multiple strategies
If you intend running multiple instances of the same strategy, with different
configurations (such as on different instruments), then you will need to define
a unique `order_id_tag` for each of these strategies (as shown above).

```{note}
The platform has built in safety measures in the event that two strategies share a
duplicated strategy ID, then an exception will be thrown that the strategy ID has already been registered.
```

The reason for this is that the system must be able to identify which strategy
various commands and events belong to. A strategy ID is made up of the
strategy class name, and the strategies `order_id_tag` separated by a hyphen. For
example the above config would result in a strategy ID of `MyStrategy-001`.

```{tip}
See the `StrategyId` [documentation](../api_reference/model/identifiers.md) for further details.
```
