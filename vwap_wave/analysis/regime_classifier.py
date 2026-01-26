# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Regime Classifier
# -------------------------------------------------------------------------------------------------
"""
Market regime state machine that gates setup eligibility.

Classifies market into Balance (mean reversion) or Imbalance (trend following)
regimes based on price location and acceptance confirmation.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum
from typing import TYPE_CHECKING
from typing import Optional

from nautilus_trader.model.data import Bar

from vwap_wave.analysis.acceptance import AcceptanceResult
from vwap_wave.analysis.acceptance import Direction


if TYPE_CHECKING:
    from vwap_wave.analysis.acceptance import AcceptanceEngine
    from vwap_wave.core.vwap_engine import VWAPEngine


class MarketRegime(Enum):
    """Market regime classification."""

    BALANCE = "balance"  # Price in value area, mean reversion setups
    IMBALANCE_BULLISH = "imbalance_bullish"  # Trending up, continuation setups
    IMBALANCE_BEARISH = "imbalance_bearish"  # Trending down, continuation setups
    BREAKOUT_UNCONFIRMED = "breakout_unconfirmed"  # Beyond value but not accepted


@dataclass
class RegimeState:
    """Current regime classification with metadata."""

    regime: MarketRegime
    bars_in_regime: int
    acceptance_confidence: float
    volume_confirmed: bool
    previous_regime: Optional[MarketRegime] = None


class RegimeClassifier:
    """
    Classifies market regime based on price location and acceptance.

    Balance: Price accepting inside SD1 bands (value area)
    Imbalance: Price accepted beyond SD1 bands (price discovery)
    Breakout Unconfirmed: Beyond SD1 but acceptance not yet validated

    Parameters
    ----------
    vwap_engine : VWAPEngine
        VWAP engine instance.
    acceptance_engine : AcceptanceEngine
        Acceptance engine instance.

    """

    def __init__(
        self,
        vwap_engine: VWAPEngine,
        acceptance_engine: AcceptanceEngine,
    ):
        self.vwap = vwap_engine
        self.acceptance = acceptance_engine

        # State tracking
        self._current_regime = MarketRegime.BALANCE
        self._previous_regime: Optional[MarketRegime] = None
        self._bars_in_regime: int = 0
        self._last_acceptance_result: Optional[AcceptanceResult] = None

    def update(self, bar: Bar) -> RegimeState:
        """
        Update regime classification.

        Parameters
        ----------
        bar : Bar
            The current bar.

        Returns
        -------
        RegimeState
            The current regime state.

        """
        if self.vwap.state is None:
            return RegimeState(
                regime=MarketRegime.BALANCE,
                bars_in_regime=0,
                acceptance_confidence=0.0,
                volume_confirmed=False,
            )

        close = bar.close.as_double()
        vwap_state = self.vwap.state

        above_value = close > vwap_state.sd1_upper
        below_value = close < vwap_state.sd1_lower
        inside_value = not above_value and not below_value

        previous_regime = self._current_regime
        acceptance_confidence = 0.5
        volume_confirmed = True

        if inside_value:
            self._current_regime = MarketRegime.BALANCE
            acceptance_confidence = 0.5
            volume_confirmed = True

        elif above_value:
            acceptance = self.acceptance.evaluate(vwap_state.sd1_upper, Direction.LONG)
            self._last_acceptance_result = acceptance

            if acceptance.accepted and acceptance.volume_confirmed:
                self._current_regime = MarketRegime.IMBALANCE_BULLISH
            elif acceptance.accepted:
                self._current_regime = MarketRegime.IMBALANCE_BULLISH
            else:
                self._current_regime = MarketRegime.BREAKOUT_UNCONFIRMED

            acceptance_confidence = acceptance.confidence
            volume_confirmed = acceptance.volume_confirmed

        elif below_value:
            acceptance = self.acceptance.evaluate(vwap_state.sd1_lower, Direction.SHORT)
            self._last_acceptance_result = acceptance

            if acceptance.accepted and acceptance.volume_confirmed:
                self._current_regime = MarketRegime.IMBALANCE_BEARISH
            elif acceptance.accepted:
                self._current_regime = MarketRegime.IMBALANCE_BEARISH
            else:
                self._current_regime = MarketRegime.BREAKOUT_UNCONFIRMED

            acceptance_confidence = acceptance.confidence
            volume_confirmed = acceptance.volume_confirmed

        # Track bars in current regime
        if self._current_regime == previous_regime:
            self._bars_in_regime += 1
        else:
            self._previous_regime = previous_regime
            self._bars_in_regime = 1

        return RegimeState(
            regime=self._current_regime,
            bars_in_regime=self._bars_in_regime,
            acceptance_confidence=acceptance_confidence,
            volume_confirmed=volume_confirmed,
            previous_regime=self._previous_regime,
        )

    @property
    def current_regime(self) -> MarketRegime:
        """Current market regime."""
        return self._current_regime

    @property
    def bars_in_current_regime(self) -> int:
        """Number of bars in current regime."""
        return self._bars_in_regime

    @property
    def previous_regime(self) -> Optional[MarketRegime]:
        """Previous market regime."""
        return self._previous_regime

    @property
    def last_acceptance(self) -> Optional[AcceptanceResult]:
        """Last acceptance evaluation result."""
        return self._last_acceptance_result

    def is_trending(self) -> bool:
        """Check if market is in a trending (imbalance) regime."""
        return self._current_regime in [
            MarketRegime.IMBALANCE_BULLISH,
            MarketRegime.IMBALANCE_BEARISH,
        ]

    def is_balanced(self) -> bool:
        """Check if market is in a balanced regime."""
        return self._current_regime == MarketRegime.BALANCE

    def is_bullish(self) -> bool:
        """Check if market is in bullish imbalance."""
        return self._current_regime == MarketRegime.IMBALANCE_BULLISH

    def is_bearish(self) -> bool:
        """Check if market is in bearish imbalance."""
        return self._current_regime == MarketRegime.IMBALANCE_BEARISH

    def regime_just_changed(self) -> bool:
        """Check if regime just changed (1 bar in new regime)."""
        return self._bars_in_regime == 1

    def get_trend_direction(self) -> Optional[str]:
        """Get the current trend direction if trending."""
        if self._current_regime == MarketRegime.IMBALANCE_BULLISH:
            return "LONG"
        elif self._current_regime == MarketRegime.IMBALANCE_BEARISH:
            return "SHORT"
        return None

    def reset(self) -> None:
        """Reset the regime classifier state."""
        self._current_regime = MarketRegime.BALANCE
        self._previous_regime = None
        self._bars_in_regime = 0
        self._last_acceptance_result = None
