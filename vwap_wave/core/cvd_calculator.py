# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - CVD Calculator
# -------------------------------------------------------------------------------------------------
"""
Cumulative Volume Delta (CVD) calculation and divergence detection.

Since tick-level bid/ask data may not be available, this implements a bar-based
proxy using the close-open relationship to estimate buying/selling pressure.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from typing import Optional

import numpy as np

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import Bar

from vwap_wave.config.settings import CVDConfig


@dataclass
class CVDDivergence:
    """Detected CVD divergence."""

    detected: bool
    divergence_type: str  # "BEARISH" or "BULLISH" or ""
    price_extreme: float
    cvd_extreme: float
    divergence_magnitude: float


class CVDCalculator(Indicator):
    """
    Cumulative Volume Delta calculator with divergence detection.

    Uses bar-based proxy: CVD += volume * sign(close - open)
    Detects divergence when price makes new extreme but CVD does not confirm.

    Parameters
    ----------
    config : CVDConfig
        The CVD configuration.
    max_history : int
        Maximum history to retain (default: 100).

    """

    def __init__(self, config: CVDConfig, max_history: int = 100):
        super().__init__(params=[config.lookback_bars])

        self.config = config
        self._lookback_bars = config.lookback_bars
        self._min_divergence_delta = config.min_divergence_delta

        # State tracking
        self._cvd_history: deque = deque(maxlen=max_history)
        self._price_history: deque = deque(maxlen=max_history)
        self._cumulative_cvd: float = 0.0
        self._bar_count: int = 0

    def handle_bar(self, bar: Bar) -> None:
        """
        Process bar and update CVD.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        """
        PyCondition.not_none(bar, "bar")

        open_price = bar.open.as_double()
        close_price = bar.close.as_double()
        volume = bar.volume.as_double()

        # Calculate delta for this bar using close-open relationship
        if close_price > open_price:
            delta = volume  # Buying pressure
        elif close_price < open_price:
            delta = -volume  # Selling pressure
        else:
            delta = 0.0

        self._cumulative_cvd += delta
        self._cvd_history.append(self._cumulative_cvd)
        self._price_history.append({
            "high": bar.high.as_double(),
            "low": bar.low.as_double(),
            "close": close_price,
        })
        self._bar_count += 1

        # Update initialization state
        if not self.has_inputs:
            self._set_has_inputs(True)
        if not self.initialized and self._bar_count >= self._lookback_bars * 2:
            self._set_initialized(True)

    def detect_bearish_divergence(self) -> CVDDivergence:
        """
        Detect bearish divergence: price higher high, CVD lower high.

        Indicates buying exhaustion at tops.

        Returns
        -------
        CVDDivergence
            The divergence detection result.

        """
        if len(self._price_history) < self._lookback_bars * 2:
            return CVDDivergence(
                detected=False,
                divergence_type="",
                price_extreme=0.0,
                cvd_extreme=0.0,
                divergence_magnitude=0.0,
            )

        lookback = self._lookback_bars

        # Find recent price high
        recent_prices = list(self._price_history)[-lookback:]
        recent_highs = [p["high"] for p in recent_prices]
        recent_high_idx = int(np.argmax(recent_highs))
        recent_high = recent_highs[recent_high_idx]

        # Find prior price high
        prior_prices = list(self._price_history)[-(lookback * 2) : -lookback]
        prior_highs = [p["high"] for p in prior_prices]
        prior_high_idx = int(np.argmax(prior_highs))
        prior_high = prior_highs[prior_high_idx]

        # Get CVD at those points
        recent_cvd_values = list(self._cvd_history)[-lookback:]
        prior_cvd_values = list(self._cvd_history)[-(lookback * 2) : -lookback]

        if not recent_cvd_values or not prior_cvd_values:
            return CVDDivergence(
                detected=False,
                divergence_type="",
                price_extreme=0.0,
                cvd_extreme=0.0,
                divergence_magnitude=0.0,
            )

        recent_cvd = recent_cvd_values[recent_high_idx]
        prior_cvd = prior_cvd_values[prior_high_idx]

        # Check for divergence: price higher high but CVD lower high
        price_higher = recent_high > prior_high
        cvd_lower = recent_cvd < prior_cvd

        if price_higher and cvd_lower and abs(prior_cvd) > 0:
            magnitude = abs(prior_cvd - recent_cvd) / abs(prior_cvd)
            if magnitude >= self._min_divergence_delta:
                return CVDDivergence(
                    detected=True,
                    divergence_type="BEARISH",
                    price_extreme=recent_high,
                    cvd_extreme=recent_cvd,
                    divergence_magnitude=magnitude,
                )

        return CVDDivergence(
            detected=False,
            divergence_type="",
            price_extreme=0.0,
            cvd_extreme=0.0,
            divergence_magnitude=0.0,
        )

    def detect_bullish_divergence(self) -> CVDDivergence:
        """
        Detect bullish divergence: price lower low, CVD higher low.

        Indicates selling exhaustion at bottoms.

        Returns
        -------
        CVDDivergence
            The divergence detection result.

        """
        if len(self._price_history) < self._lookback_bars * 2:
            return CVDDivergence(
                detected=False,
                divergence_type="",
                price_extreme=0.0,
                cvd_extreme=0.0,
                divergence_magnitude=0.0,
            )

        lookback = self._lookback_bars

        # Find recent price low
        recent_prices = list(self._price_history)[-lookback:]
        recent_lows = [p["low"] for p in recent_prices]
        recent_low_idx = int(np.argmin(recent_lows))
        recent_low = recent_lows[recent_low_idx]

        # Find prior price low
        prior_prices = list(self._price_history)[-(lookback * 2) : -lookback]
        prior_lows = [p["low"] for p in prior_prices]
        prior_low_idx = int(np.argmin(prior_lows))
        prior_low = prior_lows[prior_low_idx]

        # Get CVD at those points
        recent_cvd_values = list(self._cvd_history)[-lookback:]
        prior_cvd_values = list(self._cvd_history)[-(lookback * 2) : -lookback]

        if not recent_cvd_values or not prior_cvd_values:
            return CVDDivergence(
                detected=False,
                divergence_type="",
                price_extreme=0.0,
                cvd_extreme=0.0,
                divergence_magnitude=0.0,
            )

        recent_cvd = recent_cvd_values[recent_low_idx]
        prior_cvd = prior_cvd_values[prior_low_idx]

        # Check for divergence: price lower low but CVD higher low
        price_lower = recent_low < prior_low
        cvd_higher = recent_cvd > prior_cvd

        if price_lower and cvd_higher and abs(prior_cvd) > 0:
            magnitude = abs(recent_cvd - prior_cvd) / abs(prior_cvd)
            if magnitude >= self._min_divergence_delta:
                return CVDDivergence(
                    detected=True,
                    divergence_type="BULLISH",
                    price_extreme=recent_low,
                    cvd_extreme=recent_cvd,
                    divergence_magnitude=magnitude,
                )

        return CVDDivergence(
            detected=False,
            divergence_type="",
            price_extreme=0.0,
            cvd_extreme=0.0,
            divergence_magnitude=0.0,
        )

    def detect_any_divergence(self) -> CVDDivergence:
        """
        Detect any divergence (bearish or bullish).

        Returns
        -------
        CVDDivergence
            The divergence detection result.

        """
        bearish = self.detect_bearish_divergence()
        if bearish.detected:
            return bearish

        bullish = self.detect_bullish_divergence()
        if bullish.detected:
            return bullish

        return CVDDivergence(
            detected=False,
            divergence_type="",
            price_extreme=0.0,
            cvd_extreme=0.0,
            divergence_magnitude=0.0,
        )

    def _reset(self) -> None:
        """Reset the indicator (called by base class)."""
        self._cumulative_cvd = 0.0
        self._cvd_history.clear()
        self._price_history.clear()
        self._bar_count = 0

    def reset_session(self) -> None:
        """Reset CVD for new session (manual call)."""
        self._cumulative_cvd = 0.0
        self._cvd_history.clear()
        self._price_history.clear()

    @property
    def current_cvd(self) -> float:
        """Current cumulative CVD value."""
        return self._cumulative_cvd

    @property
    def cvd_trend(self) -> float:
        """
        Recent CVD trend direction.

        Returns positive for rising CVD, negative for falling.
        """
        if len(self._cvd_history) < 5:
            return 0.0

        recent = list(self._cvd_history)[-5:]
        return recent[-1] - recent[0]

    @property
    def is_cvd_rising(self) -> bool:
        """Check if CVD is in an upward trend."""
        return self.cvd_trend > 0

    @property
    def is_cvd_falling(self) -> bool:
        """Check if CVD is in a downward trend."""
        return self.cvd_trend < 0
