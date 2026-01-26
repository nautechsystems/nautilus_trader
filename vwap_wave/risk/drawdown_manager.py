# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Drawdown Manager
# -------------------------------------------------------------------------------------------------
"""
Daily and weekly drawdown tracking and limits.

Monitors account equity to enforce drawdown limits and halt trading
when thresholds are exceeded.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import date
from datetime import datetime
from datetime import timezone
from decimal import Decimal
from typing import Optional

from vwap_wave.config.settings import RiskConfig


@dataclass
class DrawdownState:
    """Current drawdown state."""

    daily_drawdown: float
    weekly_drawdown: float
    daily_high_water_mark: Decimal
    weekly_high_water_mark: Decimal
    is_halted: bool
    halt_reason: str


class DrawdownManager:
    """
    Tracks daily and weekly drawdown limits.

    Monitors account equity and halts trading when drawdown limits
    are exceeded. Resets at appropriate intervals.

    Parameters
    ----------
    config : RiskConfig
        The risk configuration.

    """

    def __init__(self, config: RiskConfig):
        self.config = config

        # State tracking
        self._daily_high_water_mark: Decimal = Decimal(0)
        self._weekly_high_water_mark: Decimal = Decimal(0)
        self._current_equity: Decimal = Decimal(0)

        self._last_daily_reset: Optional[date] = None
        self._last_weekly_reset: Optional[date] = None

        self._is_halted: bool = False
        self._halt_reason: str = ""

    def update(self, current_equity: Decimal, timestamp: Optional[datetime] = None) -> DrawdownState:
        """
        Update drawdown tracking with current equity.

        Parameters
        ----------
        current_equity : Decimal
            Current account equity.
        timestamp : datetime, optional
            Current timestamp (uses now if not provided).

        Returns
        -------
        DrawdownState
            Current drawdown state.

        """
        if timestamp is None:
            timestamp = datetime.now(timezone.utc)

        current_date = timestamp.date()

        # Check for daily reset
        if self._last_daily_reset is None or current_date > self._last_daily_reset:
            self._daily_high_water_mark = current_equity
            self._last_daily_reset = current_date
            # Reset daily halt if it was a daily limit issue
            if self._halt_reason == "daily_limit":
                self._is_halted = False
                self._halt_reason = ""

        # Check for weekly reset (Monday)
        if self._last_weekly_reset is None:
            self._weekly_high_water_mark = current_equity
            self._last_weekly_reset = current_date
        elif current_date.isocalendar()[1] != self._last_weekly_reset.isocalendar()[1]:
            # New week
            self._weekly_high_water_mark = current_equity
            self._last_weekly_reset = current_date
            # Reset weekly halt
            if self._halt_reason == "weekly_limit":
                self._is_halted = False
                self._halt_reason = ""

        self._current_equity = current_equity

        # Update high water marks
        if current_equity > self._daily_high_water_mark:
            self._daily_high_water_mark = current_equity
        if current_equity > self._weekly_high_water_mark:
            self._weekly_high_water_mark = current_equity

        # Check drawdown limits
        self._check_limits()

        return self.state

    def _check_limits(self) -> None:
        """Check if drawdown limits have been exceeded."""
        daily_dd = self.daily_drawdown
        weekly_dd = self.weekly_drawdown

        max_daily = float(self.config.max_daily_drawdown)
        max_weekly = float(self.config.max_weekly_drawdown)

        if daily_dd >= max_daily:
            self._is_halted = True
            self._halt_reason = "daily_limit"

        if weekly_dd >= max_weekly:
            self._is_halted = True
            self._halt_reason = "weekly_limit"

    @property
    def daily_drawdown(self) -> float:
        """Calculate current daily drawdown percentage."""
        if self._daily_high_water_mark == 0:
            return 0.0

        drawdown = (self._daily_high_water_mark - self._current_equity) / self._daily_high_water_mark
        return max(0.0, float(drawdown))

    @property
    def weekly_drawdown(self) -> float:
        """Calculate current weekly drawdown percentage."""
        if self._weekly_high_water_mark == 0:
            return 0.0

        drawdown = (
            self._weekly_high_water_mark - self._current_equity
        ) / self._weekly_high_water_mark
        return max(0.0, float(drawdown))

    @property
    def is_halted(self) -> bool:
        """Check if trading is halted due to drawdown."""
        return self._is_halted

    @property
    def halt_reason(self) -> str:
        """Get the reason for trading halt."""
        return self._halt_reason

    @property
    def state(self) -> DrawdownState:
        """Get current drawdown state."""
        return DrawdownState(
            daily_drawdown=self.daily_drawdown,
            weekly_drawdown=self.weekly_drawdown,
            daily_high_water_mark=self._daily_high_water_mark,
            weekly_high_water_mark=self._weekly_high_water_mark,
            is_halted=self._is_halted,
            halt_reason=self._halt_reason,
        )

    @property
    def daily_remaining(self) -> float:
        """Get remaining daily drawdown allowance."""
        return max(0.0, float(self.config.max_daily_drawdown) - self.daily_drawdown)

    @property
    def weekly_remaining(self) -> float:
        """Get remaining weekly drawdown allowance."""
        return max(0.0, float(self.config.max_weekly_drawdown) - self.weekly_drawdown)

    def force_halt(self, reason: str) -> None:
        """Manually halt trading."""
        self._is_halted = True
        self._halt_reason = reason

    def force_resume(self) -> None:
        """Manually resume trading (use with caution)."""
        self._is_halted = False
        self._halt_reason = ""

    def reset(self) -> None:
        """Reset all drawdown tracking."""
        self._daily_high_water_mark = Decimal(0)
        self._weekly_high_water_mark = Decimal(0)
        self._current_equity = Decimal(0)
        self._last_daily_reset = None
        self._last_weekly_reset = None
        self._is_halted = False
        self._halt_reason = ""
