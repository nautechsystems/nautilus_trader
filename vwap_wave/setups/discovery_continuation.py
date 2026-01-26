# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Price Discovery Continuation Setup
# -------------------------------------------------------------------------------------------------
"""
Price Discovery Continuation setup.

Trades pullbacks in confirmed imbalance (trending) regimes.
Enters on strength resuming after pullback to SD1 band or IB level.
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
    from vwap_wave.analysis.acceptance import AcceptanceEngine
    from vwap_wave.config.settings import VWAPWaveConfig
    from vwap_wave.core.initial_balance import InitialBalanceTracker
    from vwap_wave.core.vwap_engine import VWAPEngine


class DiscoveryContinuationSetup(BaseSetup):
    """
    Price Discovery Continuation setup.

    Trades pullbacks in confirmed imbalance (trending) regimes.
    Enters on strength resuming after pullback to SD1 band or IB level.

    Parameters
    ----------
    config : VWAPWaveConfig
        The master configuration.
    vwap_engine : VWAPEngine
        VWAP engine instance.
    ib_tracker : InitialBalanceTracker
        Initial Balance tracker instance.
    acceptance_engine : AcceptanceEngine
        Acceptance engine instance.

    """

    # Pullback depth tolerance in ATR
    PULLBACK_TOLERANCE_ATR = 0.3

    # Minimum bars in trend before taking continuation trades
    MIN_BARS_IN_TREND = 3

    def __init__(
        self,
        config: VWAPWaveConfig,
        vwap_engine: VWAPEngine,
        ib_tracker: InitialBalanceTracker,
        acceptance_engine: AcceptanceEngine,
    ):
        super().__init__("DISCOVERY_CONTINUATION", config)
        self.vwap = vwap_engine
        self.ib = ib_tracker
        self.acceptance = acceptance_engine

    def is_eligible(self, regime_state: RegimeState) -> bool:
        """
        Eligible only in confirmed imbalance regimes.

        Parameters
        ----------
        regime_state : RegimeState
            Current market regime.

        Returns
        -------
        bool
            True if in imbalance regime with sufficient duration.

        """
        return (
            regime_state.regime
            in [
                MarketRegime.IMBALANCE_BULLISH,
                MarketRegime.IMBALANCE_BEARISH,
            ]
            and regime_state.bars_in_regime >= self.MIN_BARS_IN_TREND
        )

    def evaluate(self, regime_state: RegimeState, bar: Bar, atr: float) -> SetupSignal:
        """
        Evaluate for continuation entry on pullback.

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

        vwap_state = self.vwap.state

        if regime_state.regime == MarketRegime.IMBALANCE_BULLISH:
            return self._evaluate_bullish(regime_state, bar, atr, vwap_state)
        else:
            return self._evaluate_bearish(regime_state, bar, atr, vwap_state)

    def _evaluate_bullish(
        self,
        regime_state: RegimeState,
        bar: Bar,
        atr: float,
        vwap_state,
    ) -> SetupSignal:
        """Evaluate bullish continuation setup."""
        close = bar.close.as_double()
        low = bar.low.as_double()
        open_price = bar.open.as_double()

        # Determine pullback target level
        pullback_target = vwap_state.sd1_upper
        if self.ib.state is not None and self.ib.is_complete:
            pullback_target = max(pullback_target, self.ib.state.ib_high)

        if atr == 0:
            return SetupSignal.no_signal()

        # Check if price has pulled back to target
        distance_to_target = (close - pullback_target) / atr

        if distance_to_target > self.PULLBACK_TOLERANCE_ATR:
            return SetupSignal.no_signal()  # Not pulled back enough

        if distance_to_target < -0.1:
            return SetupSignal.no_signal()  # Pulled back too far (through level)

        # Confirm strength resuming (bullish candle)
        if close <= open_price:
            return SetupSignal.no_signal()

        # Build signal
        entry = close
        stop = low - (atr * 0.25)

        # Target based on acceptance confidence
        if regime_state.acceptance_confidence >= 0.8:
            target = vwap_state.sd2_upper
        else:
            target = vwap_state.sd1_upper + (atr * 1.5)

        return SetupSignal(
            valid=True,
            setup_type=self.name,
            direction=TradeDirection.LONG,
            entry_price=entry,
            stop_price=stop,
            target_price=target,
            confidence=regime_state.acceptance_confidence,
            metadata={
                "pullback_depth": distance_to_target,
                "bars_in_trend": regime_state.bars_in_regime,
                "pullback_target": pullback_target,
            },
        )

    def _evaluate_bearish(
        self,
        regime_state: RegimeState,
        bar: Bar,
        atr: float,
        vwap_state,
    ) -> SetupSignal:
        """Evaluate bearish continuation setup."""
        close = bar.close.as_double()
        high = bar.high.as_double()
        open_price = bar.open.as_double()

        # Determine pullback target level
        pullback_target = vwap_state.sd1_lower
        if self.ib.state is not None and self.ib.is_complete:
            pullback_target = min(pullback_target, self.ib.state.ib_low)

        if atr == 0:
            return SetupSignal.no_signal()

        # Check if price has pulled back to target
        distance_to_target = (pullback_target - close) / atr

        if distance_to_target > self.PULLBACK_TOLERANCE_ATR:
            return SetupSignal.no_signal()  # Not pulled back enough

        if distance_to_target < -0.1:
            return SetupSignal.no_signal()  # Pulled back too far

        # Confirm strength resuming (bearish candle)
        if close >= open_price:
            return SetupSignal.no_signal()

        # Build signal
        entry = close
        stop = high + (atr * 0.25)

        # Target based on acceptance confidence
        if regime_state.acceptance_confidence >= 0.8:
            target = vwap_state.sd2_lower
        else:
            target = vwap_state.sd1_lower - (atr * 1.5)

        return SetupSignal(
            valid=True,
            setup_type=self.name,
            direction=TradeDirection.SHORT,
            entry_price=entry,
            stop_price=stop,
            target_price=target,
            confidence=regime_state.acceptance_confidence,
            metadata={
                "pullback_depth": distance_to_target,
                "bars_in_trend": regime_state.bars_in_regime,
                "pullback_target": pullback_target,
            },
        )
