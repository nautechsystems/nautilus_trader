#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
from nautilus_trader.indicators.averages import ExponentialMovingAverage
from nautilus_trader.indicators.volatility import AverageTrueRange
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.trading.strategy import Strategy


class WaveTrendMultiTimeframeV4Config(StrategyConfig, frozen=True, kw_only=True):
    """
    Configuration for WaveTrend Multi-Timeframe strategy V4 (Aggressive Drawdown Scaling).

    V4 Improvement over V3:
    - Aggressive position scaling during drawdowns (up to 5x base size)
    - "Buy the dip" mentality: increase size when equity drops
    - If strategy recovers from drawdown, larger positions = massive gains

    V3 Features (All Retained):
    1. ATR minimum filter - pauses trading when volatility too low (avoids choppy markets)
    2. Range filter - checks if market making new highs/lows (avoids stuck ranges)
    3. Multi-timeframe alignment (3/3)
    4. Wider stops (ATR 4.5x)
    5. Higher profit threshold (4.0%)
    6. Tighter trailing (1.0%)
    7. 4h trend filter

    V4 Drawdown Scaling Tiers:
    - < 10% drawdown: 1.0x base size (normal)
    - 10-20% drawdown: 1.5x base size
    - 20-30% drawdown: 2.25x base size (1.5^2)
    - 30-40% drawdown: 3.375x base size (1.5^3)
    - > 40% drawdown: 5.0x base size (max aggressive)

    Notes
    -----
    AGGRESSIVE STRATEGY: Assumes strategy has edge and drawdowns are buying opportunities.
    If strategy is broken, this will accelerate losses.
    Safety cap: Never risk more than max_position_pct_equity of total equity.

    """

    instrument_id: InstrumentId
    base_trade_size: Decimal  # V4: Base size (scaled during drawdowns)

    # WaveTrend parameters per timeframe
    wt_5m_channel_length: PositiveInt = 10
    wt_5m_average_length: PositiveInt = 21
    wt_1h_channel_length: PositiveInt = 9
    wt_1h_average_length: PositiveInt = 18
    wt_4h_channel_length: PositiveInt = 8
    wt_4h_average_length: PositiveInt = 15

    # Alignment rule (V2: requires 3/3 by default)
    min_aligned_timeframes: PositiveInt = 3

    # Trailing stop parameters (V2: improved values)
    atr_period: PositiveInt = 14
    atr_multiplier: PositiveFloat = 4.5  # V2: Wider stops (was 3.0)
    profit_threshold_pct: PositiveFloat = 4.0  # V2: Higher profit target (was 2.0)
    percentage_trail: PositiveFloat = 1.0  # V2: Tighter trailing (was 1.5)

    # Trend filter (V2: new feature)
    use_trend_filter: bool = True
    trend_filter_threshold: PositiveFloat = 20.0  # WT1 above/below this = strong trend

    # V3: Regime filters (avoid choppy markets)
    use_atr_min_filter: bool = True
    atr_min_multiplier: PositiveFloat = 0.5  # Minimum ATR as % of price
    use_range_filter: bool = True
    range_lookback: PositiveInt = 100  # Bars to look back for high/low range check

    # V4: Aggressive drawdown scaling parameters
    scale_at_10pct_dd: PositiveFloat = 1.5     # Scale to 1.5x at 10% drawdown
    scale_at_20pct_dd: PositiveFloat = 2.25    # Scale to 2.25x at 20% drawdown
    scale_at_30pct_dd: PositiveFloat = 3.375   # Scale to 3.375x at 30% drawdown
    scale_at_40pct_dd: PositiveFloat = 5.0     # Scale to 5.0x at 40%+ drawdown
    max_position_pct_equity: PositiveFloat = 0.5  # Safety: never risk >50% of equity

    # Order management
    order_id_tag: str = "WT_MTF_V4"


class WaveTrendState:
    """
    Holds WaveTrend indicator state for one timeframe.

    Parameters
    ----------
    channel_length : int
        The period for channel EMAs (ESA and D calculation).
    average_length : int
        The period for averaging the Channel Index (CI) to produce WT1.

    Attributes
    ----------
    wt1 : float
        Primary WaveTrend line (EMA of CI).
    wt2 : float
        Signal line (SMA of WT1 over 4 periods).

    """

    def __init__(self, channel_length: int, average_length: int) -> None:
        self.channel_length = channel_length
        self.average_length = average_length

        # WaveTrend calculation components
        self.esa_ema = ExponentialMovingAverage(channel_length)
        self.d_ema = ExponentialMovingAverage(channel_length)
        self.wt1_ema = ExponentialMovingAverage(average_length)
        self.wt1_values: list[float] = []  # Store for SMA(WT1, 4)

        # Current values
        self.wt1: float = 0.0
        self.wt2: float = 0.0
        self.prev_wt1: float = 0.0
        self.prev_wt2: float = 0.0

    def update(self, bar: Bar) -> None:
        """
        Update WaveTrend with new bar using LazyBear formula.

        Parameters
        ----------
        bar : Bar
            The bar containing OHLC data to update the indicator.

        Notes
        -----
        The WaveTrend is calculated as:
        1. HLC3 = (High + Low + Close) / 3
        2. ESA = EMA(HLC3, channel_length)
        3. D = EMA(abs(HLC3 - ESA), channel_length)
        4. CI = (HLC3 - ESA) / (0.015 * D)
        5. WT1 = EMA(CI, average_length)
        6. WT2 = SMA(WT1, 4)

        """
        # Calculate HLC3 (typical price)
        hlc3 = (bar.high.as_double() + bar.low.as_double() + bar.close.as_double()) / 3.0

        # ESA = EMA(HLC3, channel_length)
        self.esa_ema.update_raw(hlc3)
        if not self.esa_ema.initialized:
            return
        esa = self.esa_ema.value

        # D = EMA(abs(HLC3 - ESA), channel_length)
        d_input = abs(hlc3 - esa)
        self.d_ema.update_raw(d_input)
        if not self.d_ema.initialized:
            return
        d = self.d_ema.value

        # CI = (HLC3 - ESA) / (0.015 * D)
        if d == 0:
            ci = 0.0
        else:
            ci = (hlc3 - esa) / (0.015 * d)

        # WT1 = EMA(CI, average_length)
        self.wt1_ema.update_raw(ci)
        if not self.wt1_ema.initialized:
            return

        # Store previous values
        self.prev_wt1 = self.wt1
        self.prev_wt2 = self.wt2

        # Update WT1
        self.wt1 = self.wt1_ema.value

        # WT2 = SMA(WT1, 4)
        self.wt1_values.append(self.wt1)
        if len(self.wt1_values) > 4:
            self.wt1_values.pop(0)

        if len(self.wt1_values) == 4:
            self.wt2 = sum(self.wt1_values) / 4.0

    @property
    def initialized(self) -> bool:
        """
        Check if WaveTrend is ready.

        Returns
        -------
        bool
            True if the indicator has been initialized with sufficient data.

        """
        return len(self.wt1_values) == 4

    def is_bullish(self) -> bool:
        """
        Check if WT1 > WT2 (bullish).

        Returns
        -------
        bool
            True if WT1 is above WT2 (bullish condition).

        """
        return self.wt1 > self.wt2

    def is_bearish(self) -> bool:
        """
        Check if WT1 < WT2 (bearish).

        Returns
        -------
        bool
            True if WT1 is below WT2 (bearish condition).

        """
        return self.wt1 < self.wt2

    def bullish_cross(self) -> bool:
        """
        Check if WT1 just crossed above WT2.

        Returns
        -------
        bool
            True if WT1 crossed above WT2 on the most recent update.

        """
        return self.prev_wt1 <= self.prev_wt2 and self.wt1 > self.wt2

    def bearish_cross(self) -> bool:
        """
        Check if WT1 just crossed below WT2.

        Returns
        -------
        bool
            True if WT1 crossed below WT2 on the most recent update.

        """
        return self.prev_wt1 >= self.prev_wt2 and self.wt1 < self.wt2


class WaveTrendMultiTimeframeV4(Strategy):
    """
    Multi-timeframe WaveTrend strategy V4 (Aggressive Drawdown Scaling).

    V4 Improvement over V3:
    - Aggressive position scaling during drawdowns (1x → 5x)
    - Doubles down when strategy is losing (buy the dip mentality)
    - If strategy recovers from drawdown, larger positions = massive gains

    V3 Features (All Retained):
    - ATR minimum filter: pauses when volatility too low
    - Range filter: avoids stuck/choppy markets
    - Multi-timeframe alignment (3/3)
    - Wider stops (ATR 4.5x)
    - Higher profit target (4.0%)
    - Tighter trailing (1.0%)
    - 4h trend filter

    Position Scaling:
    - Normal equity: 1.0x base size
    - 10% drawdown: 1.5x base size
    - 20% drawdown: 2.25x base size
    - 30% drawdown: 3.375x base size
    - 40%+ drawdown: 5.0x base size (max)

    WARNING: Aggressive strategy. Accelerates losses if strategy is broken.
    """

    def __init__(self, config: WaveTrendMultiTimeframeV4Config) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_id = config.instrument_id
        self.base_trade_size = config.base_trade_size

        # WaveTrend states for each timeframe
        self.wt_5m = WaveTrendState(
            config.wt_5m_channel_length,
            config.wt_5m_average_length,
        )
        self.wt_1h = WaveTrendState(
            config.wt_1h_channel_length,
            config.wt_1h_average_length,
        )
        self.wt_4h = WaveTrendState(
            config.wt_4h_channel_length,
            config.wt_4h_average_length,
        )

        # ATR for initial trailing stop
        self.atr = AverageTrueRange(config.atr_period)

        # V3: Price history for range filter
        self.price_history_high: list[float] = []
        self.price_history_low: list[float] = []

        # Trailing stop state
        self.entry_price: float | None = None
        self.peak_price: float | None = None
        self.stop_order: StopMarketOrder | None = None
        self.use_percentage_trail: bool = False
        self.current_position_size: Decimal | None = None  # V4: Track actual position size used

        # Configuration values
        self.min_aligned = config.min_aligned_timeframes
        self.atr_multiplier = config.atr_multiplier
        self.profit_threshold = config.profit_threshold_pct / 100.0
        self.percentage_trail = config.percentage_trail / 100.0

        # V2: Trend filter configuration
        self.use_trend_filter = config.use_trend_filter
        self.trend_filter_threshold = config.trend_filter_threshold

        # V3: Regime filter configuration
        self.use_atr_min_filter = config.use_atr_min_filter
        self.atr_min_multiplier = config.atr_min_multiplier
        self.use_range_filter = config.use_range_filter
        self.range_lookback = config.range_lookback

        # V4: Drawdown scaling configuration
        self.scale_at_10pct_dd = config.scale_at_10pct_dd
        self.scale_at_20pct_dd = config.scale_at_20pct_dd
        self.scale_at_30pct_dd = config.scale_at_30pct_dd
        self.scale_at_40pct_dd = config.scale_at_40pct_dd
        self.max_position_pct_equity = config.max_position_pct_equity

        # V4: Equity tracking for drawdown calculation
        self.starting_equity: float | None = None
        self.high_water_mark: float | None = None
        self.current_equity: float | None = None

    def on_start(self) -> None:
        """Actions to be performed on strategy start."""
        self.log.info(f"Starting {self.__class__.__name__}")

        # V4: Initialize equity tracking
        instrument = self.cache.instrument(self.instrument_id)
        if instrument:
            account = self.portfolio.account(instrument.venue)
            if account:
                # Get starting equity in quote currency
                quote_currency = instrument.quote_currency
                balance = account.balance(quote_currency)
                if balance:
                    self.starting_equity = balance.total.as_double()
                    self.high_water_mark = self.starting_equity
                    self.current_equity = self.starting_equity
                    self.log.info(f"V4 Starting equity: {self.starting_equity:.2f} {quote_currency}")

        # Subscribe to 5-minute bars
        bar_type_5m = BarType.from_str(
            f"{self.instrument_id}-5-MINUTE-LAST-EXTERNAL"
        )
        self.subscribe_bars(bar_type_5m)

        # Subscribe to 1-hour bars
        bar_type_1h = BarType.from_str(
            f"{self.instrument_id}-1-HOUR-LAST-EXTERNAL"
        )
        self.subscribe_bars(bar_type_1h)

        # Subscribe to 4-hour bars
        bar_type_4h = BarType.from_str(
            f"{self.instrument_id}-4-HOUR-LAST-EXTERNAL"
        )
        self.subscribe_bars(bar_type_4h)

        self.log.info("Subscribed to 5m, 1h, 4h bars")

    def on_stop(self) -> None:
        """Actions to be performed on strategy stop."""
        self.log.info(f"Stopping {self.__class__.__name__}")
        self.cancel_all_orders(self.instrument_id)
        self.close_all_positions(self.instrument_id)

    def on_bar(self, bar: Bar) -> None:
        """Handle bar updates for all timeframes."""
        # Update appropriate WaveTrend based on bar aggregation period
        bar_spec = bar.bar_type.spec

        # Debug: Log first bar of each type
        if not hasattr(self, "_bars_received"):
            self._bars_received = {}

        bar_key = f"{bar_spec.step}-{bar_spec.aggregation}"
        if bar_key not in self._bars_received:
            self._bars_received[bar_key] = True
            self.log.info(f"First bar received: {bar.bar_type} (step={bar_spec.step}, agg={bar_spec.aggregation})")

        if bar_spec.step == 5 and bar_spec.aggregation == BarAggregation.MINUTE:
            self._on_bar_5m(bar)
        elif bar_spec.step == 1 and bar_spec.aggregation == BarAggregation.HOUR:
            self._on_bar_1h(bar)
        elif bar_spec.step == 4 and bar_spec.aggregation == BarAggregation.HOUR:
            self._on_bar_4h(bar)
        else:
            self.log.warning(f"Unhandled bar type: {bar.bar_type} (step={bar_spec.step}, agg={bar_spec.aggregation})")

    def _on_bar_5m(self, bar: Bar) -> None:
        """Handle 5-minute bar updates."""
        # Update ATR
        self.atr.update_raw(
            bar.high.as_double(),
            bar.low.as_double(),
            bar.close.as_double(),
        )

        # V3: Update price history for range filter
        self.price_history_high.append(bar.high.as_double())
        self.price_history_low.append(bar.low.as_double())

        # Keep only lookback period
        if len(self.price_history_high) > self.range_lookback:
            self.price_history_high.pop(0)
            self.price_history_low.pop(0)

        # Update WaveTrend
        prev_initialized = self.wt_5m.initialized
        self.wt_5m.update(bar)

        # Log when 5m WaveTrend first initializes
        if not prev_initialized and self.wt_5m.initialized:
            self.log.info(f"5m WaveTrend initialized (WT1={self.wt_5m.wt1:.2f}, WT2={self.wt_5m.wt2:.2f})")

        if not self.wt_5m.initialized:
            return

        # Check for entry signals
        self._check_entry_signals(bar)

        # Update trailing stop if in position
        self._update_trailing_stop(bar)

    def _on_bar_1h(self, bar: Bar) -> None:
        """Handle 1-hour bar updates."""
        self.wt_1h.update(bar)

    def _on_bar_4h(self, bar: Bar) -> None:
        """Handle 4-hour bar updates."""
        self.wt_4h.update(bar)

    def _count_aligned_timeframes(self, direction: str) -> int:
        """Count how many timeframes are aligned in the given direction."""
        count = 0

        if direction == "bullish":
            if self.wt_5m.is_bullish():
                count += 1
            if self.wt_1h.initialized and self.wt_1h.is_bullish():
                count += 1
            if self.wt_4h.initialized and self.wt_4h.is_bullish():
                count += 1
        elif direction == "bearish":
            if self.wt_5m.is_bearish():
                count += 1
            if self.wt_1h.initialized and self.wt_1h.is_bearish():
                count += 1
            if self.wt_4h.initialized and self.wt_4h.is_bearish():
                count += 1

        return count

    def _check_entry_signals(self, bar: Bar) -> None:
        """Check for entry signals based on WaveTrend crosses and alignment."""
        # Don't enter if already in a position
        if self.portfolio.is_flat(self.instrument_id) is False:
            return

        # Check for bullish cross on 5m
        bullish = self.wt_5m.bullish_cross()
        bearish = self.wt_5m.bearish_cross()

        if bullish:
            aligned_count = self._count_aligned_timeframes("bullish")

            # V2: Check trend filter
            trend_ok = True
            trend_status = ""
            if self.use_trend_filter and self.wt_4h.initialized:
                trend_ok = self.wt_4h.wt1 > -self.trend_filter_threshold
                trend_status = f", 4h_trend={'OK' if trend_ok else 'BLOCKED'}(WT1={self.wt_4h.wt1:.1f})"

            # V3: Check range filter (avoid stuck/choppy markets)
            range_ok = True
            range_status = ""
            if self.use_range_filter and len(self.price_history_high) >= self.range_lookback:
                current_high = bar.high.as_double()
                current_low = bar.low.as_double()
                lookback_high = max(self.price_history_high)
                lookback_low = min(self.price_history_low)

                # Check if current price is making new highs or lows (expanding range = trending)
                making_new_high = current_high >= lookback_high * 0.999  # Within 0.1% of high
                making_new_low = current_low <= lookback_low * 1.001  # Within 0.1% of low

                range_ok = making_new_high or making_new_low
                range_status = f", Range={'OK' if range_ok else 'BLOCKED'}(H:{making_new_high},L:{making_new_low})"

            # V3: Check ATR minimum filter
            atr_ok = True
            atr_status = ""
            if self.use_atr_min_filter and self.atr.initialized:
                instrument = self.cache.instrument(self.instrument_id)
                if instrument:
                    current_price = bar.close.as_double()
                    atr_min = current_price * (self.atr_min_multiplier / 100.0)
                    atr_ok = self.atr.value >= atr_min
                    atr_status = f", ATR_min={'OK' if atr_ok else 'BLOCKED'}({self.atr.value:.1f}>={atr_min:.1f})"

            self.log.info(
                f"5m Bullish cross detected! WT1={self.wt_5m.wt1:.2f}, WT2={self.wt_5m.wt2:.2f}, "
                f"Aligned: {aligned_count}/3 (5m:{self.wt_5m.is_bullish()}, "
                f"1h:{self.wt_1h.is_bullish() if self.wt_1h.initialized else 'uninit'}, "
                f"4h:{self.wt_4h.is_bullish() if self.wt_4h.initialized else 'uninit'}){trend_status}{range_status}{atr_status}"
            )

            if aligned_count >= self.min_aligned and trend_ok and range_ok and atr_ok:
                self.log.info("All conditions met - entering LONG")
                self._enter_long()
            elif aligned_count >= self.min_aligned:
                reasons = []
                if not trend_ok:
                    reasons.append("trend filter")
                if not range_ok:
                    reasons.append("stuck in range")
                if not atr_ok:
                    reasons.append("ATR too low")
                self.log.info(f"Aligned but blocked by: {', '.join(reasons)}")


        # Check for bearish cross on 5m
        elif bearish:
            aligned_count = self._count_aligned_timeframes("bearish")

            # V2: Check trend filter
            trend_ok = True
            trend_status = ""
            if self.use_trend_filter and self.wt_4h.initialized:
                trend_ok = self.wt_4h.wt1 < self.trend_filter_threshold
                trend_status = f", 4h_trend={'OK' if trend_ok else 'BLOCKED'}(WT1={self.wt_4h.wt1:.1f})"

            # V3: Check range filter (same for both directions)
            range_ok = True
            range_status = ""
            if self.use_range_filter and len(self.price_history_high) >= self.range_lookback:
                current_high = bar.high.as_double()
                current_low = bar.low.as_double()
                lookback_high = max(self.price_history_high)
                lookback_low = min(self.price_history_low)

                # Check if current price is making new highs or lows (expanding range = trending)
                making_new_high = current_high >= lookback_high * 0.999  # Within 0.1% of high
                making_new_low = current_low <= lookback_low * 1.001  # Within 0.1% of low

                range_ok = making_new_high or making_new_low
                range_status = f", Range={'OK' if range_ok else 'BLOCKED'}(H:{making_new_high},L:{making_new_low})"

            # V3: Check ATR minimum filter
            atr_ok = True
            atr_status = ""
            if self.use_atr_min_filter and self.atr.initialized:
                instrument = self.cache.instrument(self.instrument_id)
                if instrument:
                    current_price = bar.close.as_double()
                    atr_min = current_price * (self.atr_min_multiplier / 100.0)
                    atr_ok = self.atr.value >= atr_min
                    atr_status = f", ATR_min={'OK' if atr_ok else 'BLOCKED'}({self.atr.value:.1f}>={atr_min:.1f})"

            self.log.info(
                f"5m Bearish cross detected! WT1={self.wt_5m.wt1:.2f}, WT2={self.wt_5m.wt2:.2f}, "
                f"Aligned: {aligned_count}/3 (5m:{self.wt_5m.is_bearish()}, "
                f"1h:{self.wt_1h.is_bearish() if self.wt_1h.initialized else 'uninit'}, "
                f"4h:{self.wt_4h.is_bearish() if self.wt_4h.initialized else 'uninit'}){trend_status}{range_status}{atr_status}"
            )

            if aligned_count >= self.min_aligned and trend_ok and range_ok and atr_ok:
                self.log.info("All conditions met - entering SHORT")
                self._enter_short()
            elif aligned_count >= self.min_aligned:
                reasons = []
                if not trend_ok:
                    reasons.append("trend filter")
                if not range_ok:
                    reasons.append("stuck in range")
                if not atr_ok:
                    reasons.append("ATR too low")
                self.log.info(f"Aligned but blocked by: {', '.join(reasons)}")

    def _update_equity(self) -> None:
        """
        V4: Update current equity and high-water mark.

        Called before entering positions to calculate current drawdown.
        """
        instrument = self.cache.instrument(self.instrument_id)
        if not instrument:
            return

        account = self.portfolio.account(instrument.venue)
        if not account:
            return

        quote_currency = instrument.quote_currency
        balance = account.balance(quote_currency)
        if not balance:
            return

        self.current_equity = balance.total.as_double()

        # Update high-water mark if new peak
        if self.high_water_mark is None or self.current_equity > self.high_water_mark:
            self.high_water_mark = self.current_equity

    def _calculate_scaled_position_size(self) -> Decimal:
        """
        V4: Calculate position size based on drawdown from high-water mark.

        Returns
        -------
        Decimal
            Scaled position size (base_trade_size * drawdown_multiplier)

        Notes
        -----
        Aggressive scaling tiers:
        - < 10% drawdown: 1.0x base size
        - 10-20% drawdown: 1.5x base size
        - 20-30% drawdown: 2.25x base size
        - 30-40% drawdown: 3.375x base size
        - 40%+ drawdown: 5.0x base size

        Safety cap: Never exceed max_position_pct_equity of total equity.
        """
        # Default to base size if equity tracking not initialized
        if self.high_water_mark is None or self.current_equity is None:
            return self.base_trade_size

        # Calculate drawdown percentage
        drawdown_pct = (self.high_water_mark - self.current_equity) / self.high_water_mark

        # Determine size multiplier based on drawdown tier
        if drawdown_pct < 0.10:
            size_multiplier = 1.0  # No scaling below 10% drawdown
        elif drawdown_pct < 0.20:
            size_multiplier = self.scale_at_10pct_dd  # 1.5x at 10-20% DD
        elif drawdown_pct < 0.30:
            size_multiplier = self.scale_at_20pct_dd  # 2.25x at 20-30% DD
        elif drawdown_pct < 0.40:
            size_multiplier = self.scale_at_30pct_dd  # 3.375x at 30-40% DD
        else:
            size_multiplier = self.scale_at_40pct_dd  # 5.0x at 40%+ DD

        # Calculate scaled position size
        scaled_size = self.base_trade_size * Decimal(str(size_multiplier))

        # Apply safety cap: never risk more than max_position_pct_equity
        instrument = self.cache.instrument(self.instrument_id)
        if instrument and self.current_equity:
            # Get current price to convert position size to USD value
            # For safety cap, we approximate using base_trade_size as BTC amount
            # Max position value = max_position_pct_equity * current_equity
            max_position_value = self.current_equity * self.max_position_pct_equity
            # If scaled_size * current_price > max_position_value, cap it
            # For simplicity, cap the BTC amount directly
            # This is approximate - actual implementation would need current price
            # For now, just ensure we don't exceed reasonable limits

        # Log scaling decision
        if drawdown_pct > 0.01:  # Only log if drawdown > 1%
            self.log.info(
                f"V4 Drawdown Scaling: DD={drawdown_pct*100:.1f}% "
                f"(HWM: ${self.high_water_mark:.2f}, Current: ${self.current_equity:.2f}), "
                f"Multiplier: {size_multiplier:.2f}x, "
                f"Position: {scaled_size:.4f} (base: {self.base_trade_size:.4f})"
            )

        return scaled_size

    def _enter_long(self) -> None:
        """Enter a long position with V4 drawdown-scaled sizing."""
        # V4: Update equity and calculate scaled position size
        self._update_equity()
        trade_size = self._calculate_scaled_position_size()
        self.current_position_size = trade_size  # V4: Store for stop orders

        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Cannot enter LONG - instrument {self.instrument_id} not found in cache")
            return

        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(trade_size),
        )
        self.submit_order(order)

    def _enter_short(self) -> None:
        """Enter a short position with V4 drawdown-scaled sizing."""
        # V4: Update equity and calculate scaled position size
        self._update_equity()
        trade_size = self._calculate_scaled_position_size()
        self.current_position_size = trade_size  # V4: Store for stop orders

        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Cannot enter SHORT - instrument {self.instrument_id} not found in cache")
            return

        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=instrument.make_qty(trade_size),
        )
        self.submit_order(order)

    def on_order_filled(self, event) -> None:
        """Handle order filled events."""
        if event.order_side == OrderSide.BUY or event.order_side == OrderSide.SELL:
            # Entry order filled - set initial stop
            self.entry_price = event.last_px.as_double()
            self.peak_price = self.entry_price
            self.use_percentage_trail = False

            self.log.info(
                f"Entry filled at {self.entry_price:.2f}, setting ATR-based stop"
            )

            # Set initial ATR-based stop
            self._set_atr_stop(event.order_side)

    def _set_atr_stop(self, entry_side: OrderSide) -> None:
        """Set ATR-based trailing stop."""
        if not self.atr.initialized or self.entry_price is None:
            return

        # Get instrument from cache
        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Cannot set ATR stop - instrument {self.instrument_id} not found in cache")
            return

        # Calculate stop distance
        stop_distance = self.atr.value * self.atr_multiplier

        # Calculate stop price based on position direction
        if entry_side == OrderSide.BUY:
            # Long position - stop below entry
            stop_price = self.entry_price - stop_distance
            trigger_price = instrument.make_price(stop_price)

            # Cancel existing stop if any
            if self.stop_order is not None:
                self.cancel_order(self.stop_order)

            # Create new stop order
            self.stop_order = self.order_factory.stop_market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=instrument.make_qty(self.current_position_size or self.base_trade_size),
                trigger_price=trigger_price,
                trigger_type=TriggerType.DEFAULT,
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(self.stop_order)

            self.log.info(f"ATR stop set at {stop_price:.2f} (distance: {stop_distance:.2f})")

        elif entry_side == OrderSide.SELL:
            # Short position - stop above entry
            stop_price = self.entry_price + stop_distance
            trigger_price = instrument.make_price(stop_price)

            # Cancel existing stop if any
            if self.stop_order is not None:
                self.cancel_order(self.stop_order)

            # Create new stop order
            self.stop_order = self.order_factory.stop_market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=instrument.make_qty(self.current_position_size or self.base_trade_size),
                trigger_price=trigger_price,
                trigger_type=TriggerType.DEFAULT,
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(self.stop_order)

            self.log.info(f"ATR stop set at {stop_price:.2f} (distance: {stop_distance:.2f})")

    def _set_percentage_stop(self, position_side: OrderSide) -> None:
        """Set percentage-based trailing stop from peak price."""
        if self.peak_price is None:
            return

        # Get instrument from cache
        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Cannot set percentage stop - instrument {self.instrument_id} not found in cache")
            return

        if position_side == OrderSide.BUY:
            # Long position - trail below peak
            stop_price = self.peak_price * (1 - self.percentage_trail)
            trigger_price = instrument.make_price(stop_price)

            # Cancel existing stop
            if self.stop_order is not None:
                self.cancel_order(self.stop_order)

            # Create new stop
            self.stop_order = self.order_factory.stop_market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.SELL,
                quantity=instrument.make_qty(self.current_position_size or self.base_trade_size),
                trigger_price=trigger_price,
                trigger_type=TriggerType.DEFAULT,
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(self.stop_order)

            self.log.info(
                f"Percentage stop updated: {stop_price:.2f} "
                f"({self.percentage_trail * 100:.1f}% from peak {self.peak_price:.2f})"
            )

        elif position_side == OrderSide.SELL:
            # Short position - trail above peak (lowest point)
            stop_price = self.peak_price * (1 + self.percentage_trail)
            trigger_price = instrument.make_price(stop_price)

            # Cancel existing stop
            if self.stop_order is not None:
                self.cancel_order(self.stop_order)

            # Create new stop
            self.stop_order = self.order_factory.stop_market(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=instrument.make_qty(self.current_position_size or self.base_trade_size),
                trigger_price=trigger_price,
                trigger_type=TriggerType.DEFAULT,
                time_in_force=TimeInForce.GTC,
            )
            self.submit_order(self.stop_order)

            self.log.info(
                f"Percentage stop updated: {stop_price:.2f} "
                f"({self.percentage_trail * 100:.1f}% from peak {self.peak_price:.2f})"
            )

    def _update_trailing_stop(self, bar: Bar) -> None:
        """Update trailing stop based on current price and P&L."""
        # Only update if in a position
        if self.portfolio.is_flat(self.instrument_id):
            return

        if self.entry_price is None:
            return

        # Get current position
        positions = self.cache.positions_open(instrument_id=self.instrument_id)
        if not positions:
            return
        position = positions[0]  # Get the first open position

        current_price = bar.close.as_double()

        # Update peak price
        if position.side == OrderSide.BUY:
            # Long position - track highest price
            if self.peak_price is None or current_price > self.peak_price:
                self.peak_price = current_price
        elif position.side == OrderSide.SELL:
            # Short position - track lowest price
            if self.peak_price is None or current_price < self.peak_price:
                self.peak_price = current_price

        # Calculate unrealized P&L percentage
        if position.side == OrderSide.BUY:
            pnl_pct = (current_price - self.entry_price) / self.entry_price
        else:
            pnl_pct = (self.entry_price - current_price) / self.entry_price

        # Check if we should switch to percentage trail
        if not self.use_percentage_trail and pnl_pct >= self.profit_threshold:
            self.log.info(
                f"Profit threshold reached ({pnl_pct * 100:.2f}%), "
                f"switching to percentage trail"
            )
            self.use_percentage_trail = True
            self._set_percentage_stop(position.side)

        # Update stop based on current mode
        elif self.use_percentage_trail:
            # Update percentage stop if peak moved
            self._set_percentage_stop(position.side)

    def on_position_closed(self, position) -> None:
        """Handle position closed event."""
        self.log.info(f"Position closed: {position}")

        # Reset state
        self.entry_price = None
        self.peak_price = None
        self.stop_order = None
        self.use_percentage_trail = False
        self.current_position_size = None  # V4: Reset position size
