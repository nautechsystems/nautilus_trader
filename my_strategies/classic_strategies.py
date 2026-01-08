"""
Classic Validated Strategies for NautilusTrader
검증된 클래식 전략들
"""

from decimal import Decimal
from typing import Optional

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveFloat, PositiveInt, StrategyConfig
from nautilus_trader.indicators import ExponentialMovingAverage, SimpleMovingAverage
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.trading.strategy import Strategy


# =============================================================================
# RSI Indicator (Custom Implementation)
# =============================================================================
class RSI:
    """Relative Strength Index indicator."""

    def __init__(self, period: int = 14):
        self.period = period
        self._gains: list[float] = []
        self._losses: list[float] = []
        self._prev_close: Optional[float] = None
        self.value: float = 50.0
        self.initialized: bool = False

    def update(self, close: float) -> None:
        if self._prev_close is not None:
            change = close - self._prev_close
            gain = max(change, 0)
            loss = abs(min(change, 0))

            self._gains.append(gain)
            self._losses.append(loss)

            if len(self._gains) > self.period:
                self._gains = self._gains[-self.period:]
                self._losses = self._losses[-self.period:]

            if len(self._gains) >= self.period:
                avg_gain = sum(self._gains) / self.period
                avg_loss = sum(self._losses) / self.period

                if avg_loss == 0:
                    self.value = 100.0
                else:
                    rs = avg_gain / avg_loss
                    self.value = 100 - (100 / (1 + rs))

                self.initialized = True

        self._prev_close = close

    def reset(self) -> None:
        self._gains.clear()
        self._losses.clear()
        self._prev_close = None
        self.value = 50.0
        self.initialized = False


# =============================================================================
# Supertrend Indicator
# =============================================================================
class Supertrend:
    """Supertrend indicator."""

    def __init__(self, period: int = 10, multiplier: float = 3.0):
        self.period = period
        self.multiplier = multiplier
        self._highs: list[float] = []
        self._lows: list[float] = []
        self._closes: list[float] = []
        self._atr: float = 0.0
        self._prev_upper: float = 0.0
        self._prev_lower: float = 0.0
        self._prev_supertrend: float = 0.0
        self.value: float = 0.0
        self.direction: int = 1  # 1 = bullish, -1 = bearish
        self.initialized: bool = False

    def update(self, high: float, low: float, close: float) -> None:
        self._highs.append(high)
        self._lows.append(low)
        self._closes.append(close)

        if len(self._highs) > self.period + 1:
            self._highs = self._highs[-(self.period + 1):]
            self._lows = self._lows[-(self.period + 1):]
            self._closes = self._closes[-(self.period + 1):]

        if len(self._highs) >= self.period:
            # Calculate ATR
            tr_list = []
            for i in range(-self.period, 0):
                tr = max(
                    self._highs[i] - self._lows[i],
                    abs(self._highs[i] - self._closes[i - 1]) if i > -self.period else 0,
                    abs(self._lows[i] - self._closes[i - 1]) if i > -self.period else 0,
                )
                tr_list.append(tr)
            self._atr = sum(tr_list) / len(tr_list)

            # Calculate bands
            hl2 = (high + low) / 2
            upper_band = hl2 + self.multiplier * self._atr
            lower_band = hl2 - self.multiplier * self._atr

            # Adjust bands based on previous values
            if self._prev_lower > 0:
                lower_band = max(lower_band, self._prev_lower) if close > self._prev_lower else lower_band
            if self._prev_upper > 0:
                upper_band = min(upper_band, self._prev_upper) if close < self._prev_upper else upper_band

            # Determine supertrend
            if self._prev_supertrend == self._prev_upper:
                self.value = lower_band if close > upper_band else upper_band
            else:
                self.value = upper_band if close < lower_band else lower_band

            # Direction
            self.direction = 1 if close > self.value else -1

            self._prev_upper = upper_band
            self._prev_lower = lower_band
            self._prev_supertrend = self.value
            self.initialized = True

    def reset(self) -> None:
        self._highs.clear()
        self._lows.clear()
        self._closes.clear()
        self._atr = 0.0
        self._prev_upper = 0.0
        self._prev_lower = 0.0
        self._prev_supertrend = 0.0
        self.value = 0.0
        self.direction = 1
        self.initialized = False


# =============================================================================
# Strategy 1: SMA Crossover (Golden/Dead Cross)
# =============================================================================
class SMACrossoverConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    fast_period: PositiveInt = 20
    slow_period: PositiveInt = 50
    allocation_pct: PositiveFloat = 0.95
    stop_loss_pct: PositiveFloat = 0.05
    take_profit_pct: PositiveFloat = 0.15
    enable_short: bool = True
    close_positions_on_stop: bool = True


class SMACrossoverStrategy(Strategy):
    """
    SMA Crossover Strategy
    - 골든 크로스 (단기 > 장기): 롱 진입
    - 데드 크로스 (단기 < 장기): 숏 진입 또는 청산
    """

    def __init__(self, config: SMACrossoverConfig) -> None:
        super().__init__(config)
        self.instrument: Optional[Instrument] = None
        self.sma_fast = SimpleMovingAverage(config.fast_period)
        self.sma_slow = SimpleMovingAverage(config.slow_period)
        self._prev_fast: float = 0.0
        self._prev_slow: float = 0.0
        self.entry_price: Optional[float] = None
        self.position_side: int = 0  # 1=long, -1=short, 0=flat

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Instrument not found: {self.config.instrument_id}")
            self.stop()
            return
        self.subscribe_bars(self.config.bar_type)
        self.log.info("SMA Crossover Strategy started", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        if bar.bar_type != self.config.bar_type:
            return

        close = float(bar.close)
        self.sma_fast.update_raw(close)
        self.sma_slow.update_raw(close)

        if not self.sma_slow.initialized:
            return

        fast = self.sma_fast.value
        slow = self.sma_slow.value

        # Check for crossover
        if self._prev_fast > 0 and self._prev_slow > 0:
            # Golden Cross
            if self._prev_fast <= self._prev_slow and fast > slow:
                if self.position_side == -1:
                    self._close_position("golden_cross_exit")
                if self.position_side <= 0:
                    self._enter_long(bar)

            # Dead Cross
            elif self._prev_fast >= self._prev_slow and fast < slow:
                if self.position_side == 1:
                    self._close_position("dead_cross_exit")
                if self.position_side >= 0 and self.config.enable_short:
                    self._enter_short(bar)

        # Risk management
        if self.entry_price and self.position_side != 0:
            pnl_pct = (close - self.entry_price) / self.entry_price * self.position_side

            if pnl_pct <= -self.config.stop_loss_pct:
                self._close_position("stop_loss")
            elif pnl_pct >= self.config.take_profit_pct:
                self._close_position("take_profit")

        self._prev_fast = fast
        self._prev_slow = slow

    def _enter_long(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct * 0.95) / close

        self.log.info(f"LONG ENTRY @ {close:.2f}", LogColor.GREEN)
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(Decimal(str(qty))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self.entry_price = close
        self.position_side = 1

    def _enter_short(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct * 0.95) / close

        self.log.info(f"SHORT ENTRY @ {close:.2f}", LogColor.YELLOW)
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(Decimal(str(qty))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self.entry_price = close
        self.position_side = -1

    def _close_position(self, reason: str) -> None:
        if self.position_side == 0:
            return
        self.log.info(f"CLOSE ({reason})", LogColor.MAGENTA)
        self.close_all_positions(self.config.instrument_id)
        self.entry_price = None
        self.position_side = 0

    def on_stop(self) -> None:
        self.cancel_all_orders(self.config.instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.instrument_id)
        self.unsubscribe_bars(self.config.bar_type)

    def on_reset(self) -> None:
        self.sma_fast.reset()
        self.sma_slow.reset()
        self._prev_fast = 0.0
        self._prev_slow = 0.0
        self.entry_price = None
        self.position_side = 0


# =============================================================================
# Strategy 2: RSI Mean Reversion
# =============================================================================
class RSIMeanReversionConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    rsi_period: PositiveInt = 14
    oversold: PositiveFloat = 30.0
    overbought: PositiveFloat = 70.0
    allocation_pct: PositiveFloat = 0.95
    stop_loss_pct: PositiveFloat = 0.05
    take_profit_pct: PositiveFloat = 0.10
    enable_short: bool = True
    close_positions_on_stop: bool = True


class RSIMeanReversionStrategy(Strategy):
    """
    RSI Mean Reversion Strategy
    - RSI < 30 (과매도): 롱 진입
    - RSI > 70 (과매수): 숏 진입 또는 청산
    """

    def __init__(self, config: RSIMeanReversionConfig) -> None:
        super().__init__(config)
        self.instrument: Optional[Instrument] = None
        self.rsi = RSI(config.rsi_period)
        self.entry_price: Optional[float] = None
        self.position_side: int = 0

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Instrument not found: {self.config.instrument_id}")
            self.stop()
            return
        self.subscribe_bars(self.config.bar_type)
        self.log.info("RSI Mean Reversion Strategy started", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        if bar.bar_type != self.config.bar_type:
            return

        close = float(bar.close)
        self.rsi.update(close)

        if not self.rsi.initialized:
            return

        rsi = self.rsi.value

        # Entry signals
        if self.position_side == 0:
            if rsi < self.config.oversold:
                self._enter_long(bar)
            elif rsi > self.config.overbought and self.config.enable_short:
                self._enter_short(bar)

        # Exit signals
        elif self.position_side == 1 and rsi > 50:
            self._close_position("rsi_exit")
        elif self.position_side == -1 and rsi < 50:
            self._close_position("rsi_exit")

        # Risk management
        if self.entry_price and self.position_side != 0:
            pnl_pct = (close - self.entry_price) / self.entry_price * self.position_side

            if pnl_pct <= -self.config.stop_loss_pct:
                self._close_position("stop_loss")
            elif pnl_pct >= self.config.take_profit_pct:
                self._close_position("take_profit")

    def _enter_long(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct * 0.95) / close

        self.log.info(f"LONG (RSI={self.rsi.value:.1f}) @ {close:.2f}", LogColor.GREEN)
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(Decimal(str(qty))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self.entry_price = close
        self.position_side = 1

    def _enter_short(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct * 0.95) / close

        self.log.info(f"SHORT (RSI={self.rsi.value:.1f}) @ {close:.2f}", LogColor.YELLOW)
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(Decimal(str(qty))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self.entry_price = close
        self.position_side = -1

    def _close_position(self, reason: str) -> None:
        if self.position_side == 0:
            return
        self.log.info(f"CLOSE ({reason})", LogColor.MAGENTA)
        self.close_all_positions(self.config.instrument_id)
        self.entry_price = None
        self.position_side = 0

    def on_stop(self) -> None:
        self.cancel_all_orders(self.config.instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.instrument_id)
        self.unsubscribe_bars(self.config.bar_type)

    def on_reset(self) -> None:
        self.rsi.reset()
        self.entry_price = None
        self.position_side = 0


# =============================================================================
# Strategy 3: Supertrend
# =============================================================================
class SupertrendConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    period: PositiveInt = 10
    multiplier: PositiveFloat = 3.0
    allocation_pct: PositiveFloat = 0.95
    stop_loss_pct: PositiveFloat = 0.08
    enable_short: bool = True
    close_positions_on_stop: bool = True


class SupertrendStrategy(Strategy):
    """
    Supertrend Strategy
    - Supertrend 위 (상승추세): 롱 포지션
    - Supertrend 아래 (하락추세): 숏 포지션
    """

    def __init__(self, config: SupertrendConfig) -> None:
        super().__init__(config)
        self.instrument: Optional[Instrument] = None
        self.supertrend = Supertrend(config.period, config.multiplier)
        self._prev_direction: int = 0
        self.entry_price: Optional[float] = None
        self.position_side: int = 0

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Instrument not found: {self.config.instrument_id}")
            self.stop()
            return
        self.subscribe_bars(self.config.bar_type)
        self.log.info("Supertrend Strategy started", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        if bar.bar_type != self.config.bar_type:
            return

        high = float(bar.high)
        low = float(bar.low)
        close = float(bar.close)

        self.supertrend.update(high, low, close)

        if not self.supertrend.initialized:
            return

        direction = self.supertrend.direction

        # Direction change
        if self._prev_direction != 0 and direction != self._prev_direction:
            # Close existing position
            if self.position_side != 0:
                self._close_position("trend_change")

            # Enter new position
            if direction == 1:
                self._enter_long(bar)
            elif direction == -1 and self.config.enable_short:
                self._enter_short(bar)

        # Risk management (stop loss only, let trend ride)
        if self.entry_price and self.position_side != 0:
            pnl_pct = (close - self.entry_price) / self.entry_price * self.position_side

            if pnl_pct <= -self.config.stop_loss_pct:
                self._close_position("stop_loss")

        self._prev_direction = direction

    def _enter_long(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct * 0.95) / close

        self.log.info(f"LONG (Supertrend bullish) @ {close:.2f}", LogColor.GREEN)
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(Decimal(str(qty))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self.entry_price = close
        self.position_side = 1

    def _enter_short(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct * 0.95) / close

        self.log.info(f"SHORT (Supertrend bearish) @ {close:.2f}", LogColor.YELLOW)
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(Decimal(str(qty))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self.entry_price = close
        self.position_side = -1

    def _close_position(self, reason: str) -> None:
        if self.position_side == 0:
            return
        self.log.info(f"CLOSE ({reason})", LogColor.MAGENTA)
        self.close_all_positions(self.config.instrument_id)
        self.entry_price = None
        self.position_side = 0

    def on_stop(self) -> None:
        self.cancel_all_orders(self.config.instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.instrument_id)
        self.unsubscribe_bars(self.config.bar_type)

    def on_reset(self) -> None:
        self.supertrend.reset()
        self._prev_direction = 0
        self.entry_price = None
        self.position_side = 0
