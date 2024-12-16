# Actors

:::info
We are currently working on this guide.
:::

The `Actor` class provides the foundation for components that can interact with the trading system,
including `Strategy` which inherits from it and additionally provides order management
methods on top. This means everything discussed in the [Strategies](../strategies.md) guide
also applies to actors.

Just like strategies, actors support configuration through very similar pattern.

```python
from nautilus_trader.config import ActorConfig
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Bar, BarType
from nautilus_trader.common.actor import Actor


class MyActorConfig(ActorConfig):
    instrument_id: InstrumentId   # example value: "ETHUSDT-PERP.BINANCE"
    bar_type: BarType             # example value: "ETHUSDT-PERP.BINANCE-15-MINUTE[LAST]-INTERNAL"
    lookback_period: int = 10


class MyActor(Actor):
    def __init__(self, config: MyActorConfig) -> None:
        super().__init__(config)

        # Custom state variables
        self.count_of_processed_bars: int = 0

    def on_start(self) -> None:
        # Subscribe to all incoming bars
        self.subscribe_bars(self.config.bar_type)   # You can access configuration directly via `self.config`

    def on_bar(self, bar: Bar):
        self.count_of_processed_bars += 1
```
