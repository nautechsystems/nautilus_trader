"""
ichiV1 Strategy for NautilusTrader
- Freqtrade ichiV1 전략을 Nautilus로 포팅
- Ichimoku Cloud + EMA Fan 기반 추세 추종 전략
- Bybit 선물 거래용
"""

from decimal import Decimal
from typing import Optional

import numpy as np

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveFloat, PositiveInt, StrategyConfig
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.trading.strategy import Strategy


class IchimokuIndicator:
    """
    Custom Ichimoku Cloud indicator.

    - Tenkan-sen (Conversion Line): (highest high + lowest low) / 2 for conversion_period
    - Kijun-sen (Base Line): (highest high + lowest low) / 2 for base_period
    - Senkou Span A: (tenkan + kijun) / 2, shifted forward by displacement
    - Senkou Span B: (highest high + lowest low) / 2 for span_b_period, shifted forward
    - Chikou Span: Close price shifted backward by displacement
    """

    def __init__(
        self,
        conversion_period: int = 20,
        base_period: int = 60,
        span_b_period: int = 120,
        displacement: int = 30,
    ):
        self.conversion_period = conversion_period
        self.base_period = base_period
        self.span_b_period = span_b_period
        self.displacement = displacement

        # Data storage
        self._highs: list[float] = []
        self._lows: list[float] = []
        self._closes: list[float] = []

        # Senkou spans need to be stored for displacement
        self._senkou_a_buffer: list[float] = []
        self._senkou_b_buffer: list[float] = []

        # Current values
        self.tenkan_sen: float = 0.0
        self.kijun_sen: float = 0.0
        self.senkou_a: float = 0.0
        self.senkou_b: float = 0.0
        self.chikou_span: float = 0.0

        self.initialized: bool = False

    def _midpoint(self, data: list[float], period: int) -> float:
        """Calculate (highest + lowest) / 2 for given period."""
        if len(data) < period:
            return 0.0
        window = data[-period:]
        return (max(window) + min(window)) / 2

    def update(self, high: float, low: float, close: float) -> None:
        """Update indicator with new bar data."""
        self._highs.append(high)
        self._lows.append(low)
        self._closes.append(close)

        # Keep only necessary data
        max_period = max(self.span_b_period, self.base_period, self.conversion_period) + self.displacement + 10
        if len(self._highs) > max_period:
            self._highs = self._highs[-max_period:]
            self._lows = self._lows[-max_period:]
            self._closes = self._closes[-max_period:]

        # Calculate current values
        self.tenkan_sen = self._midpoint(self._highs, self.conversion_period)
        self.kijun_sen = self._midpoint(self._highs, self.base_period)

        # Senkou A and B (current calculation, will be displaced)
        current_senkou_a = (self.tenkan_sen + self.kijun_sen) / 2
        current_senkou_b = self._midpoint(self._highs, self.span_b_period)

        # Store for displacement
        self._senkou_a_buffer.append(current_senkou_a)
        self._senkou_b_buffer.append(current_senkou_b)

        if len(self._senkou_a_buffer) > self.displacement + 10:
            self._senkou_a_buffer = self._senkou_a_buffer[-(self.displacement + 10):]
            self._senkou_b_buffer = self._senkou_b_buffer[-(self.displacement + 10):]

        # Get displaced values (from displacement periods ago)
        if len(self._senkou_a_buffer) > self.displacement:
            self.senkou_a = self._senkou_a_buffer[-self.displacement]
            self.senkou_b = self._senkou_b_buffer[-self.displacement]

        # Chikou span (current close, displayed displacement periods back)
        self.chikou_span = close

        # Check if initialized
        if len(self._highs) >= self.span_b_period + self.displacement:
            self.initialized = True

    def reset(self) -> None:
        """Reset indicator state."""
        self._highs.clear()
        self._lows.clear()
        self._closes.clear()
        self._senkou_a_buffer.clear()
        self._senkou_b_buffer.clear()
        self.tenkan_sen = 0.0
        self.kijun_sen = 0.0
        self.senkou_a = 0.0
        self.senkou_b = 0.0
        self.chikou_span = 0.0
        self.initialized = False


class IchiV1Config(StrategyConfig, frozen=True):
    """Configuration for ichiV1 Strategy."""

    instrument_id: InstrumentId
    bar_type: BarType

    # Ichimoku settings (from original)
    ichimoku_conversion: PositiveInt = 20
    ichimoku_base: PositiveInt = 60
    ichimoku_span_b: PositiveInt = 120
    ichimoku_displacement: PositiveInt = 30

    # Entry parameters
    entry_trend_above_senkou_level: PositiveInt = 1
    entry_trend_bullish_level: PositiveInt = 6
    entry_fan_magnitude_shift_value: PositiveInt = 3
    entry_min_fan_magnitude_gain: PositiveFloat = 1.002

    # Risk management
    allocation_pct: PositiveFloat = 0.95
    stoploss: PositiveFloat = 0.275

    # ROI table (minutes: profit_ratio)
    roi_0: PositiveFloat = 0.059
    roi_10: PositiveFloat = 0.037
    roi_41: PositiveFloat = 0.012
    roi_114: PositiveFloat = 0.0

    close_positions_on_stop: bool = True


class IchiV1Strategy(Strategy):
    """
    ichiV1 Strategy - Ichimoku Cloud + EMA Fan trend following.

    Entry conditions:
    - Price above Ichimoku cloud (senkou_a and senkou_b)
    - EMA fan is bullish (close > open for multiple timeframes)
    - Fan magnitude is increasing

    Exit conditions:
    - 5m EMA crosses below 2h EMA
    - ROI targets reached
    - Stop loss hit
    """

    def __init__(self, config: IchiV1Config) -> None:
        super().__init__(config)

        self.instrument: Optional[Instrument] = None

        # Ichimoku indicator
        self.ichimoku = IchimokuIndicator(
            conversion_period=config.ichimoku_conversion,
            base_period=config.ichimoku_base,
            span_b_period=config.ichimoku_span_b,
            displacement=config.ichimoku_displacement,
        )

        # EMA indicators for trend (simulating different timeframes on 5m bars)
        # 5m = 1 bar, 15m = 3 bars, 30m = 6 bars, etc.
        self.ema_5m = ExponentialMovingAverage(1)    # Just close price
        self.ema_15m = ExponentialMovingAverage(3)
        self.ema_30m = ExponentialMovingAverage(6)
        self.ema_1h = ExponentialMovingAverage(12)
        self.ema_2h = ExponentialMovingAverage(24)
        self.ema_4h = ExponentialMovingAverage(48)
        self.ema_6h = ExponentialMovingAverage(72)
        self.ema_8h = ExponentialMovingAverage(96)

        # EMA for open prices (for bullish check)
        self.ema_open_5m = ExponentialMovingAverage(1)
        self.ema_open_15m = ExponentialMovingAverage(3)
        self.ema_open_30m = ExponentialMovingAverage(6)
        self.ema_open_1h = ExponentialMovingAverage(12)
        self.ema_open_2h = ExponentialMovingAverage(24)
        self.ema_open_4h = ExponentialMovingAverage(48)
        self.ema_open_6h = ExponentialMovingAverage(72)
        self.ema_open_8h = ExponentialMovingAverage(96)

        # Fan magnitude tracking
        self._fan_magnitude_history: list[float] = []

        # Position tracking
        self.entry_price: Optional[float] = None
        self.entry_time: Optional[int] = None  # timestamp in ns
        self.in_position: bool = False

    def on_start(self) -> None:
        """Actions on strategy start."""
        self.instrument = self.cache.instrument(self.config.instrument_id)

        if self.instrument is None:
            self.log.error(f"Could not find instrument: {self.config.instrument_id}")
            self.stop()
            return

        # Subscribe to bar data
        self.subscribe_bars(self.config.bar_type)

        self.log.info(
            f"ichiV1 Strategy started for {self.config.instrument_id}",
            LogColor.GREEN,
        )

    def on_bar(self, bar: Bar) -> None:
        """Handle incoming bar data."""
        if bar.bar_type != self.config.bar_type:
            return

        # Update indicators
        high = float(bar.high)
        low = float(bar.low)
        close = float(bar.close)
        open_price = float(bar.open)

        self.ichimoku.update(high, low, close)

        # Update EMAs for close
        self.ema_5m.update_raw(close)
        self.ema_15m.update_raw(close)
        self.ema_30m.update_raw(close)
        self.ema_1h.update_raw(close)
        self.ema_2h.update_raw(close)
        self.ema_4h.update_raw(close)
        self.ema_6h.update_raw(close)
        self.ema_8h.update_raw(close)

        # Update EMAs for open (Heikin Ashi approximation)
        self.ema_open_5m.update_raw(open_price)
        self.ema_open_15m.update_raw(open_price)
        self.ema_open_30m.update_raw(open_price)
        self.ema_open_1h.update_raw(open_price)
        self.ema_open_2h.update_raw(open_price)
        self.ema_open_4h.update_raw(open_price)
        self.ema_open_6h.update_raw(open_price)
        self.ema_open_8h.update_raw(open_price)

        # Calculate fan magnitude
        if self.ema_8h.initialized:
            fan_magnitude = self.ema_1h.value / self.ema_8h.value
            self._fan_magnitude_history.append(fan_magnitude)
            if len(self._fan_magnitude_history) > 10:
                self._fan_magnitude_history = self._fan_magnitude_history[-10:]

        # Check if ready
        if not self._is_ready():
            return

        # Check exit conditions first
        if self.in_position:
            self._check_exit(bar)
        else:
            self._check_entry(bar)

    def _is_ready(self) -> bool:
        """Check if all indicators are initialized."""
        return (
            self.ichimoku.initialized and
            self.ema_8h.initialized and
            len(self._fan_magnitude_history) >= self.config.entry_fan_magnitude_shift_value + 1
        )

    def _check_entry(self, bar: Bar) -> None:
        """Check entry conditions."""
        close = float(bar.close)

        # 1. Check if price is above cloud
        if not self._is_above_cloud():
            return

        # 2. Check if trends are bullish
        if not self._is_trend_bullish():
            return

        # 3. Check fan magnitude conditions
        if not self._is_fan_magnitude_valid():
            return

        # All conditions met - enter position
        self._enter_long(bar)

    def _is_above_cloud(self) -> bool:
        """Check if EMAs are above Ichimoku cloud."""
        senkou_a = self.ichimoku.senkou_a
        senkou_b = self.ichimoku.senkou_b
        level = self.config.entry_trend_above_senkou_level

        emas = [
            self.ema_5m.value,
            self.ema_15m.value,
            self.ema_30m.value,
            self.ema_1h.value,
            self.ema_2h.value,
            self.ema_4h.value,
            self.ema_6h.value,
            self.ema_8h.value,
        ]

        # Check first 'level' EMAs are above both senkou lines
        for i in range(min(level, len(emas))):
            if emas[i] <= senkou_a or emas[i] <= senkou_b:
                return False

        return True

    def _is_trend_bullish(self) -> bool:
        """Check if close EMAs are above open EMAs (bullish)."""
        level = self.config.entry_trend_bullish_level

        close_emas = [
            self.ema_5m.value,
            self.ema_15m.value,
            self.ema_30m.value,
            self.ema_1h.value,
            self.ema_2h.value,
            self.ema_4h.value,
            self.ema_6h.value,
            self.ema_8h.value,
        ]

        open_emas = [
            self.ema_open_5m.value,
            self.ema_open_15m.value,
            self.ema_open_30m.value,
            self.ema_open_1h.value,
            self.ema_open_2h.value,
            self.ema_open_4h.value,
            self.ema_open_6h.value,
            self.ema_open_8h.value,
        ]

        for i in range(min(level, len(close_emas))):
            if close_emas[i] <= open_emas[i]:
                return False

        return True

    def _is_fan_magnitude_valid(self) -> bool:
        """Check fan magnitude conditions."""
        if len(self._fan_magnitude_history) < self.config.entry_fan_magnitude_shift_value + 1:
            return False

        current = self._fan_magnitude_history[-1]

        # Fan magnitude > 1 (1h EMA > 8h EMA)
        if current <= 1.0:
            return False

        # Check magnitude gain
        prev = self._fan_magnitude_history[-2]
        gain = current / prev if prev > 0 else 0
        if gain < self.config.entry_min_fan_magnitude_gain:
            return False

        # Check that magnitude is increasing for last N periods
        for i in range(self.config.entry_fan_magnitude_shift_value):
            idx = -(i + 2)  # -2, -3, -4, ...
            if self._fan_magnitude_history[idx] >= current:
                return False

        return True

    def _check_exit(self, bar: Bar) -> None:
        """Check exit conditions."""
        close = float(bar.close)
        current_time = bar.ts_event

        # 1. Stop loss check
        if self.entry_price:
            pnl_pct = (close - self.entry_price) / self.entry_price
            if pnl_pct <= -self.config.stoploss:
                self.log.warning(f"STOP LOSS triggered: {pnl_pct * 100:.2f}%")
                self._exit_position("stop_loss")
                return

            # 2. ROI check
            if self.entry_time:
                minutes_in_trade = (current_time - self.entry_time) / (60 * 1_000_000_000)
                roi_target = self._get_roi_target(minutes_in_trade)

                if pnl_pct >= roi_target:
                    self.log.info(f"ROI target reached: {pnl_pct * 100:.2f}% >= {roi_target * 100:.2f}%")
                    self._exit_position("roi")
                    return

        # 3. Signal-based exit: 5m EMA crosses below 2h EMA
        if self.ema_5m.value < self.ema_2h.value:
            self.log.info("Exit signal: 5m EMA crossed below 2h EMA")
            self._exit_position("exit_signal")

    def _get_roi_target(self, minutes: float) -> float:
        """Get ROI target based on time in trade."""
        if minutes >= 114:
            return self.config.roi_114
        elif minutes >= 41:
            return self.config.roi_41
        elif minutes >= 10:
            return self.config.roi_10
        else:
            return self.config.roi_0

    def _enter_long(self, bar: Bar) -> None:
        """Enter long position."""
        if self.in_position:
            return

        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            self.log.error("No account found")
            return

        close = float(bar.close)
        balance = float(account.balance_total())
        target_value = balance * self.config.allocation_pct * 0.95

        # Calculate quantity
        quantity = target_value / close
        if quantity < 0.001:
            return

        self.log.info(
            f"ENTRY: {self.config.instrument_id} @ {close:.4f} | qty: {quantity:.4f}",
            LogColor.GREEN,
        )

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(Decimal(str(quantity))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)

        self.entry_price = close
        self.entry_time = bar.ts_event
        self.in_position = True

    def _exit_position(self, reason: str) -> None:
        """Exit current position."""
        if not self.in_position:
            return

        self.log.info(f"EXIT ({reason}): Closing position", LogColor.YELLOW)
        self.close_all_positions(self.config.instrument_id)

        self.entry_price = None
        self.entry_time = None
        self.in_position = False

    def on_stop(self) -> None:
        """Cleanup on stop."""
        self.cancel_all_orders(self.config.instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.instrument_id)

        self.unsubscribe_bars(self.config.bar_type)

    def on_reset(self) -> None:
        """Reset strategy state."""
        self.ichimoku.reset()
        self.ema_5m.reset()
        self.ema_15m.reset()
        self.ema_30m.reset()
        self.ema_1h.reset()
        self.ema_2h.reset()
        self.ema_4h.reset()
        self.ema_6h.reset()
        self.ema_8h.reset()
        self.ema_open_5m.reset()
        self.ema_open_15m.reset()
        self.ema_open_30m.reset()
        self.ema_open_1h.reset()
        self.ema_open_2h.reset()
        self.ema_open_4h.reset()
        self.ema_open_6h.reset()
        self.ema_open_8h.reset()
        self._fan_magnitude_history.clear()
        self.entry_price = None
        self.entry_time = None
        self.in_position = False
