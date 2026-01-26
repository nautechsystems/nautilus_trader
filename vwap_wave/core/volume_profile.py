# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Volume Profile Builder
# -------------------------------------------------------------------------------------------------
"""
Volume profile construction with High Volume Node (HVN) and Low Volume Node (LVN) detection.

Builds a price-volume distribution profile to identify areas of acceptance (HVNs)
and rejection (LVNs) for trade targeting and support/resistance levels.
"""

from __future__ import annotations

from collections import deque
from dataclasses import dataclass
from enum import Enum
from typing import Optional

import numpy as np

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators import Indicator
from nautilus_trader.model.data import Bar

from vwap_wave.config.settings import VolumeProfileConfig


class VolumeNodeType(Enum):
    """Type of volume node."""

    HVN = "hvn"  # High Volume Node
    LVN = "lvn"  # Low Volume Node
    NEUTRAL = "neutral"


@dataclass
class VolumeNode:
    """A volume node in the profile."""

    price_level: float
    volume: float
    node_type: VolumeNodeType
    percentile: float


@dataclass
class VolumeProfileState:
    """Current volume profile state."""

    poc: float  # Point of Control (highest volume level)
    poc_volume: float
    value_area_high: float  # Upper bound of 70% volume
    value_area_low: float  # Lower bound of 70% volume
    profile_high: float  # Highest price in profile
    profile_low: float  # Lowest price in profile
    hvn_levels: list[float]  # High Volume Node price levels
    lvn_levels: list[float]  # Low Volume Node price levels
    total_volume: float


class VolumeProfileBuilder(Indicator):
    """
    Builds volume profile with HVN/LVN detection.

    The profile distributes volume across price buckets to identify
    areas of high acceptance (HVNs) and low acceptance (LVNs).

    Parameters
    ----------
    config : VolumeProfileConfig
        The volume profile configuration.

    """

    def __init__(self, config: VolumeProfileConfig):
        super().__init__(params=[config.lookback_bars, config.price_buckets])

        self.config = config
        self._lookback_bars = config.lookback_bars
        self._price_buckets = config.price_buckets
        self._hvn_percentile = config.hvn_percentile
        self._lvn_percentile = config.lvn_percentile

        # State tracking
        self._bar_history: deque = deque(maxlen=config.lookback_bars)
        self._current_state: Optional[VolumeProfileState] = None
        self._bar_count: int = 0

    def handle_bar(self, bar: Bar) -> None:
        """
        Process bar and update volume profile.

        Parameters
        ----------
        bar : Bar
            The bar to process.

        """
        PyCondition.not_none(bar, "bar")

        # Store bar data
        self._bar_history.append({
            "high": bar.high.as_double(),
            "low": bar.low.as_double(),
            "close": bar.close.as_double(),
            "volume": bar.volume.as_double(),
        })
        self._bar_count += 1

        # Rebuild profile
        if len(self._bar_history) >= 10:
            self._build_profile()

        # Update initialization state
        if not self.has_inputs:
            self._set_has_inputs(True)
        if not self.initialized and self._bar_count >= self._lookback_bars:
            self._set_initialized(True)

    def _build_profile(self) -> None:
        """Build the volume profile from bar history."""
        if len(self._bar_history) < 2:
            return

        bars = list(self._bar_history)

        # Find price range
        all_highs = [b["high"] for b in bars]
        all_lows = [b["low"] for b in bars]
        profile_high = max(all_highs)
        profile_low = min(all_lows)

        price_range = profile_high - profile_low
        if price_range == 0:
            return

        # Create price buckets
        bucket_size = price_range / self._price_buckets
        buckets = np.zeros(self._price_buckets)

        # Distribute volume across buckets using typical price
        for bar in bars:
            typical_price = (bar["high"] + bar["low"] + bar["close"]) / 3.0
            volume = bar["volume"]

            # Distribute volume to the appropriate bucket
            # Also distribute some volume to adjacent buckets for the bar's range
            for price in [bar["low"], typical_price, bar["high"]]:
                bucket_idx = int((price - profile_low) / bucket_size)
                bucket_idx = max(0, min(bucket_idx, self._price_buckets - 1))
                buckets[bucket_idx] += volume / 3.0

        # Find POC (highest volume bucket)
        poc_bucket = np.argmax(buckets)
        poc = profile_low + (poc_bucket + 0.5) * bucket_size
        poc_volume = buckets[poc_bucket]

        # Calculate value area (70% of volume centered on POC)
        total_volume = np.sum(buckets)
        if total_volume == 0:
            return

        value_area_volume = total_volume * 0.7
        accumulated_volume = poc_volume
        upper_idx = poc_bucket
        lower_idx = poc_bucket

        while accumulated_volume < value_area_volume:
            # Extend to the side with higher volume
            upper_vol = buckets[upper_idx + 1] if upper_idx + 1 < self._price_buckets else 0
            lower_vol = buckets[lower_idx - 1] if lower_idx - 1 >= 0 else 0

            if upper_vol >= lower_vol and upper_idx + 1 < self._price_buckets:
                upper_idx += 1
                accumulated_volume += upper_vol
            elif lower_idx - 1 >= 0:
                lower_idx -= 1
                accumulated_volume += lower_vol
            else:
                break

        value_area_high = profile_low + (upper_idx + 1) * bucket_size
        value_area_low = profile_low + lower_idx * bucket_size

        # Identify HVN and LVN levels
        volume_threshold_hvn = np.percentile(buckets[buckets > 0], self._hvn_percentile)
        volume_threshold_lvn = np.percentile(buckets[buckets > 0], self._lvn_percentile)

        hvn_levels = []
        lvn_levels = []

        for i, vol in enumerate(buckets):
            price_level = profile_low + (i + 0.5) * bucket_size
            if vol >= volume_threshold_hvn:
                hvn_levels.append(price_level)
            elif vol > 0 and vol <= volume_threshold_lvn:
                lvn_levels.append(price_level)

        self._current_state = VolumeProfileState(
            poc=poc,
            poc_volume=poc_volume,
            value_area_high=value_area_high,
            value_area_low=value_area_low,
            profile_high=profile_high,
            profile_low=profile_low,
            hvn_levels=hvn_levels,
            lvn_levels=lvn_levels,
            total_volume=total_volume,
        )

    def _reset(self) -> None:
        """Reset the indicator (called by base class)."""
        self._bar_history.clear()
        self._current_state = None
        self._bar_count = 0

    @property
    def state(self) -> Optional[VolumeProfileState]:
        """Current volume profile state."""
        return self._current_state

    @property
    def poc(self) -> float:
        """Point of Control (highest volume level)."""
        return self._current_state.poc if self._current_state else 0.0

    @property
    def value_area_high(self) -> float:
        """Upper bound of value area."""
        return self._current_state.value_area_high if self._current_state else 0.0

    @property
    def value_area_low(self) -> float:
        """Lower bound of value area."""
        return self._current_state.value_area_low if self._current_state else 0.0

    def get_nearest_hvn(self, price: float) -> Optional[float]:
        """Get the nearest HVN to a price."""
        if self._current_state is None or not self._current_state.hvn_levels:
            return None

        return min(self._current_state.hvn_levels, key=lambda x: abs(x - price))

    def get_nearest_lvn(self, price: float) -> Optional[float]:
        """Get the nearest LVN to a price."""
        if self._current_state is None or not self._current_state.lvn_levels:
            return None

        return min(self._current_state.lvn_levels, key=lambda x: abs(x - price))

    def price_in_value_area(self, price: float) -> bool:
        """Check if price is within the value area."""
        if self._current_state is None:
            return False
        return self._current_state.value_area_low <= price <= self._current_state.value_area_high

    def get_node_type(self, price: float, tolerance: float = 0.001) -> VolumeNodeType:
        """
        Get the volume node type at a price level.

        Parameters
        ----------
        price : float
            The price to check.
        tolerance : float
            Relative tolerance for matching levels.

        Returns
        -------
        VolumeNodeType
            The type of volume node at this level.

        """
        if self._current_state is None:
            return VolumeNodeType.NEUTRAL

        # Check HVN levels
        for hvn in self._current_state.hvn_levels:
            if abs(price - hvn) / hvn < tolerance:
                return VolumeNodeType.HVN

        # Check LVN levels
        for lvn in self._current_state.lvn_levels:
            if abs(price - lvn) / lvn < tolerance:
                return VolumeNodeType.LVN

        return VolumeNodeType.NEUTRAL
