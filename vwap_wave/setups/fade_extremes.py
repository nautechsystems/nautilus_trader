# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Fade Value Area Extremes Setup
# -------------------------------------------------------------------------------------------------
"""
Fade Value Area Extremes setup.

Uses the ExhaustionEngine as primary trigger. Eligible in Balance regime
or at statistical extremes. Targets mean reversion to VWAP or POC.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from nautilus_trader.model.data import Bar

from vwap_wave.analysis.exhaustion import FadeDirection
from vwap_wave.analysis.regime_classifier import MarketRegime
from vwap_wave.analysis.regime_classifier import RegimeState
from vwap_wave.setups.base_setup import BaseSetup
from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection


if TYPE_CHECKING:
    from vwap_wave.analysis.exhaustion import ExhaustionEngine
    from vwap_wave.config.settings import VWAPWaveConfig
    from vwap_wave.core.volume_profile import VolumeProfileBuilder
    from vwap_wave.core.vwap_engine import VWAPEngine


class FadeExtremesSetup(BaseSetup):
    """
    Fade Value Area Extremes setup.

    Uses exhaustion signals to fade moves at statistical extremes.
    Targets mean reversion to VWAP or Point of Control (POC).

    Parameters
    ----------
    config : VWAPWaveConfig
        The master configuration.
    exhaustion_engine : ExhaustionEngine
        Exhaustion engine instance.
    vwap_engine : VWAPEngine
        VWAP engine instance.
    volume_profile : VolumeProfileBuilder
        Volume profile builder instance.

    """

    # Minimum confidence for taking fade trades
    MIN_EXHAUSTION_CONFIDENCE = 0.6

    def __init__(
        self,
        config: VWAPWaveConfig,
        exhaustion_engine: ExhaustionEngine,
        vwap_engine: VWAPEngine,
        volume_profile: VolumeProfileBuilder,
    ):
        super().__init__("FADE_EXTREMES", config)
        self.exhaustion = exhaustion_engine
        self.vwap = vwap_engine
        self.volume_profile = volume_profile

    def is_eligible(self, regime_state: RegimeState) -> bool:
        """
        Eligible in Balance regime or when exhaustion signal pending.

        Parameters
        ----------
        regime_state : RegimeState
            Current market regime.

        Returns
        -------
        bool
            True if eligible for fade trades.

        """
        # Always eligible if exhaustion is pending (overrides regime)
        if self.exhaustion.has_pending_signal():
            return True

        # Otherwise, prefer balance regime for mean reversion
        return regime_state.regime == MarketRegime.BALANCE

    def evaluate(self, regime_state: RegimeState, bar: Bar, atr: float) -> SetupSignal:
        """
        Evaluate for fade entry on exhaustion.

        Parameters
        ----------
        regime_state : RegimeState
            Current market regime.
        bar : Bar
            Current bar data.
        atr : float
            Current ATR value.

        Returns
        -------
        SetupSignal
            The trade signal if conditions met.

        """
        if not self.is_eligible(regime_state):
            return SetupSignal.no_signal()

        # Get exhaustion signal
        exhaustion_signal = self.exhaustion.evaluate()

        if not exhaustion_signal.confirmed:
            return SetupSignal.no_signal()

        if exhaustion_signal.confidence < self.MIN_EXHAUSTION_CONFIDENCE:
            return SetupSignal.no_signal()

        if self.vwap.state is None:
            return SetupSignal.no_signal()

        vwap_state = self.vwap.state
        close = bar.close.as_double()
        high = bar.high.as_double()
        low = bar.low.as_double()

        if atr == 0:
            return SetupSignal.no_signal()

        # Determine trade direction and levels
        if exhaustion_signal.direction == FadeDirection.FADE_SHORT:
            return self._build_short_signal(
                bar,
                atr,
                vwap_state,
                exhaustion_signal,
                close,
                high,
                low,
            )
        elif exhaustion_signal.direction == FadeDirection.FADE_LONG:
            return self._build_long_signal(
                bar,
                atr,
                vwap_state,
                exhaustion_signal,
                close,
                high,
                low,
            )

        return SetupSignal.no_signal()

    def _build_short_signal(
        self,
        bar: Bar,
        atr: float,
        vwap_state,
        exhaustion_signal,
        close: float,
        high: float,
        low: float,
    ) -> SetupSignal:
        """Build short signal for fading upside exhaustion."""
        entry = close
        stop = high + (atr * 0.5)  # Stop above recent high

        # Target: VWAP or POC, whichever is closer
        vwap_target = vwap_state.vwap
        poc_target = self.volume_profile.poc if self.volume_profile.state else vwap_target

        # Use the closer target for conservative approach
        target = max(vwap_target, poc_target)  # Higher value = closer to current price

        # Ensure minimum reward
        min_target = close - (atr * 1.5)
        target = max(target, min_target)

        return SetupSignal(
            valid=True,
            setup_type=self.name,
            direction=TradeDirection.SHORT,
            entry_price=entry,
            stop_price=stop,
            target_price=target,
            confidence=exhaustion_signal.confidence,
            metadata={
                "exhaustion_zone": exhaustion_signal.zone.value,
                "absorption_volume_spike": exhaustion_signal.absorption.volume_spike,
                "divergence_magnitude": exhaustion_signal.divergence_magnitude,
                "vwap_target": vwap_target,
                "poc_target": poc_target,
            },
        )

    def _build_long_signal(
        self,
        bar: Bar,
        atr: float,
        vwap_state,
        exhaustion_signal,
        close: float,
        high: float,
        low: float,
    ) -> SetupSignal:
        """Build long signal for fading downside exhaustion."""
        entry = close
        stop = low - (atr * 0.5)  # Stop below recent low

        # Target: VWAP or POC, whichever is closer
        vwap_target = vwap_state.vwap
        poc_target = self.volume_profile.poc if self.volume_profile.state else vwap_target

        # Use the closer target for conservative approach
        target = min(vwap_target, poc_target)  # Lower value = closer to current price

        # Ensure minimum reward
        max_target = close + (atr * 1.5)
        target = min(target, max_target)

        return SetupSignal(
            valid=True,
            setup_type=self.name,
            direction=TradeDirection.LONG,
            entry_price=entry,
            stop_price=stop,
            target_price=target,
            confidence=exhaustion_signal.confidence,
            metadata={
                "exhaustion_zone": exhaustion_signal.zone.value,
                "absorption_volume_spike": exhaustion_signal.absorption.volume_spike,
                "divergence_magnitude": exhaustion_signal.divergence_magnitude,
                "vwap_target": vwap_target,
                "poc_target": poc_target,
            },
        )
