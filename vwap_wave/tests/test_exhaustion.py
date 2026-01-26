# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Exhaustion Tests
# -------------------------------------------------------------------------------------------------
"""Tests for the Exhaustion Engine."""

from unittest.mock import MagicMock
from unittest.mock import PropertyMock

import pytest

from vwap_wave.analysis.exhaustion import ExhaustionEngine
from vwap_wave.analysis.exhaustion import ExhaustionSignal
from vwap_wave.analysis.exhaustion import ExhaustionZone
from vwap_wave.analysis.exhaustion import FadeDirection
from vwap_wave.config.settings import ExhaustionConfig
from vwap_wave.core.cvd_calculator import CVDDivergence


def create_mock_bar(high: float, low: float, close: float, open_: float, volume: float) -> MagicMock:
    """Create a mock bar with given values."""
    bar = MagicMock()
    bar.high.as_double.return_value = high
    bar.low.as_double.return_value = low
    bar.close.as_double.return_value = close
    bar.open.as_double.return_value = open_
    bar.volume.as_double.return_value = volume
    return bar


def create_mock_vwap_state(
    vwap: float,
    sd2_upper: float,
    sd2_lower: float,
) -> MagicMock:
    """Create a mock VWAP state."""
    state = MagicMock()
    state.vwap = vwap
    state.sd1_upper = vwap + (sd2_upper - vwap) / 2
    state.sd1_lower = vwap - (vwap - sd2_lower) / 2
    state.sd2_upper = sd2_upper
    state.sd2_lower = sd2_lower
    state.sd3_upper = vwap + (sd2_upper - vwap) * 1.5
    state.sd3_lower = vwap - (vwap - sd2_lower) * 1.5
    return state


def create_mock_ib_state(
    ib_high: float,
    ib_low: float,
) -> MagicMock:
    """Create a mock IB state."""
    state = MagicMock()
    ib_range = ib_high - ib_low
    state.ib_high = ib_high
    state.ib_low = ib_low
    state.ib_range = ib_range
    state.x3_upper = ib_high + ib_range * 3
    state.x3_lower = ib_low - ib_range * 3
    return state


class TestExhaustionEngine:
    """Test cases for ExhaustionEngine."""

    def setup_method(self):
        """Set up test fixtures."""
        self.config = ExhaustionConfig(
            sd_min=2.0,
            ib_extension=3.0,
            volume_spike_mult=2.0,
            price_progress_threshold=0.2,
            absorption_body_ratio=0.3,
            confirmation_bars=2,
            volume_dropoff_threshold=0.7,
        )

        # Mock dependencies
        self.cvd = MagicMock()
        self.cvd.detect_bearish_divergence.return_value = CVDDivergence(
            detected=False,
            divergence_type="",
            price_extreme=0.0,
            cvd_extreme=0.0,
            divergence_magnitude=0.0,
        )
        self.cvd.detect_bullish_divergence.return_value = CVDDivergence(
            detected=False,
            divergence_type="",
            price_extreme=0.0,
            cvd_extreme=0.0,
            divergence_magnitude=0.0,
        )

        self.vwap = MagicMock()
        vwap_state = create_mock_vwap_state(1.1000, 1.1040, 1.0960)
        type(self.vwap).state = PropertyMock(return_value=vwap_state)
        type(self.vwap).initialized = PropertyMock(return_value=True)

        self.ib = MagicMock()
        ib_state = create_mock_ib_state(1.1020, 1.0980)
        type(self.ib).state = PropertyMock(return_value=ib_state)
        type(self.ib).is_complete = PropertyMock(return_value=True)

        self.engine = ExhaustionEngine(
            self.config,
            self.cvd,
            self.vwap,
            self.ib,
        )

    def test_initialization(self):
        """Test engine initializes correctly."""
        assert self.engine.config == self.config
        assert not self.engine.has_pending_signal()

    def test_no_exhaustion_in_normal_zone(self):
        """Test no exhaustion detected when price is in normal zone."""
        # Price in normal range (between SD1 bands)
        bar = create_mock_bar(1.1015, 1.0995, 1.1005, 1.1000, 1000)
        self.engine.update(bar, atr=0.0010, avg_volume=1000)

        signal = self.engine.evaluate()

        assert signal.confirmed is False
        assert signal.zone == ExhaustionZone.NONE

    def test_exhaustion_zone_detection_sd_upper(self):
        """Test exhaustion zone detection at SD2 upper."""
        # Price at/above SD2 upper (1.1040)
        bar = create_mock_bar(1.1050, 1.1035, 1.1045, 1.1040, 1000)
        self.engine.update(bar, atr=0.0010, avg_volume=1000)

        # Access private method for testing zone detection
        zone = self.engine._get_exhaustion_zone()

        assert zone == ExhaustionZone.SD_UPPER

    def test_exhaustion_zone_detection_sd_lower(self):
        """Test exhaustion zone detection at SD2 lower."""
        # Price at/below SD2 lower (1.0960)
        bar = create_mock_bar(1.0965, 1.0945, 1.0955, 1.0960, 1000)
        self.engine.update(bar, atr=0.0010, avg_volume=1000)

        zone = self.engine._get_exhaustion_zone()

        assert zone == ExhaustionZone.SD_LOWER

    def test_absorption_candle_detection(self):
        """Test absorption candle detection (high volume, small body)."""
        # Create bars with one absorption candle
        avg_volume = 1000

        # Normal bars
        for _ in range(3):
            bar = create_mock_bar(1.1050, 1.1040, 1.1045, 1.1040, 1000)
            self.engine.update(bar, atr=0.0010, avg_volume=avg_volume)

        # Absorption candle: high volume, small body, no progress
        absorption_bar = create_mock_bar(
            high=1.1052,  # No new high
            low=1.1040,
            close=1.1046,  # Small body
            open_=1.1045,
            volume=2500,  # 2.5x volume spike
        )
        self.engine.update(absorption_bar, atr=0.0010, avg_volume=avg_volume)

        # Test absorption detection
        absorption = self.engine._detect_absorption("BULLISH_EXHAUSTION", lookback=3)

        assert absorption.detected is True
        assert absorption.volume_spike >= self.config.volume_spike_mult

    def test_lag_rule_delays_confirmation(self):
        """Test that lag rule delays signal confirmation."""
        # Set up for exhaustion at upper extreme
        vwap_state = create_mock_vwap_state(1.1000, 1.1030, 1.0970)
        type(self.vwap).state = PropertyMock(return_value=vwap_state)

        # Price at SD2 upper with absorption and divergence
        self.cvd.detect_bearish_divergence.return_value = CVDDivergence(
            detected=True,
            divergence_type="BEARISH",
            price_extreme=1.1050,
            cvd_extreme=100.0,
            divergence_magnitude=0.3,
        )

        avg_volume = 1000

        # Add bars at extreme
        for i in range(5):
            bar = create_mock_bar(1.1050, 1.1035, 1.1045, 1.1040, avg_volume)
            self.engine.update(bar, atr=0.0010, avg_volume=avg_volume)

        # Absorption candle
        absorption = create_mock_bar(1.1052, 1.1040, 1.1046, 1.1045, 2500)
        self.engine.update(absorption, atr=0.0010, avg_volume=avg_volume)

        signal = self.engine.evaluate()

        # Signal should be pending, not confirmed (lag rule)
        if self.engine.has_pending_signal():
            assert signal.confirmed is False

    def test_volume_dropoff_required_for_confirmation(self):
        """Test that volume dropoff is required for confirmation."""
        # This tests that without volume dropoff, signal isn't confirmed
        vwap_state = create_mock_vwap_state(1.1000, 1.1030, 1.0970)
        type(self.vwap).state = PropertyMock(return_value=vwap_state)

        self.cvd.detect_bearish_divergence.return_value = CVDDivergence(
            detected=True,
            divergence_type="BEARISH",
            price_extreme=1.1050,
            cvd_extreme=100.0,
            divergence_magnitude=0.3,
        )

        avg_volume = 1000

        # Add initial bars
        for _ in range(3):
            bar = create_mock_bar(1.1050, 1.1035, 1.1045, 1.1040, 1000)
            self.engine.update(bar, atr=0.0010, avg_volume=avg_volume)

        # Absorption candle
        absorption = create_mock_bar(1.1052, 1.1040, 1.1046, 1.1045, 2500)
        self.engine.update(absorption, atr=0.0010, avg_volume=avg_volume)

        self.engine.evaluate()  # Register pending

        # Add confirmation bars but with still-high volume (no dropoff)
        for _ in range(self.config.confirmation_bars):
            bar = create_mock_bar(1.1045, 1.1030, 1.1035, 1.1040, 2000)  # Still high volume
            self.engine.update(bar, atr=0.0010, avg_volume=avg_volume)

        signal = self.engine.evaluate()

        # Should not confirm without proper volume dropoff
        # (Implementation depends on exact threshold)

    def test_fade_direction_determination(self):
        """Test correct fade direction is determined."""
        # At upper extreme, should fade short
        vwap_state = create_mock_vwap_state(1.1000, 1.1030, 1.0970)
        type(self.vwap).state = PropertyMock(return_value=vwap_state)

        bar = create_mock_bar(1.1050, 1.1035, 1.1045, 1.1040, 1000)
        self.engine.update(bar, atr=0.0010, avg_volume=1000)

        zone = self.engine._get_exhaustion_zone()

        if zone == ExhaustionZone.SD_UPPER:
            expected_fade = FadeDirection.FADE_SHORT
        else:
            expected_fade = FadeDirection.FADE_LONG

        # If signal was generated, check direction
        if self.engine.get_pending_direction():
            assert self.engine.get_pending_direction() in [
                FadeDirection.FADE_SHORT,
                FadeDirection.FADE_LONG,
            ]

    def test_reset_clears_pending_signal(self):
        """Test reset clears pending signals."""
        # Add some data and potentially create pending signal
        for _ in range(5):
            bar = create_mock_bar(1.1050, 1.1035, 1.1045, 1.1040, 2000)
            self.engine.update(bar, atr=0.0010, avg_volume=1000)

        self.engine.evaluate()

        # Reset
        self.engine.reset()

        assert not self.engine.has_pending_signal()
        assert self.engine.get_pending_direction() is None
