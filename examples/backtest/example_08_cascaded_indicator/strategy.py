from collections import deque

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.ma_factory import MovingAverageType
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.strategy import Strategy


class DemoStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for the demo strategy.
    """

    instrument: Instrument
    primary_bar_type: BarType
    primary_ema_period: int = 10  # Period for primary EMA indicator
    secondary_ema_period: int = 20  # Period for secondary cascaded EMA indicator


class DemoStrategy(Strategy):
    """
    A simple strategy demonstrating the use of cascaded indicators.
    """

    def __init__(self, config: DemoStrategyConfig):
        super().__init__(config)

        # Count processed bars
        self.bars_processed = 0

        # Store bar type from config
        self.bar_type = config.primary_bar_type

        # Primary indicator: EMA calculated on 1-min bars
        self.primary_ema = MovingAverageFactory.create(
            config.primary_ema_period,  # Period for primary EMA indicator
            MovingAverageType.EXPONENTIAL,  # Type of moving average
        )
        self.primary_ema_history: deque[float] = deque()  # Store historical values here

        # Cascaded indicator: EMA calculated on primary EMA values
        self.secondary_ema = MovingAverageFactory.create(
            config.secondary_ema_period,  # Period for secondary cascaded EMA indicator
            MovingAverageType.EXPONENTIAL,  # Type of moving average
        )
        self.secondary_ema_history: deque[float] = deque()  # Store historical values here

    def on_start(self):
        # Subscribe to bars
        self.subscribe_bars(self.bar_type)

        # Register primary indicator to receive bar data
        self.register_indicator_for_bars(self.bar_type, self.primary_ema)

        self.log.info("Strategy started.")

    def on_bar(self, bar: Bar):
        # Count processed bars
        self.bars_processed += 1
        self.log.info(
            f"Bar #{self.bars_processed} | "
            f"Bar: {bar} | "
            f"Time={unix_nanos_to_dt(bar.ts_event)}",
            color=LogColor.YELLOW,
        )

        # Store latest primary EMA value
        # Since primary EMA is registered, it's automatically updated with new bars
        primary_ema_value = self.primary_ema.value
        self.primary_ema_history.appendleft(primary_ema_value)

        # Update cascaded EMA with the latest primary EMA value
        # We need to wait until primary EMA is initialized
        if self.primary_ema.initialized:
            # Manually feed primary EMA value into secondary EMA
            self.secondary_ema.update_raw(self.primary_ema.value)
            # Store latest secondary EMA value
            self.secondary_ema_history.appendleft(self.secondary_ema.value)

        # Wait until both indicators are initialized
        # - Primary EMA needs first `primary_ema_period` bars to initialize
        # - Secondary EMA needs `secondary_ema_period` values from primary EMA to initialize
        # So in total we need at least `primary_ema_period + secondary_ema_period` bars before both indicators are ready
        if not self.primary_ema.initialized or not self.secondary_ema.initialized:
            self.log.info("Waiting for indicators to initialize...", color=LogColor.RED)
            return

        # Access and log indicator values
        primary_ema_latest = self.primary_ema.value
        secondary_ema_latest = self.secondary_ema.value

        # Log latest indicator values
        self.log.info(
            f"Latest values. | "
            f"Primary EMA({self.config.primary_ema_period}) = {primary_ema_latest:.7f}, "
            f"Secondary EMA({self.config.secondary_ema_period}) = {secondary_ema_latest:.7f}",
            color=LogColor.BLUE,
        )

        # Check history and log previous values if available
        if len(self.primary_ema_history) > 1 and len(self.secondary_ema_history) > 1:
            primary_ema_prev = self.primary_ema_history[1]
            secondary_ema_prev = self.secondary_ema_history[1]
            self.log.info(
                f"Previous values | "
                f"Primary EMA({self.config.primary_ema_period}) = {primary_ema_prev:.7f}, "
                f"Secondary EMA({self.config.secondary_ema_period}) = {secondary_ema_prev:.7f}",
            )

    def on_stop(self):
        self.log.info(f"Strategy stopped. Processed {self.bars_processed} bars.")
