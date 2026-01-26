# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Position Sizer
# -------------------------------------------------------------------------------------------------
"""
Confidence-weighted position sizing with drawdown adjustment.

Scales position size based on signal confidence, current drawdown,
and correlation with existing positions.
"""

from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from typing import TYPE_CHECKING

from vwap_wave.config.settings import RiskConfig
from vwap_wave.setups.base_setup import SetupSignal


if TYPE_CHECKING:
    from vwap_wave.risk.correlation_manager import CorrelationManager
    from vwap_wave.risk.drawdown_manager import DrawdownManager


@dataclass
class PositionSizeResult:
    """Calculated position size with risk metrics."""

    quantity: Decimal
    risk_amount: Decimal
    risk_percent: Decimal
    confidence_multiplier: float
    drawdown_multiplier: float
    correlation_multiplier: float


class PositionSizer:
    """
    Confidence-weighted position sizing with drawdown adjustment.

    Scales position size based on signal confidence, current drawdown,
    and correlation with existing positions.

    Parameters
    ----------
    config : RiskConfig
        The risk configuration.
    drawdown_manager : DrawdownManager
        Drawdown manager instance.
    correlation_manager : CorrelationManager
        Correlation manager instance.

    """

    def __init__(
        self,
        config: RiskConfig,
        drawdown_manager: DrawdownManager,
        correlation_manager: CorrelationManager,
    ):
        self.config = config
        self.drawdown = drawdown_manager
        self.correlation = correlation_manager

    def calculate(
        self,
        signal: SetupSignal,
        account_equity: Decimal,
        symbol: str,
        tick_value: Decimal = Decimal("1"),
    ) -> PositionSizeResult:
        """
        Calculate position size for a signal.

        Parameters
        ----------
        signal : SetupSignal
            The trading signal with confidence score.
        account_equity : Decimal
            Current account equity.
        symbol : str
            Instrument symbol.
        tick_value : Decimal
            Value per tick/pip (default: 1).

        Returns
        -------
        PositionSizeResult
            Position size with risk metrics.

        """
        # Step 1: Base risk from confidence
        confidence_mult, base_risk = self._get_base_risk(signal.confidence)

        if base_risk == Decimal(0):
            return PositionSizeResult(
                quantity=Decimal(0),
                risk_amount=Decimal(0),
                risk_percent=Decimal(0),
                confidence_multiplier=0.0,
                drawdown_multiplier=0.0,
                correlation_multiplier=0.0,
            )

        # Step 2: Drawdown adjustment
        dd_mult = self._get_drawdown_multiplier()

        # Step 3: Correlation adjustment
        corr_mult = self.correlation.get_adjustment(symbol, signal.direction.value)

        # Step 4: Calculate final risk
        final_risk = (
            base_risk
            * Decimal(str(confidence_mult))
            * Decimal(str(dd_mult))
            * Decimal(str(corr_mult))
        )

        risk_amount = account_equity * final_risk

        # Step 5: Convert to position size
        stop_distance = Decimal(str(abs(signal.entry_price - signal.stop_price)))

        if stop_distance == 0:
            return PositionSizeResult(
                quantity=Decimal(0),
                risk_amount=Decimal(0),
                risk_percent=Decimal(0),
                confidence_multiplier=confidence_mult,
                drawdown_multiplier=dd_mult,
                correlation_multiplier=corr_mult,
            )

        # Quantity = risk_amount / (stop_distance * tick_value)
        quantity = risk_amount / (stop_distance * tick_value)

        return PositionSizeResult(
            quantity=quantity,
            risk_amount=risk_amount,
            risk_percent=final_risk,
            confidence_multiplier=confidence_mult,
            drawdown_multiplier=dd_mult,
            correlation_multiplier=corr_mult,
        )

    def _get_base_risk(self, confidence: float) -> tuple[float, Decimal]:
        """
        Get base risk percentage from confidence score.

        Parameters
        ----------
        confidence : float
            Signal confidence (0-1).

        Returns
        -------
        tuple[float, Decimal]
            Confidence multiplier and base risk amount.

        """
        high_threshold = float(self.config.high_confidence_threshold)
        medium_threshold = float(self.config.medium_confidence_threshold)
        low_threshold = float(self.config.low_confidence_threshold)

        if confidence >= high_threshold:
            return (1.0, self.config.max_risk_per_trade)

        elif confidence >= medium_threshold:
            ratio = (confidence - medium_threshold) / (high_threshold - medium_threshold)
            base = self.config.base_risk_per_trade + Decimal(str(ratio)) * (
                self.config.max_risk_per_trade - self.config.base_risk_per_trade
            )
            return (0.7 + ratio * 0.3, base)

        elif confidence >= low_threshold:
            ratio = (confidence - low_threshold) / (medium_threshold - low_threshold)
            base = self.config.min_risk_per_trade + Decimal(str(ratio)) * (
                self.config.base_risk_per_trade - self.config.min_risk_per_trade
            )
            return (0.5 + ratio * 0.2, base)

        else:
            return (0.0, Decimal(0))

    def _get_drawdown_multiplier(self) -> float:
        """
        Get risk multiplier based on current drawdown.

        Returns
        -------
        float
            Risk multiplier (0-1).

        """
        daily_dd = self.drawdown.daily_drawdown
        weekly_dd = self.drawdown.weekly_drawdown

        max_daily = float(self.config.max_daily_drawdown)
        max_weekly = float(self.config.max_weekly_drawdown)

        # Halt trading if limits exceeded
        if daily_dd >= max_daily or weekly_dd >= max_weekly:
            return 0.0

        # Progressive reduction near limits
        if daily_dd >= max_daily * 0.8:
            return 0.25
        elif daily_dd >= max_daily * 0.5:
            return 0.5
        elif weekly_dd >= max_weekly * 0.7:
            return 0.5
        else:
            return 1.0

    def calculate_max_quantity(
        self,
        account_equity: Decimal,
        entry_price: Decimal,
        stop_price: Decimal,
        tick_value: Decimal = Decimal("1"),
    ) -> Decimal:
        """
        Calculate maximum position size based on max risk.

        Parameters
        ----------
        account_equity : Decimal
            Current account equity.
        entry_price : Decimal
            Planned entry price.
        stop_price : Decimal
            Planned stop price.
        tick_value : Decimal
            Value per tick/pip.

        Returns
        -------
        Decimal
            Maximum position quantity.

        """
        stop_distance = abs(entry_price - stop_price)
        if stop_distance == 0:
            return Decimal(0)

        max_risk_amount = account_equity * self.config.max_risk_per_trade
        return max_risk_amount / (stop_distance * tick_value)
