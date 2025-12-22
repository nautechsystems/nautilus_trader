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


class WaveTrendMultiTimeframeV4_3ImprovedConfig(StrategyConfig, frozen=True, kw_only=True):
    """
    Configuration for WaveTrend Multi-Timeframe strategy V4.3 Improved.

    V4.3 Improved fixes over original V4.3:
    1. Add V4.1's volatility BLOCKING (blocks HIGH/ELEVATED volatility)
    2. Faster regime detection: 24h vs 7d (was 48h vs 30d) - 4x faster
    3. Tighter stops in remaining regimes
    4. Simplify to 2 regimes (NORMAL/LOW) since we block HIGH/ELEVATED

    Original V4.3 traded through ALL volatility with adjusted stops.
    This FAILED in choppy 2022 (-0.04% vs V4.1's +0.55%).

    V4.3 Improved BLOCKS bad conditions AND adjusts exits dynamically.

    Expected Result:
    - Fewer trades than original V4.3 (55-65 vs 73)
    - Better 2022 performance (blocking prevents choppy losses)
    - Target: +6-7% over 4 years

    Risk:
    - Faster detection might exit good trends prematurely

    Notes
    -----
    Volatility Regime & Exit Parameters:
    - HIGH (>1.5x baseline): BLOCKED (no entry)
    - ELEVATED (1.1-1.5x): BLOCKED (no entry)
    - NORMAL (0.9-1.1x): 3.5x ATR stop, 3.0% profit target
    - LOW (<0.9x): 5.0x ATR stop, 5.0% profit target

    """

    instrument_id: InstrumentId
    trade_size: Decimal

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

    # V4.3 Improved: Volatility blocking (CRITICAL FIX)
    use_volatility_blocking: bool = True  # BLOCK HIGH/ELEVATED volatility

    # V4.3 Improved: Faster regime detection (4x faster)
    atr_recent_bars: PositiveInt = 288  # 24 hours at 5m (was 576 = 48h)
    atr_baseline_bars: PositiveInt = 2016  # 7 days at 5m (was 8640 = 30d)
    high_vol_threshold: PositiveFloat = 1.5  # Recent/Baseline > 1.5 = HIGH
    elevated_vol_threshold: PositiveFloat = 1.1  # Recent/Baseline > 1.1 = ELEVATED
    low_vol_threshold: PositiveFloat = 0.9  # Recent/Baseline < 0.9 = LOW

    # V4.3 Improved: Simplified to 2 regimes (we block HIGH/ELEVATED)
    # NORMAL volatility (0.9-1.1x): Tightened from original
    normal_vol_atr_mult: PositiveFloat = 3.5  # Was 4.5 in original V4.3
    normal_vol_profit_pct: PositiveFloat = 3.0  # Was 4.0

    # LOW volatility (<0.9x): Tightened from original
    low_vol_atr_mult: PositiveFloat = 5.0  # Was 6.0
    low_vol_profit_pct: PositiveFloat = 5.0  # Was 6.0

    # Order management
    order_id_tag: str = "WT_MTF_V4_3I"


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


class WaveTrendMultiTimeframeV4_3Improved(Strategy):
    """
    Multi-timeframe WaveTrend strategy V4.3 Improved (Hybrid Blocking + Dynamic Exits).

    V4.3 Improved fixes over original V4.3:
    - BLOCKS HIGH/ELEVATED volatility (V4.1's approach)
    - Faster regime detection: 24h vs 7d (4x faster)
    - Tightened stops/targets in NORMAL/LOW regimes
    - Simplified to 2 regimes (only trade NORMAL/LOW)

    Original V4.3 traded through ALL volatility with adjusted stops.
    This FAILED in choppy 2022 (-0.04% vs V4.1's +0.55%).

    V4.3 Improved combines the best of both:
    - BLOCK bad conditions (HIGH/ELEVATED) like V4.1
    - ADJUST exits dynamically in remaining conditions (NORMAL/LOW) like V4.3
    - DETECT regimes 4x faster for quicker adaptation

    Expected: 55-65 trades, +6-7% over 4 years.
    """

    def __init__(self, config: WaveTrendMultiTimeframeV4_3ImprovedConfig) -> None:
        super().__init__(config)

        # Configuration
        self.instrument_id = config.instrument_id
        self.trade_size = config.trade_size

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

        # V4.3 Improved: ATR history for volatility regime detection (4x faster)
        self.atr_history: list[float] = []

        # Trailing stop state
        self.entry_price: float | None = None
        self.peak_price: float | None = None
        self.stop_order: StopMarketOrder | None = None
        self.use_percentage_trail: bool = False

        # V4.3 Improved: Current profit target (set dynamically per trade based on regime)
        self.current_profit_target: float | None = None

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

        # V4.3 Improved: Volatility blocking configuration (CRITICAL FIX)
        self.use_volatility_blocking = config.use_volatility_blocking

        # V4.3 Improved: Volatility regime detection configuration (4x faster)
        self.atr_recent_bars = config.atr_recent_bars
        self.atr_baseline_bars = config.atr_baseline_bars
        self.high_vol_threshold = config.high_vol_threshold
        self.elevated_vol_threshold = config.elevated_vol_threshold
        self.low_vol_threshold = config.low_vol_threshold

        # V4.3 Improved: Dynamic exit parameters (2 regimes only)
        self.normal_vol_atr_mult = config.normal_vol_atr_mult
        self.normal_vol_profit_pct = config.normal_vol_profit_pct
        self.low_vol_atr_mult = config.low_vol_atr_mult
        self.low_vol_profit_pct = config.low_vol_profit_pct

    def on_start(self) -> None:
        """Actions to be performed on strategy start."""
        self.log.info(f"Starting {self.__class__.__name__}")

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

        # V4.3 Improved: Track ATR history for volatility regime detection
        if self.atr.initialized:
            self.atr_history.append(self.atr.value)
            # Keep only baseline period (7 days = 2016 bars at 5m)
            if len(self.atr_history) > self.atr_baseline_bars:
                self.atr_history.pop(0)

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

    def _detect_volatility_regime(self) -> str:
        """
        V4.3 Improved: Detect current volatility regime for blocking and exit adjustment.

        Compares recent ATR (24h) vs baseline ATR (7d) to classify
        market volatility state. 4x faster than original V4.3.

        Returns
        -------
        str
            One of: 'HIGH', 'ELEVATED', 'NORMAL', 'LOW'
        """
        # Need sufficient ATR history
        if len(self.atr_history) < self.atr_baseline_bars:
            return "NORMAL"  # Default until enough data

        # Calculate recent ATR average (24h = 288 bars at 5m)
        if len(self.atr_history) >= self.atr_recent_bars:
            atr_recent = sum(self.atr_history[-self.atr_recent_bars:]) / self.atr_recent_bars
        else:
            atr_recent = sum(self.atr_history) / len(self.atr_history)

        # Calculate baseline ATR average (7d = 2016 bars)
        atr_baseline = sum(self.atr_history) / len(self.atr_history)

        # Avoid division by zero
        if atr_baseline == 0:
            return "NORMAL"

        # Calculate volatility ratio
        vol_ratio = atr_recent / atr_baseline

        # Classify regime
        if vol_ratio >= self.high_vol_threshold:
            return "HIGH"  # Chop accelerating
        elif vol_ratio >= self.elevated_vol_threshold:
            return "ELEVATED"  # Chop continuing
        elif vol_ratio <= self.low_vol_threshold:
            return "LOW"  # Chop ending / trending
        else:
            return "NORMAL"

    def _get_exit_parameters(self, regime: str) -> tuple[float, float]:
        """
        Get ATR multiplier and profit target % based on regime (V4.3 Improved: 2 regimes only).

        Parameters
        ----------
        regime : str
            Volatility regime: 'LOW', 'NORMAL', 'ELEVATED', or 'HIGH'

        Returns
        -------
        tuple[float, float]
            (atr_multiplier, profit_target_pct) - ATR multiplier for stop, profit % threshold

        Notes
        -----
        V4.3 Improved uses only 2 regimes since HIGH/ELEVATED are blocked.
        If we somehow receive HIGH/ELEVATED, we use NORMAL defaults with a warning.
        """
        if regime == "LOW":
            return (self.low_vol_atr_mult, self.low_vol_profit_pct)
        elif regime == "NORMAL":
            return (self.normal_vol_atr_mult, self.normal_vol_profit_pct)
        else:
            # HIGH/ELEVATED should be blocked, but if we somehow get here, use NORMAL defaults
            self.log.warning(f"Unexpected regime {regime} in _get_exit_parameters, using NORMAL defaults")
            return (self.normal_vol_atr_mult, self.normal_vol_profit_pct)

    def _check_entry_signals(self, bar: Bar) -> None:
        """Check for entry signals based on WaveTrend crosses and alignment."""
        # Don't enter if already in a position
        if self.portfolio.is_flat(self.instrument_id) is False:
            return

        # V4.3 Improved: BLOCK HIGH/ELEVATED volatility (critical fix)
        if self.use_volatility_blocking:
            volatility_regime = self._detect_volatility_regime()
            if volatility_regime in ("HIGH", "ELEVATED"):
                # Silent blocking - don't log on every bar
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

    def _enter_long(self) -> None:
        """Enter a long position."""
        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Cannot enter LONG - instrument {self.instrument_id} not found in cache")
            return

        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(self.trade_size),
        )
        self.submit_order(order)

    def _enter_short(self) -> None:
        """Enter a short position."""
        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Cannot enter SHORT - instrument {self.instrument_id} not found in cache")
            return

        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=instrument.make_qty(self.trade_size),
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
        """Set ATR-based trailing stop with dynamic adjustment based on volatility regime."""
        if not self.atr.initialized or self.entry_price is None:
            return

        # Get instrument from cache
        instrument = self.cache.instrument(self.instrument_id)
        if instrument is None:
            self.log.error(f"Cannot set ATR stop - instrument {self.instrument_id} not found in cache")
            return

        # V4.3 Improved: Detect volatility regime and adjust stop multiplier + profit target
        vol_regime = self._detect_volatility_regime()

        # Get regime-specific parameters
        atr_mult, profit_pct = self._get_exit_parameters(vol_regime)
        self.current_profit_target = profit_pct / 100.0

        self.log.info(
            f"Vol regime: {vol_regime} → ATR mult: {atr_mult}x, Profit target: {self.current_profit_target * 100:.1f}%"
        )

        # Calculate stop distance
        stop_distance = self.atr.value * atr_mult

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
                quantity=instrument.make_qty(self.trade_size),
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
                quantity=instrument.make_qty(self.trade_size),
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
                quantity=instrument.make_qty(self.trade_size),
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
                quantity=instrument.make_qty(self.trade_size),
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

        # V4.3 Improved: Check if we should switch to percentage trail (using dynamic target)
        if not self.use_percentage_trail and self.current_profit_target is not None and pnl_pct >= self.current_profit_target:
            self.log.info(
                f"Profit threshold reached ({pnl_pct * 100:.2f}% >= {self.current_profit_target * 100:.1f}%), "
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
        self.current_profit_target = None  # V4.3 Improved: Reset dynamic profit target
