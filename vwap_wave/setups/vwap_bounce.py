# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - VWAP Bounce Setup
# -------------------------------------------------------------------------------------------------
"""
VWAP Bounce setup.

Requires confirmed imbalance regime with minimum duration.
Enters on VWAP touch with strength resuming. Rejects if CVD shows divergence.
"""

from __future__ import annotations

from typing import TYPE_CHECKING

from nautilus_trader.model.data import Bar

from vwap_wave.analysis.regime_classifier import MarketRegime
from vwap_wave.analysis.regime_classifier import RegimeState
from vwap_wave.setups.base_setup import BaseSetup
from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection


if TYPE_CHECKING:
    from vwap_wave.analysis.regime_classifier import RegimeClassifier
    from vwap_wave.config.settings import VWAPWaveConfig
    from vwap_wave.core.cvd_calculator import CVDCalculator
    from vwap_wave.core.vwap_engine import VWAPEngine


class VWAPBounceSetup(BaseSetup):
    """
    VWAP Bounce setup.

    Trades bounces off VWAP in trending (imbalance) regimes.
    Requires CVD confirmation of trend continuation.

    Parameters
    ----------
    config : VWAPWaveConfig
        The master configuration.
    vwap_engine : VWAPEngine
        VWAP engine instance.
    cvd_calculator : CVDCalculator
        CVD calculator instance.
    regime_classifier : RegimeClassifier
        Regime classifier instance.

    """

    # Minimum bars in trend before taking VWAP bounce trades
    MIN_BARS_IN_TREND = 5

    # Tolerance for VWAP touch (in ATR)
    VWAP_TOUCH_TOLERANCE_ATR = 0.3

    # Minimum body ratio for confirmation candle
    MIN_BODY_RATIO = 0.5

    def __init__(
        self,
        config: VWAPWaveConfig,
        vwap_engine: VWAPEngine,
        cvd_calculator: CVDCalculator,
        regime_classifier: RegimeClassifier,
    ):
        super().__init__("VWAP_BOUNCE", config)
        self.vwap = vwap_engine
        self.cvd = cvd_calculator
        self.regime = regime_classifier

    def is_eligible(self, regime_state: RegimeState) -> bool:
        """
        Eligible only in confirmed imbalance with sufficient duration.

        Parameters
        ----------
        regime_state : RegimeState
            Current market regime.

        Returns
        -------
        bool
            True if eligible for VWAP bounce trades.

        """
        return (
            regime_state.regime
            in [
                MarketRegime.IMBALANCE_BULLISH,
                MarketRegime.IMBALANCE_BEARISH,
            ]
            and regime_state.bars_in_regime >= self.MIN_BARS_IN_TREND
            and regime_state.volume_confirmed
        )

    def evaluate(self, regime_state: RegimeState, bar: Bar, atr: float) -> SetupSignal:
        """
        Evaluate for VWAP bounce entry.

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

        if self.vwap.state is None:
            return SetupSignal.no_signal()

        if atr == 0:
            return SetupSignal.no_signal()

        vwap_value = self.vwap.state.vwap
        close = bar.close.as_double()
        high = bar.high.as_double()
        low = bar.low.as_double()
        open_price = bar.open.as_double()

        # Check VWAP touch
        if not self._is_vwap_touch(bar, vwap_value, atr):
            return SetupSignal.no_signal()

        # Evaluate based on regime direction
        if regime_state.regime == MarketRegime.IMBALANCE_BULLISH:
            return self._evaluate_bullish_bounce(
                regime_state,
                bar,
                atr,
                vwap_value,
                close,
                high,
                low,
                open_price,
            )
        else:
            return self._evaluate_bearish_bounce(
                regime_state,
                bar,
                atr,
                vwap_value,
                close,
                high,
                low,
                open_price,
            )

    def _is_vwap_touch(self, bar: Bar, vwap: float, atr: float) -> bool:
        """Check if bar touched VWAP."""
        low = bar.low.as_double()
        high = bar.high.as_double()

        # Bar's range should cross VWAP
        tolerance = atr * self.VWAP_TOUCH_TOLERANCE_ATR

        # For bullish bounce: low touched VWAP
        if low <= vwap + tolerance and high >= vwap:
            return True

        # For bearish bounce: high touched VWAP
        if high >= vwap - tolerance and low <= vwap:
            return True

        return False

    def _evaluate_bullish_bounce(
        self,
        regime_state: RegimeState,
        bar: Bar,
        atr: float,
        vwap: float,
        close: float,
        high: float,
        low: float,
        open_price: float,
    ) -> SetupSignal:
        """Evaluate bullish bounce setup."""
        # Low should have touched VWAP and price bounced up
        if low > vwap:
            return SetupSignal.no_signal()

        # Must be a bullish bar
        if close <= open_price:
            return SetupSignal.no_signal()

        # Check body ratio for strength
        bar_range = high - low
        body = close - open_price
        if bar_range > 0 and (body / bar_range) < self.MIN_BODY_RATIO:
            return SetupSignal.no_signal()

        # Check CVD - should not show bearish divergence
        bearish_div = self.cvd.detect_bearish_divergence()
        if bearish_div.detected:
            return SetupSignal.no_signal()

        # CVD should be rising or neutral
        if self.cvd.is_cvd_falling:
            return SetupSignal.no_signal()

        # Build signal
        entry = close
        stop = low - (atr * 0.25)
        target = self.vwap.state.sd1_upper

        confidence = min(regime_state.acceptance_confidence + 0.1, 0.95)

        return SetupSignal(
            valid=True,
            setup_type=self.name,
            direction=TradeDirection.LONG,
            entry_price=entry,
            stop_price=stop,
            target_price=target,
            confidence=confidence,
            metadata={
                "vwap": vwap,
                "bounce_from_low": low,
                "bars_in_trend": regime_state.bars_in_regime,
                "cvd_trend": self.cvd.cvd_trend,
            },
        )

    def _evaluate_bearish_bounce(
        self,
        regime_state: RegimeState,
        bar: Bar,
        atr: float,
        vwap: float,
        close: float,
        high: float,
        low: float,
        open_price: float,
    ) -> SetupSignal:
        """Evaluate bearish bounce setup."""
        # High should have touched VWAP and price bounced down
        if high < vwap:
            return SetupSignal.no_signal()

        # Must be a bearish bar
        if close >= open_price:
            return SetupSignal.no_signal()

        # Check body ratio for strength
        bar_range = high - low
        body = open_price - close
        if bar_range > 0 and (body / bar_range) < self.MIN_BODY_RATIO:
            return SetupSignal.no_signal()

        # Check CVD - should not show bullish divergence
        bullish_div = self.cvd.detect_bullish_divergence()
        if bullish_div.detected:
            return SetupSignal.no_signal()

        # CVD should be falling or neutral
        if self.cvd.is_cvd_rising:
            return SetupSignal.no_signal()

        # Build signal
        entry = close
        stop = high + (atr * 0.25)
        target = self.vwap.state.sd1_lower

        confidence = min(regime_state.acceptance_confidence + 0.1, 0.95)

        return SetupSignal(
            valid=True,
            setup_type=self.name,
            direction=TradeDirection.SHORT,
            entry_price=entry,
            stop_price=stop,
            target_price=target,
            confidence=confidence,
            metadata={
                "vwap": vwap,
                "bounce_from_high": high,
                "bars_in_trend": regime_state.bars_in_regime,
                "cvd_trend": self.cvd.cvd_trend,
            },
        )
