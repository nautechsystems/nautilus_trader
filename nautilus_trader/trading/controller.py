from nautilus_trader.common.actor import Actor
from nautilus_trader.config.common import ActorConfig
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.trader import Trader


class Controller(Actor):
    def __init__(
        self,
        config: ActorConfig,
        trader: Trader,
    ):
        super().__init__(config=config)
        self.trader = trader

    def create_strategy(self, strategy: Strategy):
        self.trader.add_strategy(strategy)
        strategy.on_start()
