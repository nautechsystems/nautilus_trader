from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


class GridConfig(StrategyConfig):
    instrument_id: str
    value: str = "madin"


class GridStrategy(Strategy):
    def __init__(self, config: GridConfig) -> None:
        super().__init__(config)
        self.instrument_id = InstrumentId.from_str(self.config.instrument_id)
        self.value = config.value

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        self.subscribe_trade_ticks(instrument_id=self.instrument_id)

    def on_trade_tick(self, tick: TradeTick) -> None:
        print(tick)
