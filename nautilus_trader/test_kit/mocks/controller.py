from nautilus_trader.config import ActorConfig
from nautilus_trader.examples.strategies.signal_strategy import SignalStrategy
from nautilus_trader.examples.strategies.signal_strategy import SignalStrategyConfig
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.controller import Controller


class ControllerConfig(ActorConfig, frozen=True):
    pass


class MyController(Controller):
    def start(self):
        """
        Dynamically add a new strategy after startup.
        """
        instruments = self.cache.instruments()
        strategy_config = ImportableStrategyConfig(
            strategy_path=SignalStrategy.fully_qualified_name(),
            config_path=SignalStrategyConfig.fully_qualified_name(),
            config={
                "instrument_id": instruments[0].id,
            },
        )
        self.create_strategy_from_config(strategy_config)
