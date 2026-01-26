# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Exhaustion Engine
# -------------------------------------------------------------------------------------------------
"""
Exhaustion detection with the two-candle lag rule.

Detects exhaustion at statistical extremes using absorption patterns,
CVD divergence, and volume confirmation with proper lag rule enforcement.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from enum import Enum
from typing import TYPE_CHECKING
from typing import Optional

from nautilus_trader.model.data import Bar

from vwap_wave.config.settings import ExhaustionConfig


if TYPE_CHECKING:
    from vwap_wave.core.cvd_calculator import CVDCalculator
    from vwap_wave.core.initial_balance import InitialBalanceTracker
    from vwap_wave.core.vwap_engine import VWAPEngine


class ExhaustionZone(Enum):
    """Zone where exhaustion is detected."""

    NONE = "none"
    SD_UPPER = "sd_upper"  # At or above SD2 upper
    SD_LOWER = "sd_lower"  # At or below SD2 lower
    IB_UPPER = "ib_upper"  # At or above IB x3 upper
    IB_LOWER = "ib_lower"  # At or below IB x3 lower


class FadeDirection(Enum):
    """Direction to fade after exhaustion."""

    NONE = "none"
    FADE_SHORT = "fade_short"  # Exhaustion at top, fade short
    FADE_LONG = "fade_long"  # Exhaustion at bottom, fade long


@dataclass
class AbsorptionCandle:
    """Detected absorption candle signature."""

    detected: bool
    bar_index: int
    volume_spike: float  # Ratio vs average volume
    price_progress: float  # ATR progress made
    direction: str  # "BULLISH_EXHAUSTION" or "BEARISH_EXHAUSTION" or ""


@dataclass
class PendingExhaustion:
    """Exhaustion signal awaiting lag confirmation."""

    zone: ExhaustionZone
    direction: FadeDirection
    absorption: AbsorptionCandle
    divergence_magnitude: float
    registered_bar_index: int


@dataclass
class ExhaustionSignal:
    """Confirmed exhaustion signal."""

    confirmed: bool
    zone: ExhaustionZone
    direction: FadeDirection
    absorption: AbsorptionCandle
    divergence_magnitude: float
    confidence: float


class ExhaustionEngine:
    """
    Detects exhaustion at statistical extremes.

    Uses absorption candle detection + CVD divergence + lag rule confirmation.
    Exhaustion is not confirmed until subsequent bars show volume dropoff
    and price rejection.

    Parameters
    ----------
    config : ExhaustionConfig
        The exhaustion configuration.
    cvd_calculator : CVDCalculator
        CVD calculator instance.
    vwap_engine : VWAPEngine
        VWAP engine instance.
    ib_tracker : InitialBalanceTracker
        Initial Balance tracker instance.

    """

    def __init__(
        self,
        config: ExhaustionConfig,
        cvd_calculator: CVDCalculator,
        vwap_engine: VWAPEngine,
        ib_tracker: InitialBalanceTracker,
    ):
        self.config = config
        self.cvd = cvd_calculator
        self.vwap = vwap_engine
        self.ib = ib_tracker

        # State tracking
        self._bar_history: deque = deque(maxlen=50)
        self._pending: Optional[PendingExhaustion] = None
        self._bar_index: int = 0
        self._atr: float = 0.0
        self._avg_volume: float = 0.0

    def update(self, bar: Bar, atr: float, avg_volume: float) -> None:
        """
        Update with new bar.

        Parameters
        ----------
        bar : Bar
            The bar to process.
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
        self._bar_index += 1
        self._atr = atr
        self._avg_volume = avg_volume

    def evaluate(self) -> ExhaustionSignal:
        """
        Evaluate for exhaustion signal.

        Returns confirmed signal only after lag rule is satisfied.

        Returns
        -------
        ExhaustionSignal
            The exhaustion detection result.

        """
        no_signal = ExhaustionSignal(
            confirmed=False,
            zone=ExhaustionZone.NONE,
            direction=FadeDirection.NONE,
            absorption=AbsorptionCandle(False, 0, 0.0, 0.0, ""),
            divergence_magnitude=0.0,
            confidence=0.0,
        )

        # Phase 1: Check exhaustion zone
        zone = self._get_exhaustion_zone()
        if zone == ExhaustionZone.NONE:
            self._pending = None
            return no_signal

        # Phase 2: Detect absorption candle
        expected_direction = (
            "BULLISH_EXHAUSTION"
            if zone in [ExhaustionZone.SD_UPPER, ExhaustionZone.IB_UPPER]
            else "BEARISH_EXHAUSTION"
        )
        absorption = self._detect_absorption(expected_direction)

        if absorption.detected:
            # Phase 3: Check CVD divergence
            if expected_direction == "BULLISH_EXHAUSTION":
                divergence = self.cvd.detect_bearish_divergence()
                fade_direction = FadeDirection.FADE_SHORT
            else:
                divergence = self.cvd.detect_bullish_divergence()
                fade_direction = FadeDirection.FADE_LONG

            if divergence.detected:
                # Register pending exhaustion (lag rule starts)
                self._pending = PendingExhaustion(
                    zone=zone,
                    direction=fade_direction,
                    absorption=absorption,
                    divergence_magnitude=divergence.divergence_magnitude,
                    registered_bar_index=self._bar_index,
                )

        # Phase 4: Check pending exhaustion confirmation
        if self._pending is not None:
            bars_since_registration = self._bar_index - self._pending.registered_bar_index

            if bars_since_registration >= self.config.confirmation_bars:
                # Verify volume dropoff
                if self._verify_volume_dropoff(self._pending.absorption.bar_index):
                    # Verify price rejection
                    if self._verify_price_rejection(self._pending.direction):
                        confidence = self._calculate_confidence(self._pending)
                        signal = ExhaustionSignal(
                            confirmed=True,
                            zone=self._pending.zone,
                            direction=self._pending.direction,
                            absorption=self._pending.absorption,
                            divergence_magnitude=self._pending.divergence_magnitude,
                            confidence=confidence,
                        )
                        self._pending = None
                        return signal

            # Invalidate if too much time passed
            if bars_since_registration > self.config.confirmation_bars + 2:
                self._pending = None

        return no_signal

    def _get_exhaustion_zone(self) -> ExhaustionZone:
        """Determine if price is in an exhaustion-eligible zone."""
        if not self._bar_history or self.vwap.state is None:
            return ExhaustionZone.NONE

        close = self._bar_history[-1]["close"]
        vwap_state = self.vwap.state

        # Check VWAP SD bands (SD2 or higher)
        if close >= vwap_state.sd2_upper:
            return ExhaustionZone.SD_UPPER
        if close <= vwap_state.sd2_lower:
            return ExhaustionZone.SD_LOWER

        # Check IB extensions (x3 or higher)
        if self.ib.state is not None and self.ib.is_complete:
            if close >= self.ib.state.x3_upper:
                return ExhaustionZone.IB_UPPER
            if close <= self.ib.state.x3_lower:
                return ExhaustionZone.IB_LOWER

        return ExhaustionZone.NONE

    def _detect_absorption(
        self,
        expected_direction: str,
        lookback: int = 5,
    ) -> AbsorptionCandle:
        """
        Detect absorption candle: high volume, minimal price progress.

        Parameters
        ----------
        expected_direction : str
            Expected exhaustion direction.
        lookback : int
            Bars to search (default: 5).

        Returns
        -------
        AbsorptionCandle
            The absorption detection result.

        """
        if len(self._bar_history) < lookback or self._atr <= 0 or self._avg_volume <= 0:
            return AbsorptionCandle(False, 0, 0.0, 0.0, "")

        bars = list(self._bar_history)[-lookback:]

        for i, bar in enumerate(bars):
            # Check volume spike
            volume_ratio = bar["volume"] / self._avg_volume
            is_spike = volume_ratio >= self.config.volume_spike_mult

            if not is_spike:
                continue

            # Calculate effort vs result
            bar_range = bar["high"] - bar["low"]
            body = abs(bar["close"] - bar["open"])
            body_ratio = body / bar_range if bar_range > 0 else 0.0

            # Price progress beyond prior bar
            if i > 0:
                prior_bar = bars[i - 1]
                if expected_direction == "BULLISH_EXHAUSTION":
                    progress = (bar["high"] - prior_bar["high"]) / self._atr
                else:
                    progress = (prior_bar["low"] - bar["low"]) / self._atr
            else:
                progress = 0.0

            # Absorption signature: high volume, small body, minimal progress
            is_absorption = (
                is_spike
                and progress <= self.config.price_progress_threshold
                and body_ratio <= self.config.absorption_body_ratio
            )

            if is_absorption:
                return AbsorptionCandle(
                    detected=True,
                    bar_index=self._bar_index - (lookback - i - 1),
                    volume_spike=volume_ratio,
                    price_progress=progress,
                    direction=expected_direction,
                )

        return AbsorptionCandle(False, 0, 0.0, 0.0, "")

    def _verify_volume_dropoff(self, absorption_bar_index: int) -> bool:
        """Verify volume has dropped off since absorption."""
        bars_since = self._bar_index - absorption_bar_index
        if bars_since < self.config.confirmation_bars:
            return False

        recent_bars = list(self._bar_history)[-self.config.confirmation_bars :]
        if not recent_bars:
            return False

        recent_avg_vol = sum(b["volume"] for b in recent_bars) / len(recent_bars)

        # Get absorption bar volume
        absorption_offset = len(self._bar_history) - (self._bar_index - absorption_bar_index)
        if absorption_offset < 0 or absorption_offset >= len(self._bar_history):
            return False

        absorption_vol = list(self._bar_history)[absorption_offset]["volume"]

        return recent_avg_vol < (absorption_vol * self.config.volume_dropoff_threshold)

    def _verify_price_rejection(self, direction: FadeDirection) -> bool:
        """Verify price is rejecting the extreme."""
        if len(self._bar_history) < 2:
            return False

        current = self._bar_history[-1]
        recent_bars = list(self._bar_history)[-5:]

        if direction == FadeDirection.FADE_SHORT:
            # Price should be below recent high
            recent_high = max(b["high"] for b in recent_bars)
            return current["close"] < recent_high
        else:
            # Price should be above recent low
            recent_low = min(b["low"] for b in recent_bars)
            return current["close"] > recent_low

    def _calculate_confidence(self, pending: PendingExhaustion) -> float:
        """Calculate confidence score for exhaustion signal."""
        confidence = 0.5

        # Zone quality (IB zones slightly more reliable)
        if pending.zone in [ExhaustionZone.IB_UPPER, ExhaustionZone.IB_LOWER]:
            confidence += 0.15

        # Absorption quality
        if pending.absorption.volume_spike >= 3.0:
            confidence += 0.1
        if pending.absorption.price_progress <= 0.1:
            confidence += 0.1

        # Divergence quality
        if pending.divergence_magnitude >= 0.2:
            confidence += 0.15

        return min(confidence, 1.0)

    def has_pending_signal(self) -> bool:
        """Check if there's a pending exhaustion signal."""
        return self._pending is not None

    def get_pending_direction(self) -> Optional[FadeDirection]:
        """Get the direction of the pending signal."""
        if self._pending is None:
            return None
        return self._pending.direction

    def reset(self) -> None:
        """Reset the exhaustion engine state."""
        self._bar_history.clear()
        self._pending = None
        self._bar_index = 0
        self._atr = 0.0
        self._avg_volume = 0.0
