"""
Popular NautilusTrader Strategies
검증된 인기 전략들
"""

from decimal import Decimal
from typing import Optional

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import PositiveFloat, PositiveInt, StrategyConfig
from nautilus_trader.indicators import ExponentialMovingAverage, BollingerBands
from nautilus_trader.model.data import Bar, BarType
from nautilus_trader.model.enums import OrderSide, TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.trading.strategy import Strategy


# =============================================================================
# MACD Indicator
# =============================================================================
class MACD:
    """Moving Average Convergence Divergence"""

    def __init__(self, fast: int = 12, slow: int = 26, signal: int = 9):
        self.ema_fast = ExponentialMovingAverage(fast)
        self.ema_slow = ExponentialMovingAverage(slow)
        self.ema_signal = ExponentialMovingAverage(signal)
        self.macd_line: float = 0.0
        self.signal_line: float = 0.0
        self.histogram: float = 0.0
        self.initialized: bool = False

    def update(self, close: float) -> None:
        self.ema_fast.update_raw(close)
        self.ema_slow.update_raw(close)

        if self.ema_slow.initialized:
            self.macd_line = self.ema_fast.value - self.ema_slow.value
            self.ema_signal.update_raw(self.macd_line)

            if self.ema_signal.initialized:
                self.signal_line = self.ema_signal.value
                self.histogram = self.macd_line - self.signal_line
                self.initialized = True

    def reset(self) -> None:
        self.ema_fast.reset()
        self.ema_slow.reset()
        self.ema_signal.reset()
        self.macd_line = 0.0
        self.signal_line = 0.0
        self.histogram = 0.0
        self.initialized = False


# =============================================================================
# Strategy 1: EMA Cross with Trailing Stop (Most Popular)
# =============================================================================
class EMACrossConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    fast_period: PositiveInt = 10
    slow_period: PositiveInt = 20
    atr_period: PositiveInt = 14
    risk_per_trade: PositiveFloat = 0.02  # 2% risk per trade
    trailing_stop_atr: PositiveFloat = 2.0  # ATR multiplier for trailing stop
    enable_short: bool = True
    close_positions_on_stop: bool = True


class EMACrossTrailingStrategy(Strategy):
    """
    EMA Crossover with ATR-based Trailing Stop
    - Golden cross → Long
    - Dead cross → Short/Exit
    - Trailing stop based on ATR
    """

    def __init__(self, config: EMACrossConfig) -> None:
        super().__init__(config)
        self.instrument: Optional[Instrument] = None
        self.ema_fast = ExponentialMovingAverage(config.fast_period)
        self.ema_slow = ExponentialMovingAverage(config.slow_period)

        # ATR for trailing stop
        self._highs: list[float] = []
        self._lows: list[float] = []
        self._closes: list[float] = []
        self._atr: float = 0.0

        self._prev_fast: float = 0.0
        self._prev_slow: float = 0.0
        self.entry_price: Optional[float] = None
        self.trailing_stop: Optional[float] = None
        self.position_side: int = 0
        self.highest_since_entry: float = 0.0
        self.lowest_since_entry: float = float('inf')

    def _update_atr(self, high: float, low: float, close: float) -> None:
        self._highs.append(high)
        self._lows.append(low)
        self._closes.append(close)

        period = self.config.atr_period
        if len(self._highs) > period + 1:
            self._highs = self._highs[-(period + 1):]
            self._lows = self._lows[-(period + 1):]
            self._closes = self._closes[-(period + 1):]

        if len(self._highs) >= period:
            tr_list = []
            for i in range(-period, 0):
                prev_close = self._closes[i - 1] if i > -period else self._closes[0]
                tr = max(
                    self._highs[i] - self._lows[i],
                    abs(self._highs[i] - prev_close),
                    abs(self._lows[i] - prev_close),
                )
                tr_list.append(tr)
            self._atr = sum(tr_list) / len(tr_list)

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Instrument not found: {self.config.instrument_id}")
            self.stop()
            return
        self.subscribe_bars(self.config.bar_type)
        self.log.info("EMA Cross Trailing Strategy started", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        if bar.bar_type != self.config.bar_type:
            return

        high = float(bar.high)
        low = float(bar.low)
        close = float(bar.close)

        self.ema_fast.update_raw(close)
        self.ema_slow.update_raw(close)
        self._update_atr(high, low, close)

        if not self.ema_slow.initialized or self._atr == 0:
            return

        fast = self.ema_fast.value
        slow = self.ema_slow.value

        # Update trailing stop
        if self.position_side == 1:  # Long
            self.highest_since_entry = max(self.highest_since_entry, high)
            self.trailing_stop = self.highest_since_entry - self._atr * self.config.trailing_stop_atr
            if close < self.trailing_stop:
                self._close_position("trailing_stop")

        elif self.position_side == -1:  # Short
            self.lowest_since_entry = min(self.lowest_since_entry, low)
            self.trailing_stop = self.lowest_since_entry + self._atr * self.config.trailing_stop_atr
            if close > self.trailing_stop:
                self._close_position("trailing_stop")

        # Check for crossover
        if self._prev_fast > 0 and self._prev_slow > 0:
            # Golden Cross
            if self._prev_fast <= self._prev_slow and fast > slow:
                if self.position_side == -1:
                    self._close_position("golden_cross")
                if self.position_side == 0:
                    self._enter_long(bar)

            # Dead Cross
            elif self._prev_fast >= self._prev_slow and fast < slow:
                if self.position_side == 1:
                    self._close_position("dead_cross")
                if self.position_side == 0 and self.config.enable_short:
                    self._enter_short(bar)

        self._prev_fast = fast
        self._prev_slow = slow

    def _enter_long(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())

        # Position size based on ATR risk
        risk_amount = balance * self.config.risk_per_trade
        stop_distance = self._atr * self.config.trailing_stop_atr
        qty = risk_amount / stop_distance if stop_distance > 0 else 0
        qty = min(qty, (balance * 0.9) / close)  # Max 90% of balance

        if qty < 0.001:
            return

        self.log.info(f"LONG @ {close:.2f}, ATR={self._atr:.2f}", LogColor.GREEN)
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(Decimal(str(qty))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self.entry_price = close
        self.position_side = 1
        self.highest_since_entry = float(bar.high)
        self.trailing_stop = close - self._atr * self.config.trailing_stop_atr

    def _enter_short(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())

        risk_amount = balance * self.config.risk_per_trade
        stop_distance = self._atr * self.config.trailing_stop_atr
        qty = risk_amount / stop_distance if stop_distance > 0 else 0
        qty = min(qty, (balance * 0.9) / close)

        if qty < 0.001:
            return

        self.log.info(f"SHORT @ {close:.2f}, ATR={self._atr:.2f}", LogColor.YELLOW)
        order = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(Decimal(str(qty))),
            time_in_force=TimeInForce.GTC,
        )
        self.submit_order(order)
        self.entry_price = close
        self.position_side = -1
        self.lowest_since_entry = float(bar.low)
        self.trailing_stop = close + self._atr * self.config.trailing_stop_atr

    def _close_position(self, reason: str) -> None:
        if self.position_side == 0:
            return
        self.log.info(f"CLOSE ({reason})", LogColor.MAGENTA)
        self.close_all_positions(self.config.instrument_id)
        self.entry_price = None
        self.trailing_stop = None
        self.position_side = 0
        self.highest_since_entry = 0.0
        self.lowest_since_entry = float('inf')

    def on_stop(self) -> None:
        self.cancel_all_orders(self.config.instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.instrument_id)
        self.unsubscribe_bars(self.config.bar_type)

    def on_reset(self) -> None:
        self.ema_fast.reset()
        self.ema_slow.reset()
        self._highs.clear()
        self._lows.clear()
        self._closes.clear()
        self._atr = 0.0
        self._prev_fast = 0.0
        self._prev_slow = 0.0
        self.entry_price = None
        self.trailing_stop = None
        self.position_side = 0


# =============================================================================
# Strategy 2: MACD Zero Cross
# =============================================================================
class MACDConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    fast_period: PositiveInt = 12
    slow_period: PositiveInt = 26
    signal_period: PositiveInt = 9
    allocation_pct: PositiveFloat = 0.5
    stop_loss_pct: PositiveFloat = 0.05
    take_profit_pct: PositiveFloat = 0.10
    enable_short: bool = True
    close_positions_on_stop: bool = True


class MACDStrategy(Strategy):
    """
    MACD Strategy
    - MACD crosses above signal → Long
    - MACD crosses below signal → Short/Exit
    """

    def __init__(self, config: MACDConfig) -> None:
        super().__init__(config)
        self.instrument: Optional[Instrument] = None
        self.macd = MACD(config.fast_period, config.slow_period, config.signal_period)
        self._prev_histogram: float = 0.0
        self.entry_price: Optional[float] = None
        self.position_side: int = 0

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Instrument not found: {self.config.instrument_id}")
            self.stop()
            return
        self.subscribe_bars(self.config.bar_type)
        self.log.info("MACD Strategy started", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        if bar.bar_type != self.config.bar_type:
            return

        close = float(bar.close)
        self.macd.update(close)

        if not self.macd.initialized:
            return

        histogram = self.macd.histogram

        # Signal crossover
        if self._prev_histogram != 0:
            # Bullish crossover (histogram goes positive)
            if self._prev_histogram <= 0 and histogram > 0:
                if self.position_side == -1:
                    self._close_position("macd_bullish")
                if self.position_side == 0:
                    self._enter_long(bar)

            # Bearish crossover (histogram goes negative)
            elif self._prev_histogram >= 0 and histogram < 0:
                if self.position_side == 1:
                    self._close_position("macd_bearish")
                if self.position_side == 0 and self.config.enable_short:
                    self._enter_short(bar)

        # Risk management
        if self.entry_price and self.position_side != 0:
            pnl_pct = (close - self.entry_price) / self.entry_price * self.position_side
            if pnl_pct <= -self.config.stop_loss_pct:
                self._close_position("stop_loss")
            elif pnl_pct >= self.config.take_profit_pct:
                self._close_position("take_profit")

        self._prev_histogram = histogram

    def _enter_long(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct) / close

        if qty < 0.001:
            return

        self.log.info(f"LONG (MACD) @ {close:.2f}", LogColor.GREEN)
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
        qty = (balance * self.config.allocation_pct) / close

        if qty < 0.001:
            return

        self.log.info(f"SHORT (MACD) @ {close:.2f}", LogColor.YELLOW)
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
        self.macd.reset()
        self._prev_histogram = 0.0
        self.entry_price = None
        self.position_side = 0


# =============================================================================
# Strategy 3: Bollinger Band Mean Reversion
# =============================================================================
class BollingerConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    period: PositiveInt = 20
    std_dev: PositiveFloat = 2.0
    allocation_pct: PositiveFloat = 0.5
    stop_loss_pct: PositiveFloat = 0.03
    enable_short: bool = True
    close_positions_on_stop: bool = True


class BollingerMeanReversionStrategy(Strategy):
    """
    Bollinger Band Mean Reversion
    - Price below lower band → Long (oversold)
    - Price above upper band → Short (overbought)
    - Exit at middle band
    """

    def __init__(self, config: BollingerConfig) -> None:
        super().__init__(config)
        self.instrument: Optional[Instrument] = None
        self.bb = BollingerBands(config.period, config.std_dev)
        self.entry_price: Optional[float] = None
        self.position_side: int = 0

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Instrument not found: {self.config.instrument_id}")
            self.stop()
            return
        self.subscribe_bars(self.config.bar_type)
        self.log.info("Bollinger Mean Reversion Strategy started", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        if bar.bar_type != self.config.bar_type:
            return

        high = float(bar.high)
        low = float(bar.low)
        close = float(bar.close)
        self.bb.update_raw(high, low, close)

        if not self.bb.initialized:
            return

        upper = self.bb.upper
        middle = self.bb.middle
        lower = self.bb.lower

        # Entry signals
        if self.position_side == 0:
            if close < lower:  # Oversold
                self._enter_long(bar)
            elif close > upper and self.config.enable_short:  # Overbought
                self._enter_short(bar)

        # Exit at middle band
        elif self.position_side == 1 and close >= middle:
            self._close_position("mean_reversion")
        elif self.position_side == -1 and close <= middle:
            self._close_position("mean_reversion")

        # Stop loss
        if self.entry_price and self.position_side != 0:
            pnl_pct = (close - self.entry_price) / self.entry_price * self.position_side
            if pnl_pct <= -self.config.stop_loss_pct:
                self._close_position("stop_loss")

    def _enter_long(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct) / close

        if qty < 0.001:
            return

        self.log.info(f"LONG (BB oversold) @ {close:.2f}", LogColor.GREEN)
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
        qty = (balance * self.config.allocation_pct) / close

        if qty < 0.001:
            return

        self.log.info(f"SHORT (BB overbought) @ {close:.2f}", LogColor.YELLOW)
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
        self.bb.reset()
        self.entry_price = None
        self.position_side = 0


# =============================================================================
# Strategy 4: Donchian Channel Breakout (Turtle Trading)
# =============================================================================
class DonchianConfig(StrategyConfig, frozen=True):
    instrument_id: InstrumentId
    bar_type: BarType
    entry_period: PositiveInt = 20  # 20-day breakout for entry
    exit_period: PositiveInt = 10   # 10-day breakout for exit
    allocation_pct: PositiveFloat = 0.5
    enable_short: bool = True
    close_positions_on_stop: bool = True


class DonchianBreakoutStrategy(Strategy):
    """
    Donchian Channel Breakout (Turtle Trading)
    - Break above 20-day high → Long
    - Break below 20-day low → Short
    - Exit on 10-day opposite breakout
    """

    def __init__(self, config: DonchianConfig) -> None:
        super().__init__(config)
        self.instrument: Optional[Instrument] = None
        self._highs: list[float] = []
        self._lows: list[float] = []
        self.entry_price: Optional[float] = None
        self.position_side: int = 0

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Instrument not found: {self.config.instrument_id}")
            self.stop()
            return
        self.subscribe_bars(self.config.bar_type)
        self.log.info("Donchian Breakout Strategy started", LogColor.GREEN)

    def on_bar(self, bar: Bar) -> None:
        if bar.bar_type != self.config.bar_type:
            return

        high = float(bar.high)
        low = float(bar.low)
        close = float(bar.close)

        self._highs.append(high)
        self._lows.append(low)

        # Keep only necessary data
        max_period = max(self.config.entry_period, self.config.exit_period) + 1
        if len(self._highs) > max_period:
            self._highs = self._highs[-max_period:]
            self._lows = self._lows[-max_period:]

        if len(self._highs) < self.config.entry_period:
            return

        # Calculate channels
        entry_high = max(self._highs[-self.config.entry_period:-1]) if len(self._highs) > self.config.entry_period else max(self._highs[:-1])
        entry_low = min(self._lows[-self.config.entry_period:-1]) if len(self._lows) > self.config.entry_period else min(self._lows[:-1])

        if len(self._highs) >= self.config.exit_period:
            exit_high = max(self._highs[-self.config.exit_period:-1])
            exit_low = min(self._lows[-self.config.exit_period:-1])
        else:
            exit_high = entry_high
            exit_low = entry_low

        # Entry signals
        if self.position_side == 0:
            if high > entry_high:  # Breakout up
                self._enter_long(bar)
            elif low < entry_low and self.config.enable_short:  # Breakout down
                self._enter_short(bar)

        # Exit signals
        elif self.position_side == 1 and low < exit_low:
            self._close_position("exit_breakout")
        elif self.position_side == -1 and high > exit_high:
            self._close_position("exit_breakout")

    def _enter_long(self, bar: Bar) -> None:
        account = self.portfolio.account(self.instrument.id.venue)
        if account is None:
            return
        close = float(bar.close)
        balance = float(account.balance_total())
        qty = (balance * self.config.allocation_pct) / close

        if qty < 0.001:
            return

        self.log.info(f"LONG (Donchian breakout) @ {close:.2f}", LogColor.GREEN)
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
        qty = (balance * self.config.allocation_pct) / close

        if qty < 0.001:
            return

        self.log.info(f"SHORT (Donchian breakout) @ {close:.2f}", LogColor.YELLOW)
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
        self._highs.clear()
        self._lows.clear()
        self.entry_price = None
        self.position_side = 0
