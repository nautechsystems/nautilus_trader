# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Correlation Manager
# -------------------------------------------------------------------------------------------------
"""
Cross-pair correlation risk adjustment.

Manages exposure across correlated instruments to prevent over-concentration
of risk in a single market direction.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Dict
from typing import List
from typing import Optional
from typing import Set

from vwap_wave.config.instruments import CORRELATION_GROUPS
from vwap_wave.config.instruments import get_correlation_group
from vwap_wave.config.settings import RiskConfig


@dataclass
class CorrelationState:
    """Current correlation exposure state."""

    total_long_exposure: int
    total_short_exposure: int
    group_exposures: Dict[str, Dict[str, int]]  # group -> {"long": n, "short": n}
    correlated_symbols: List[str]


@dataclass
class OpenPosition:
    """Tracked open position for correlation management."""

    symbol: str
    direction: str  # "long" or "short"
    correlation_group: Optional[str]


class CorrelationManager:
    """
    Manages correlation risk across instruments.

    Tracks open positions by correlation group and adjusts position
    sizing to prevent over-concentration of risk.

    Parameters
    ----------
    config : RiskConfig
        The risk configuration.

    """

    def __init__(self, config: RiskConfig):
        self.config = config
        self._open_positions: Dict[str, OpenPosition] = {}
        self._correlation_groups = CORRELATION_GROUPS

    def register_position(self, symbol: str, direction: str) -> None:
        """
        Register a new open position.

        Parameters
        ----------
        symbol : str
            Instrument symbol.
        direction : str
            Trade direction ("long" or "short").

        """
        correlation_group = get_correlation_group(symbol)
        self._open_positions[symbol] = OpenPosition(
            symbol=symbol,
            direction=direction.lower(),
            correlation_group=correlation_group,
        )

    def unregister_position(self, symbol: str) -> None:
        """
        Remove a closed position.

        Parameters
        ----------
        symbol : str
            Instrument symbol.

        """
        if symbol in self._open_positions:
            del self._open_positions[symbol]

    def get_adjustment(self, symbol: str, direction: str) -> float:
        """
        Get position size adjustment for correlation risk.

        Parameters
        ----------
        symbol : str
            Instrument symbol.
        direction : str
            Proposed trade direction ("long" or "short").

        Returns
        -------
        float
            Adjustment multiplier (0-1).

        """
        direction = direction.lower()
        correlation_group = get_correlation_group(symbol)

        # If no correlation group, no adjustment needed
        if correlation_group is None:
            return 1.0

        # Count existing exposure in the same direction within the group
        same_direction_count = 0
        for pos in self._open_positions.values():
            if pos.correlation_group == correlation_group and pos.direction == direction:
                same_direction_count += 1

        # Apply progressive reduction for correlated positions
        if same_direction_count == 0:
            return 1.0
        elif same_direction_count == 1:
            return self.config.correlation_risk_reduction
        elif same_direction_count == 2:
            return self.config.correlation_risk_reduction * 0.5
        else:
            return 0.0  # Block additional correlated positions

    def can_open_position(self, symbol: str, direction: str) -> bool:
        """
        Check if a new position can be opened given correlation constraints.

        Parameters
        ----------
        symbol : str
            Instrument symbol.
        direction : str
            Proposed trade direction.

        Returns
        -------
        bool
            True if position can be opened.

        """
        return self.get_adjustment(symbol, direction) > 0

    def get_correlated_symbols(self, symbol: str) -> List[str]:
        """
        Get all symbols correlated with the given symbol.

        Parameters
        ----------
        symbol : str
            Instrument symbol.

        Returns
        -------
        List[str]
            List of correlated symbols.

        """
        correlation_group = get_correlation_group(symbol)
        if correlation_group is None:
            return []

        return [s for s in self._correlation_groups.get(correlation_group, []) if s != symbol]

    def get_open_correlated_positions(self, symbol: str) -> List[OpenPosition]:
        """
        Get open positions in the same correlation group.

        Parameters
        ----------
        symbol : str
            Instrument symbol.

        Returns
        -------
        List[OpenPosition]
            List of correlated open positions.

        """
        correlation_group = get_correlation_group(symbol)
        if correlation_group is None:
            return []

        return [
            pos
            for pos in self._open_positions.values()
            if pos.correlation_group == correlation_group and pos.symbol != symbol
        ]

    @property
    def state(self) -> CorrelationState:
        """Get current correlation state."""
        total_long = sum(1 for p in self._open_positions.values() if p.direction == "long")
        total_short = sum(1 for p in self._open_positions.values() if p.direction == "short")

        group_exposures: Dict[str, Dict[str, int]] = {}
        for group_name in self._correlation_groups:
            long_count = sum(
                1
                for p in self._open_positions.values()
                if p.correlation_group == group_name and p.direction == "long"
            )
            short_count = sum(
                1
                for p in self._open_positions.values()
                if p.correlation_group == group_name and p.direction == "short"
            )
            if long_count > 0 or short_count > 0:
                group_exposures[group_name] = {"long": long_count, "short": short_count}

        all_correlated: Set[str] = set()
        for pos in self._open_positions.values():
            if pos.correlation_group:
                all_correlated.update(self._correlation_groups.get(pos.correlation_group, []))

        return CorrelationState(
            total_long_exposure=total_long,
            total_short_exposure=total_short,
            group_exposures=group_exposures,
            correlated_symbols=list(all_correlated),
        )

    def is_hedged(self, symbol: str, direction: str) -> bool:
        """
        Check if a position would be hedged against existing positions.

        Parameters
        ----------
        symbol : str
            Instrument symbol.
        direction : str
            Proposed trade direction.

        Returns
        -------
        bool
            True if position would hedge existing exposure.

        """
        direction = direction.lower()
        opposite = "short" if direction == "long" else "long"
        correlation_group = get_correlation_group(symbol)

        if correlation_group is None:
            return False

        # Check if there's opposite exposure in the same group
        for pos in self._open_positions.values():
            if pos.correlation_group == correlation_group and pos.direction == opposite:
                return True

        return False

    def reset(self) -> None:
        """Reset all position tracking."""
        self._open_positions.clear()
