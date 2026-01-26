# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Return to Value Setup
# -------------------------------------------------------------------------------------------------
"""
Return to Value setup.

Tracks failed breakouts using rejection detection. Maintains pending state
for "look above and fail" scenarios. Enters on re-entry backtest to value
area edge.
"""

from __future__ import annotations

from typing import TYPE_CHECKING
from typing import Optional

from nautilus_trader.model.data import Bar

from vwap_wave.analysis.regime_classifier import MarketRegime
from vwap_wave.analysis.regime_classifier import RegimeState
from vwap_wave.analysis.rejection import RejectionType
from vwap_wave.setups.base_setup import BaseSetup
from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection


if TYPE_CHECKING:
    from vwap_wave.analysis.acceptance import AcceptanceEngine
    from vwap_wave.analysis.rejection import RejectionEngine
    from vwap_wave.config.settings import VWAPWaveConfig
    from vwap_wave.core.vwap_engine import VWAPEngine


class ReturnToValueSetup(BaseSetup):
    """
    Return to Value setup.

    Trades failed breakouts that return to the value area.
    "Look above and fail" or "look below and fail" patterns.

    Parameters
    ----------
    config : VWAPWaveConfig
        The master configuration.
    vwap_engine : VWAPEngine
        VWAP engine instance.
    acceptance_engine : AcceptanceEngine
        Acceptance engine instance.
    rejection_engine : RejectionEngine
        Rejection engine instance.

    """

    # Minimum rejection strength to consider
    MIN_REJECTION_STRENGTH = 0.5

    # Maximum bars since rejection to still consider entry
    MAX_BARS_SINCE_REJECTION = 5

    def __init__(
        self,
        config: VWAPWaveConfig,
        vwap_engine: VWAPEngine,
        acceptance_engine: AcceptanceEngine,
        rejection_engine: Optional[RejectionEngine] = None,
    ):
        super().__init__("RETURN_TO_VALUE", config)
        self.vwap = vwap_engine
        self.acceptance = acceptance_engine
        self.rejection = rejection_engine

        # Internal state for tracking failed breakouts
        self._pending_rejection: Optional[dict] = None
        self._bars_since_rejection: int = 0

    def is_eligible(self, regime_state: RegimeState) -> bool:
        """
        Eligible when breakout unconfirmed or transitioning back to balance.

        Parameters
        ----------
        regime_state : RegimeState
            Current market regime.

        Returns
        -------
        bool
            True if eligible for return to value trades.

        """
        # Eligible if there's a pending rejection
        if self._pending_rejection is not None:
            return True

        # Eligible on breakout that's not confirming
        if regime_state.regime == MarketRegime.BREAKOUT_UNCONFIRMED:
            return True

        # Eligible if regime just changed from imbalance to balance
        if (
            regime_state.regime == MarketRegime.BALANCE
            and regime_state.previous_regime
            in [
                MarketRegime.IMBALANCE_BULLISH,
                MarketRegime.IMBALANCE_BEARISH,
                MarketRegime.BREAKOUT_UNCONFIRMED,
            ]
            and regime_state.bars_in_regime <= 3
        ):
            return True

        return False

    def evaluate(self, regime_state: RegimeState, bar: Bar, atr: float) -> SetupSignal:
        """
        Evaluate for return to value entry.

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
        # Track bars since any pending rejection
        if self._pending_rejection is not None:
            self._bars_since_rejection += 1

            # Expire old rejections
            if self._bars_since_rejection > self.MAX_BARS_SINCE_REJECTION:
                self._pending_rejection = None
                self._bars_since_rejection = 0

        if not self.is_eligible(regime_state):
            return SetupSignal.no_signal()

        if self.vwap.state is None:
            return SetupSignal.no_signal()

        if atr == 0:
            return SetupSignal.no_signal()

        vwap_state = self.vwap.state
        close = bar.close.as_double()
        high = bar.high.as_double()
        low = bar.low.as_double()
        open_price = bar.open.as_double()

        # Check for new rejection signal if we have rejection engine
        if self.rejection is not None:
            rejection = self.rejection.evaluate()
            if rejection.detected and rejection.rejection_strength >= self.MIN_REJECTION_STRENGTH:
                self._pending_rejection = {
                    "type": rejection.rejection_type,
                    "breakout_level": rejection.breakout_level,
                    "rejection_level": rejection.rejection_level,
                    "strength": rejection.rejection_strength,
                }
                self._bars_since_rejection = 0

        # Use regime change as implicit rejection detection if no rejection engine
        if self.rejection is None and regime_state.regime == MarketRegime.BALANCE:
            if regime_state.previous_regime == MarketRegime.IMBALANCE_BULLISH:
                self._pending_rejection = {
                    "type": RejectionType.LOOK_ABOVE_FAIL,
                    "breakout_level": vwap_state.sd1_upper,
                    "rejection_level": close,
                    "strength": 0.6,
                }
                self._bars_since_rejection = 0
            elif regime_state.previous_regime == MarketRegime.IMBALANCE_BEARISH:
                self._pending_rejection = {
                    "type": RejectionType.LOOK_BELOW_FAIL,
                    "breakout_level": vwap_state.sd1_lower,
                    "rejection_level": close,
                    "strength": 0.6,
                }
                self._bars_since_rejection = 0

        # If we have a pending rejection, look for entry
        if self._pending_rejection is not None:
            return self._evaluate_entry(
                bar,
                atr,
                vwap_state,
                close,
                high,
                low,
                open_price,
            )

        return SetupSignal.no_signal()

    def _evaluate_entry(
        self,
        bar: Bar,
        atr: float,
        vwap_state,
        close: float,
        high: float,
        low: float,
        open_price: float,
    ) -> SetupSignal:
        """Evaluate entry conditions for pending rejection."""
        rejection = self._pending_rejection
        rejection_type = rejection["type"]

        if rejection_type in [RejectionType.LOOK_ABOVE_FAIL, RejectionType.TRAP_LONG]:
            # Failed upside breakout - look for short entry
            # Price should be back inside value area and showing bearish momentum

            if close > vwap_state.sd1_upper:
                return SetupSignal.no_signal()  # Not yet back in value

            # Confirm bearish momentum
            if close >= open_price:
                return SetupSignal.no_signal()  # Not bearish

            entry = close
            stop = high + (atr * 0.5)

            # Target VWAP or SD1 lower
            target = vwap_state.vwap

            # Adjust confidence based on rejection strength
            confidence = min(rejection["strength"] + 0.2, 0.9)

            # Clear pending rejection after taking trade
            self._pending_rejection = None
            self._bars_since_rejection = 0

            return SetupSignal(
                valid=True,
                setup_type=self.name,
                direction=TradeDirection.SHORT,
                entry_price=entry,
                stop_price=stop,
                target_price=target,
                confidence=confidence,
                metadata={
                    "rejection_type": rejection_type.value,
                    "breakout_level": rejection["breakout_level"],
                    "rejection_strength": rejection["strength"],
                    "bars_since_rejection": self._bars_since_rejection,
                },
            )

        elif rejection_type in [RejectionType.LOOK_BELOW_FAIL, RejectionType.TRAP_SHORT]:
            # Failed downside breakout - look for long entry

            if close < vwap_state.sd1_lower:
                return SetupSignal.no_signal()  # Not yet back in value

            # Confirm bullish momentum
            if close <= open_price:
                return SetupSignal.no_signal()  # Not bullish

            entry = close
            stop = low - (atr * 0.5)

            # Target VWAP or SD1 upper
            target = vwap_state.vwap

            confidence = min(rejection["strength"] + 0.2, 0.9)

            self._pending_rejection = None
            self._bars_since_rejection = 0

            return SetupSignal(
                valid=True,
                setup_type=self.name,
                direction=TradeDirection.LONG,
                entry_price=entry,
                stop_price=stop,
                target_price=target,
                confidence=confidence,
                metadata={
                    "rejection_type": rejection_type.value,
                    "breakout_level": rejection["breakout_level"],
                    "rejection_strength": rejection["strength"],
                    "bars_since_rejection": self._bars_since_rejection,
                },
            )

        return SetupSignal.no_signal()

    def reset(self) -> None:
        """Reset internal state."""
        self._pending_rejection = None
        self._bars_since_rejection = 0
