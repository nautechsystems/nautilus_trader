# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Rejection Engine
# -------------------------------------------------------------------------------------------------
"""
Rejection and trap detection for failed breakouts.

Identifies "look above and fail" and "look below and fail" patterns that
indicate potential reversal opportunities.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from enum import Enum
from typing import Optional

from nautilus_trader.model.data import Bar


class RejectionType(Enum):
    """Type of rejection detected."""

    NONE = "none"
    LOOK_ABOVE_FAIL = "look_above_fail"  # Failed upside breakout
    LOOK_BELOW_FAIL = "look_below_fail"  # Failed downside breakout
    TRAP_LONG = "trap_long"  # Long trap (false bullish breakout)
    TRAP_SHORT = "trap_short"  # Short trap (false bearish breakout)


@dataclass
class RejectionResult:
    """Result of rejection evaluation."""

    detected: bool
    rejection_type: RejectionType
    breakout_level: float
    rejection_level: float
    rejection_strength: float  # 0-1 based on how far price retreated
    bars_since_breakout: int
    volume_on_rejection: float


class RejectionEngine:
    """
    Detects rejection patterns and failed breakouts.

    Tracks when price breaks a level but fails to hold, retreating
    back through the breakout level. These patterns often precede
    strong moves in the opposite direction.

    Parameters
    ----------
    max_breakout_bars : int
        Maximum bars to consider breakout still valid (default: 10).
    rejection_threshold : float
        ATR multiple for rejection confirmation (default: 0.5).

    """

    def __init__(
        self,
        max_breakout_bars: int = 10,
        rejection_threshold: float = 0.5,
    ):
        self._max_breakout_bars = max_breakout_bars
        self._rejection_threshold = rejection_threshold

        # State tracking
        self._bar_history: deque = deque(maxlen=50)
        self._pending_breakout: Optional[dict] = None
        self._bar_count: int = 0
        self._atr: float = 0.0

    def update(self, bar: Bar, atr: float) -> None:
        """
        Update with new bar data.

        Parameters
        ----------
        bar : Bar
            The bar to add.
        atr : float
            Current ATR value.

        """
        self._bar_history.append({
            "high": bar.high.as_double(),
            "low": bar.low.as_double(),
            "close": bar.close.as_double(),
            "open": bar.open.as_double(),
            "volume": bar.volume.as_double(),
        })
        self._bar_count += 1
        self._atr = atr

    def register_breakout(
        self,
        level: float,
        direction: str,  # "LONG" or "SHORT"
    ) -> None:
        """
        Register a potential breakout to track for rejection.

        Parameters
        ----------
        level : float
            The level that was broken.
        direction : str
            Direction of the breakout ("LONG" for upside, "SHORT" for downside).

        """
        self._pending_breakout = {
            "level": level,
            "direction": direction,
            "bar_index": self._bar_count,
            "extreme_reached": level,  # Will be updated as price extends
        }

    def evaluate(self) -> RejectionResult:
        """
        Evaluate for rejection pattern.

        Returns
        -------
        RejectionResult
            The rejection detection result.

        """
        if self._pending_breakout is None or len(self._bar_history) < 2:
            return RejectionResult(
                detected=False,
                rejection_type=RejectionType.NONE,
                breakout_level=0.0,
                rejection_level=0.0,
                rejection_strength=0.0,
                bars_since_breakout=0,
                volume_on_rejection=0.0,
            )

        breakout = self._pending_breakout
        bars_since = self._bar_count - breakout["bar_index"]

        # Check if breakout is too old
        if bars_since > self._max_breakout_bars:
            self._pending_breakout = None
            return RejectionResult(
                detected=False,
                rejection_type=RejectionType.NONE,
                breakout_level=0.0,
                rejection_level=0.0,
                rejection_strength=0.0,
                bars_since_breakout=bars_since,
                volume_on_rejection=0.0,
            )

        current_bar = self._bar_history[-1]
        level = breakout["level"]

        if breakout["direction"] == "LONG":
            # Update extreme reached
            breakout["extreme_reached"] = max(
                breakout["extreme_reached"],
                current_bar["high"],
            )

            # Check for look above and fail
            if current_bar["close"] < level:
                extension = breakout["extreme_reached"] - level
                retreat = breakout["extreme_reached"] - current_bar["close"]

                if self._atr > 0:
                    rejection_strength = min(retreat / (extension + self._atr * 0.1), 1.0)
                else:
                    rejection_strength = 0.5

                # Determine if it's a trap (quick reversal with volume)
                recent_volume = sum(b["volume"] for b in list(self._bar_history)[-3:]) / 3
                is_trap = bars_since <= 3 and rejection_strength > 0.7

                self._pending_breakout = None

                return RejectionResult(
                    detected=True,
                    rejection_type=RejectionType.TRAP_LONG if is_trap else RejectionType.LOOK_ABOVE_FAIL,
                    breakout_level=level,
                    rejection_level=current_bar["close"],
                    rejection_strength=rejection_strength,
                    bars_since_breakout=bars_since,
                    volume_on_rejection=recent_volume,
                )

        else:  # SHORT direction
            # Update extreme reached
            breakout["extreme_reached"] = min(
                breakout["extreme_reached"],
                current_bar["low"],
            )

            # Check for look below and fail
            if current_bar["close"] > level:
                extension = level - breakout["extreme_reached"]
                retreat = current_bar["close"] - breakout["extreme_reached"]

                if self._atr > 0:
                    rejection_strength = min(retreat / (extension + self._atr * 0.1), 1.0)
                else:
                    rejection_strength = 0.5

                recent_volume = sum(b["volume"] for b in list(self._bar_history)[-3:]) / 3
                is_trap = bars_since <= 3 and rejection_strength > 0.7

                self._pending_breakout = None

                return RejectionResult(
                    detected=True,
                    rejection_type=RejectionType.TRAP_SHORT if is_trap else RejectionType.LOOK_BELOW_FAIL,
                    breakout_level=level,
                    rejection_level=current_bar["close"],
                    rejection_strength=rejection_strength,
                    bars_since_breakout=bars_since,
                    volume_on_rejection=recent_volume,
                )

        return RejectionResult(
            detected=False,
            rejection_type=RejectionType.NONE,
            breakout_level=level,
            rejection_level=0.0,
            rejection_strength=0.0,
            bars_since_breakout=bars_since,
            volume_on_rejection=0.0,
        )

    def has_pending_breakout(self) -> bool:
        """Check if there's a pending breakout being tracked."""
        return self._pending_breakout is not None

    def get_pending_direction(self) -> Optional[str]:
        """Get the direction of the pending breakout."""
        if self._pending_breakout is None:
            return None
        return self._pending_breakout["direction"]

    def clear_pending(self) -> None:
        """Clear any pending breakout tracking."""
        self._pending_breakout = None

    def reset(self) -> None:
        """Reset the rejection engine state."""
        self._bar_history.clear()
        self._pending_breakout = None
        self._bar_count = 0
        self._atr = 0.0
