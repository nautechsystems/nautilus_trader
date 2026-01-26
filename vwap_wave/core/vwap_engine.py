# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - VWAP Engine
# -------------------------------------------------------------------------------------------------
"""
VWAP calculation with session-anchored reset and standard deviation bands.

The engine tracks cumulative price-volume and cumulative volume to compute
VWAP incrementally on each bar.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime
from datetime import timezone
from typing import Optional

import numpy as np

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import Bar

from vwap_wave.config.settings import VWAPConfig


@dataclass
class VWAPState:
    """Current VWAP calculation state with all bands."""

    vwap: float
    sd1_upper: float
    sd1_lower: float
    sd2_upper: float
    sd2_lower: float
    sd3_upper: float
    sd3_lower: float
    cumulative_pv: float  # Cumulative (price * volume)
    cumulative_volume: float
    std_dev: float  # Current standard deviation


class VWAPEngine(Indicator):
    """
    Session-anchored VWAP with standard deviation bands.

    Resets at the configured session hour (default: 00:00 UTC).
    Calculates bands at 1, 2, and 3 standard deviations.

    Parameters
    ----------
    config : VWAPConfig
        The VWAP configuration.

    """

    def __init__(self, config: VWAPConfig):
        super().__init__(params=[config.session_reset_hour])

        self.config = config
        self._session_reset_hour = config.session_reset_hour
        self._sd_bands = config.sd_bands

        # State tracking
        self._cumulative_pv: float = 0.0
        self._cumulative_volume: float = 0.0
        self._prices: list[float] = []
        self._volumes: list[float] = []
        self._current_state: Optional[VWAPState] = None
        self._last_reset_date: Optional[datetime] = None
        self._bar_count: int = 0

    def handle_bar(self, bar: Bar) -> None:
        """
        Process new bar and update VWAP state.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        """
        PyCondition.not_none(bar, "bar")

        # Check for session reset
        bar_dt = datetime.fromtimestamp(bar.ts_event / 1e9, tz=timezone.utc)
        if self._should_reset(bar_dt):
            self._reset_session()
            self._last_reset_date = bar_dt.date()

        # Calculate typical price: (H + L + C) / 3
        high = bar.high.as_double()
        low = bar.low.as_double()
        close = bar.close.as_double()
        volume = bar.volume.as_double()

        typical_price = (high + low + close) / 3.0

        # Update cumulative values
        self._cumulative_pv += typical_price * volume
        self._cumulative_volume += volume
        self._prices.append(typical_price)
        self._volumes.append(volume)
        self._bar_count += 1

        # Calculate VWAP
        if self._cumulative_volume > 0:
            vwap = self._cumulative_pv / self._cumulative_volume
        else:
            vwap = typical_price

        # Calculate volume-weighted standard deviation
        std_dev = self._calculate_vwap_std_dev(vwap)

        # Build state object with bands
        self._current_state = VWAPState(
            vwap=vwap,
            sd1_upper=vwap + std_dev * self._sd_bands[0],
            sd1_lower=vwap - std_dev * self._sd_bands[0],
            sd2_upper=vwap + std_dev * self._sd_bands[1],
            sd2_lower=vwap - std_dev * self._sd_bands[1],
            sd3_upper=vwap + std_dev * self._sd_bands[2],
            sd3_lower=vwap - std_dev * self._sd_bands[2],
            cumulative_pv=self._cumulative_pv,
            cumulative_volume=self._cumulative_volume,
            std_dev=std_dev,
        )

        # Update initialization state
        if not self.has_inputs:
            self._set_has_inputs(True)
        if not self.initialized and self._bar_count >= 2:
            self._set_initialized(True)

    def _calculate_vwap_std_dev(self, vwap: float) -> float:
        """Calculate volume-weighted standard deviation from VWAP."""
        if len(self._prices) < 2:
            return 0.0

        prices = np.array(self._prices)
        volumes = np.array(self._volumes)

        # Volume-weighted squared deviations
        squared_devs = volumes * (prices - vwap) ** 2
        total_volume = np.sum(volumes)

        if total_volume == 0:
            return 0.0

        variance = np.sum(squared_devs) / total_volume
        return float(np.sqrt(variance))

    def _should_reset(self, bar_dt: datetime) -> bool:
        """Check if we've crossed into a new session."""
        if self._last_reset_date is None:
            return True

        # Check if this is a new day and we've passed the reset hour
        current_date = bar_dt.date()
        if current_date > self._last_reset_date:
            if bar_dt.hour >= self._session_reset_hour:
                return True

        return False

    def _reset_session(self) -> None:
        """Reset all cumulative values for new session."""
        self._cumulative_pv = 0.0
        self._cumulative_volume = 0.0
        self._prices.clear()
        self._volumes.clear()
        self._current_state = None
        self._bar_count = 0

    def _reset(self) -> None:
        """Reset the indicator (called by base class)."""
        self._reset_session()
        self._last_reset_date = None

    @property
    def state(self) -> Optional[VWAPState]:
        """Current VWAP state with all bands."""
        return self._current_state

    @property
    def vwap(self) -> float:
        """Current VWAP value."""
        return self._current_state.vwap if self._current_state else 0.0

    @property
    def std_dev(self) -> float:
        """Current standard deviation."""
        return self._current_state.std_dev if self._current_state else 0.0

    def get_band(self, level: int, upper: bool) -> float:
        """
        Get a specific band value.

        Parameters
        ----------
        level : int
            The band level (1, 2, or 3).
        upper : bool
            True for upper band, False for lower band.

        Returns
        -------
        float
            The band value.

        """
        if self._current_state is None:
            return 0.0

        if level == 1:
            return self._current_state.sd1_upper if upper else self._current_state.sd1_lower
        elif level == 2:
            return self._current_state.sd2_upper if upper else self._current_state.sd2_lower
        elif level == 3:
            return self._current_state.sd3_upper if upper else self._current_state.sd3_lower
        else:
            return 0.0

    def price_in_value_area(self, price: float) -> bool:
        """Check if price is within the SD1 bands (value area)."""
        if self._current_state is None:
            return False
        return self._current_state.sd1_lower <= price <= self._current_state.sd1_upper

    def price_above_value(self, price: float) -> bool:
        """Check if price is above the value area (SD1 upper)."""
        if self._current_state is None:
            return False
        return price > self._current_state.sd1_upper

    def price_below_value(self, price: float) -> bool:
        """Check if price is below the value area (SD1 lower)."""
        if self._current_state is None:
            return False
        return price < self._current_state.sd1_lower

    def get_sd_level(self, price: float) -> float:
        """
        Get the standard deviation level for a price.

        Returns positive values for prices above VWAP, negative for below.
        """
        if self._current_state is None or self._current_state.std_dev == 0:
            return 0.0
        return (price - self._current_state.vwap) / self._current_state.std_dev
