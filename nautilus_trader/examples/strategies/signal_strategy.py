from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY ***


class SignalStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``SignalStrategy`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.

    """

    instrument_id: InstrumentId


class SignalStrategy(Strategy):
    """
    A strategy that simply emits a signal counter (FOR TESTING PURPOSES ONLY).

    Parameters
    ----------
    config : OrderbookImbalanceConfig
        The configuration for the instance.

    """

    def __init__(self, config: SignalStrategyConfig) -> None:
        super().__init__(config)
        self.instrument: Instrument | None = None
        self.counter = 0

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        self.subscribe_trade_ticks(instrument_id=self.config.instrument_id)
        self.subscribe_quote_ticks(instrument_id=self.config.instrument_id)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote tick.
        """
        self.counter += 1
        self.publish_signal(name="counter", value=self.counter, ts_event=tick.ts_event)

    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a trade tick.
        """
        self.counter += 1
        self.publish_signal(name="counter", value=self.counter, ts_event=tick.ts_event)
