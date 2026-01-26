# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Initial Balance Tracker
# -------------------------------------------------------------------------------------------------
"""
Initial Balance calculation and extension levels.

Tracks the high and low of the configurable initial balance period
and calculates extension levels for trade targets and exhaustion zones.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from datetime import timezone
from typing import Optional

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import Bar

from vwap_wave.config.settings import InitialBalanceConfig


@dataclass
class IBState:
    """Initial Balance state with extension levels."""

    ib_high: float
    ib_low: float
    ib_range: float
    ib_complete: bool
    ib_midpoint: float
    x1_upper: float  # IB High + 1x range
    x1_lower: float  # IB Low - 1x range
    x2_upper: float
    x2_lower: float
    x3_upper: float
    x3_lower: float


class InitialBalanceTracker(Indicator):
    """
    Tracks Initial Balance high/low and calculates extensions.

    IB is defined as the high-low range of the first N minutes of the session.
    Extensions are multiples of the IB range added/subtracted from IB high/low.

    Parameters
    ----------
    config : InitialBalanceConfig
        The initial balance configuration.
    session_start_hour : int
        The hour (UTC) when the session starts (default: 0).

    """

    def __init__(self, config: InitialBalanceConfig, session_start_hour: int = 0):
        super().__init__(params=[config.ib_period_minutes, session_start_hour])

        self.config = config
        self._ib_period_minutes = config.ib_period_minutes
        self._extensions = config.extensions
        self._session_start_hour = session_start_hour

        # State tracking
        self._ib_high: Optional[float] = None
        self._ib_low: Optional[float] = None
        self._ib_complete: bool = False
        self._session_start_time: Optional[datetime] = None
        self._current_state: Optional[IBState] = None
        self._last_session_date: Optional[datetime] = None

    def handle_bar(self, bar: Bar) -> None:
        """
        Process bar and update IB state.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        """
        PyCondition.not_none(bar, "bar")

        bar_dt = datetime.fromtimestamp(bar.ts_event / 1e9, tz=timezone.utc)

        # Check for new session
        if self._is_new_session(bar_dt):
            self._reset_session()
            self._session_start_time = bar_dt.replace(
                hour=self._session_start_hour,
                minute=0,
                second=0,
                microsecond=0,
            )
            self._last_session_date = bar_dt.date()

        # Get bar values
        high = bar.high.as_double()
        low = bar.low.as_double()

        # If IB period not complete, update high/low
        if not self._ib_complete and self._session_start_time is not None:
            minutes_since_start = self._minutes_since_session_start(bar_dt)

            if minutes_since_start <= self._ib_period_minutes:
                # Update IB range
                if self._ib_high is None or high > self._ib_high:
                    self._ib_high = high
                if self._ib_low is None or low < self._ib_low:
                    self._ib_low = low
            else:
                # IB period complete
                self._ib_complete = True
                self._calculate_extensions()

        # Update initialization state
        if not self.has_inputs:
            self._set_has_inputs(True)
        if not self.initialized and self._ib_complete:
            self._set_initialized(True)

    def _is_new_session(self, bar_dt: datetime) -> bool:
        """Check if we've entered a new session."""
        if self._last_session_date is None:
            return True

        current_date = bar_dt.date()
        if current_date > self._last_session_date:
            # Check if we've passed the session start hour
            if bar_dt.hour >= self._session_start_hour:
                return True

        return False

    def _minutes_since_session_start(self, bar_dt: datetime) -> float:
        """Calculate minutes since session start."""
        if self._session_start_time is None:
            return 0

        delta = bar_dt - self._session_start_time
        return delta.total_seconds() / 60.0

    def _calculate_extensions(self) -> None:
        """Calculate IB extension levels."""
        if self._ib_high is None or self._ib_low is None:
            return

        ib_range = self._ib_high - self._ib_low

        self._current_state = IBState(
            ib_high=self._ib_high,
            ib_low=self._ib_low,
            ib_range=ib_range,
            ib_complete=True,
            ib_midpoint=(self._ib_high + self._ib_low) / 2.0,
            x1_upper=self._ib_high + ib_range * self._extensions[0],
            x1_lower=self._ib_low - ib_range * self._extensions[0],
            x2_upper=self._ib_high + ib_range * self._extensions[1],
            x2_lower=self._ib_low - ib_range * self._extensions[1],
            x3_upper=self._ib_high + ib_range * self._extensions[2],
            x3_lower=self._ib_low - ib_range * self._extensions[2],
        )

    def _reset_session(self) -> None:
        """Reset for new session."""
        self._ib_high = None
        self._ib_low = None
        self._ib_complete = False
        self._session_start_time = None
        self._current_state = None

    def _reset(self) -> None:
        """Reset the indicator (called by base class)."""
        self._reset_session()
        self._last_session_date = None

    @property
    def state(self) -> Optional[IBState]:
        """Current IB state with extensions."""
        return self._current_state

    @property
    def is_complete(self) -> bool:
        """Whether the IB period is complete."""
        return self._ib_complete

    @property
    def ib_high(self) -> float:
        """IB high value."""
        return self._ib_high if self._ib_high is not None else 0.0

    @property
    def ib_low(self) -> float:
        """IB low value."""
        return self._ib_low if self._ib_low is not None else 0.0

    @property
    def ib_range(self) -> float:
        """IB range value."""
        if self._current_state is not None:
            return self._current_state.ib_range
        if self._ib_high is not None and self._ib_low is not None:
            return self._ib_high - self._ib_low
        return 0.0

    def get_extension_level(self, multiplier: float, upper: bool) -> float:
        """
        Get extension level for a given multiplier.

        Parameters
        ----------
        multiplier : float
            The extension multiplier (e.g., 1.0, 2.0, 3.0).
        upper : bool
            True for upper extension, False for lower.

        Returns
        -------
        float
            The extension level.

        """
        if self._ib_high is None or self._ib_low is None:
            return 0.0

        ib_range = self._ib_high - self._ib_low

        if upper:
            return self._ib_high + ib_range * multiplier
        else:
            return self._ib_low - ib_range * multiplier

    def price_in_ib_range(self, price: float) -> bool:
        """Check if price is within the IB range."""
        if self._ib_high is None or self._ib_low is None:
            return False
        return self._ib_low <= price <= self._ib_high

    def price_above_ib(self, price: float) -> bool:
        """Check if price is above IB high."""
        if self._ib_high is None:
            return False
        return price > self._ib_high

    def price_below_ib(self, price: float) -> bool:
        """Check if price is below IB low."""
        if self._ib_low is None:
            return False
        return price < self._ib_low

    def get_ib_extension_level(self, price: float) -> float:
        """
        Get the IB extension level for a price.

        Returns positive values for prices above IB high, negative for below IB low.
        Returns 0 for prices within IB range.
        """
        if self._ib_high is None or self._ib_low is None:
            return 0.0

        ib_range = self._ib_high - self._ib_low
        if ib_range == 0:
            return 0.0

        if price > self._ib_high:
            return (price - self._ib_high) / ib_range
        elif price < self._ib_low:
            return (self._ib_low - price) / ib_range
        else:
            return 0.0
