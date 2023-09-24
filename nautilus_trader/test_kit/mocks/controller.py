from nautilus_trader.config import ActorConfig
from nautilus_trader.trading.controller import Controller


class ControllerConfig(ActorConfig, frozen=True):
    pass


class MyController(Controller):
    def start(self):
        """
        Dynamically add a new strategy after startup.
        """
        from nautilus_trader.examples.strategies.signal_strategy import SignalStrategy
        from nautilus_trader.examples.strategies.signal_strategy import SignalStrategyConfig

        instruments = self.cache.instruments()
        strategy_config = SignalStrategyConfig(instrument_id=instruments[0].id.value)
        strategy = SignalStrategy(strategy_config)
        self.create_strategy(strategy)
