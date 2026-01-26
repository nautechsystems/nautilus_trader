# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Acceptance Engine
# -------------------------------------------------------------------------------------------------
"""
Acceptance evaluation using time, distance, and volume confirmation.

Determines whether price has accepted a level using multiple confirmation modes.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from enum import Enum
from typing import Optional

from nautilus_trader.model.data import Bar

from vwap_wave.config.settings import AcceptanceConfig


class AcceptanceType(Enum):
    """Type of acceptance detected."""

    NONE = "none"
    TIME = "time"
    DISTANCE = "distance"
    TIME_AND_DISTANCE = "time_and_distance"


class Direction(Enum):
    """Direction for acceptance evaluation."""

    LONG = "long"
    SHORT = "short"


@dataclass
class AcceptanceResult:
    """Result of acceptance evaluation."""

    accepted: bool
    acceptance_type: AcceptanceType
    confidence: float
    volume_confirmed: bool
    consecutive_closes: int
    max_distance_atr: float


class AcceptanceEngine:
    """
    Evaluates whether price has accepted a level using time, distance, and volume.

    Time acceptance: N consecutive closes beyond the level.
    Distance acceptance: Price moved significant ATR distance beyond level.
    Volume confirmation: Average volume at level exceeds threshold.

    Parameters
    ----------
    config : AcceptanceConfig
        The acceptance configuration.

    """

    def __init__(self, config: AcceptanceConfig):
        self.config = config
        self._bar_history: deque = deque(maxlen=50)
        self._atr: float = 0.0
        self._avg_volume: float = 0.0

    def update(self, bar: Bar, atr: float, avg_volume: float) -> None:
        """
        Update with new bar data.

        Parameters
        ----------
        bar : Bar
            The bar to add.
        atr : float
            Current ATR value.
        avg_volume : float
            Average volume.

        """
        self._bar_history.append({
            "high": bar.high.as_double(),
            "low": bar.low.as_double(),
            "close": bar.close.as_double(),
            "open": bar.open.as_double(),
            "volume": bar.volume.as_double(),
        })
        self._atr = atr
        self._avg_volume = avg_volume

    def evaluate(
        self,
        level: float,
        direction: Direction,
        lookback_bars: int = 5,
    ) -> AcceptanceResult:
        """
        Evaluate acceptance of a price level.

        Parameters
        ----------
        level : float
            The price level to evaluate acceptance for.
        direction : Direction
            LONG (acceptance above) or SHORT (acceptance below).
        lookback_bars : int
            Number of bars to scan (default: 5).

        Returns
        -------
        AcceptanceResult
            Result with acceptance type and confidence score.

        """
        if len(self._bar_history) < lookback_bars or self._atr <= 0:
            return AcceptanceResult(
                accepted=False,
                acceptance_type=AcceptanceType.NONE,
                confidence=0.0,
                volume_confirmed=False,
                consecutive_closes=0,
                max_distance_atr=0.0,
            )

        bars = list(self._bar_history)[-lookback_bars:]

        consecutive_closes = 0
        max_distance = 0.0
        volume_at_level = 0.0

        for bar in bars:
            if direction == Direction.LONG:
                beyond_level = bar["close"] > level
                distance = (bar["close"] - level) / self._atr if beyond_level else 0.0
            else:
                beyond_level = bar["close"] < level
                distance = (level - bar["close"]) / self._atr if beyond_level else 0.0

            if beyond_level:
                consecutive_closes += 1
                max_distance = max(max_distance, distance)
                volume_at_level += bar["volume"]
            else:
                consecutive_closes = 0  # Reset on failure to hold

        # Evaluate time acceptance
        time_accepted = consecutive_closes >= self.config.time_bars

        # Evaluate distance acceptance with momentum check
        last_bar = bars[-1]
        bar_range = last_bar["high"] - last_bar["low"]
        body = abs(last_bar["close"] - last_bar["open"])
        momentum_candle = (body / bar_range) >= self.config.momentum_threshold if bar_range > 0 else False
        distance_accepted = max_distance >= self.config.distance_atr_mult and momentum_candle

        # Determine acceptance type and confidence
        if time_accepted and distance_accepted:
            acceptance_type = AcceptanceType.TIME_AND_DISTANCE
            confidence = 1.0
        elif time_accepted:
            acceptance_type = AcceptanceType.TIME
            confidence = 0.7
        elif distance_accepted:
            acceptance_type = AcceptanceType.DISTANCE
            confidence = 0.6
        else:
            acceptance_type = AcceptanceType.NONE
            confidence = 0.0

        # Volume confirmation
        if consecutive_closes > 0 and self._avg_volume > 0:
            avg_vol_at_level = volume_at_level / consecutive_closes
            volume_confirmed = avg_vol_at_level >= (self._avg_volume * self.config.volume_mult)
        else:
            volume_confirmed = False

        if acceptance_type != AcceptanceType.NONE and not volume_confirmed:
            confidence *= 0.5  # Weak acceptance without volume

        return AcceptanceResult(
            accepted=acceptance_type != AcceptanceType.NONE,
            acceptance_type=acceptance_type,
            confidence=confidence,
            volume_confirmed=volume_confirmed,
            consecutive_closes=consecutive_closes,
            max_distance_atr=max_distance,
        )

    def evaluate_breakout(
        self,
        level: float,
        direction: Direction,
    ) -> AcceptanceResult:
        """
        Evaluate a breakout for immediate acceptance signals.

        Uses stricter criteria for breakout acceptance.
        """
        result = self.evaluate(level, direction, lookback_bars=3)

        # Require volume confirmation for breakout acceptance
        if result.accepted and not result.volume_confirmed:
            return AcceptanceResult(
                accepted=False,
                acceptance_type=AcceptanceType.NONE,
                confidence=result.confidence * 0.5,
                volume_confirmed=False,
                consecutive_closes=result.consecutive_closes,
                max_distance_atr=result.max_distance_atr,
            )

        return result

    def reset(self) -> None:
        """Reset the acceptance engine state."""
        self._bar_history.clear()
        self._atr = 0.0
        self._avg_volume = 0.0
