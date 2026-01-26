# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Acceptance Tests
# -------------------------------------------------------------------------------------------------
"""Tests for the Acceptance Engine."""

from unittest.mock import MagicMock

import pytest

from vwap_wave.analysis.acceptance import AcceptanceEngine
from vwap_wave.analysis.acceptance import AcceptanceResult
from vwap_wave.analysis.acceptance import AcceptanceType
from vwap_wave.analysis.acceptance import Direction
from vwap_wave.config.settings import AcceptanceConfig


def create_mock_bar(high: float, low: float, close: float, open_: float, volume: float) -> MagicMock:
    """Create a mock bar with given values."""
    bar = MagicMock()
    bar.high.as_double.return_value = high
    bar.low.as_double.return_value = low
    bar.close.as_double.return_value = close
    bar.open.as_double.return_value = open_
    bar.volume.as_double.return_value = volume
    return bar


class TestAcceptanceEngine:
    """Test cases for AcceptanceEngine."""

    def test_initialization(self):
        """Test engine initializes with config."""
        config = AcceptanceConfig()
        engine = AcceptanceEngine(config)

        assert engine.config == config

    def test_evaluate_returns_no_acceptance_when_insufficient_data(self):
        """Test returns no acceptance when insufficient data."""
        config = AcceptanceConfig()
        engine = AcceptanceEngine(config)

        result = engine.evaluate(1.1000, Direction.LONG, lookback_bars=5)

        assert result.accepted is False
        assert result.acceptance_type == AcceptanceType.NONE

    def test_time_acceptance_with_consecutive_closes(self):
        """Test time acceptance with consecutive closes above level."""
        config = AcceptanceConfig(time_bars=2, distance_atr_mult=1.0)
        engine = AcceptanceEngine(config)

        # Add bars that close above the level
        level = 1.1000
        bars = [
            create_mock_bar(1.1020, 1.0990, 1.1010, 1.1000, 1000),  # Close above
            create_mock_bar(1.1030, 1.1005, 1.1020, 1.1010, 1000),  # Close above
            create_mock_bar(1.1040, 1.1010, 1.1030, 1.1020, 1000),  # Close above
        ]

        for bar in bars:
            engine.update(bar, atr=0.0010, avg_volume=1000)

        result = engine.evaluate(level, Direction.LONG, lookback_bars=3)

        assert result.accepted is True
        assert result.consecutive_closes >= config.time_bars

    def test_no_acceptance_when_closes_not_consecutive(self):
        """Test no acceptance when closes are not consecutive."""
        config = AcceptanceConfig(time_bars=3)
        engine = AcceptanceEngine(config)

        level = 1.1000
        bars = [
            create_mock_bar(1.1020, 1.0990, 1.1010, 1.1000, 1000),  # Above
            create_mock_bar(1.1010, 1.0980, 1.0990, 1.1000, 1000),  # Below
            create_mock_bar(1.1020, 1.0995, 1.1015, 1.1000, 1000),  # Above
        ]

        for bar in bars:
            engine.update(bar, atr=0.0010, avg_volume=1000)

        result = engine.evaluate(level, Direction.LONG, lookback_bars=3)

        # Should not have 3 consecutive closes
        assert result.consecutive_closes < config.time_bars

    def test_distance_acceptance_with_atr_move(self):
        """Test distance acceptance with significant ATR move."""
        config = AcceptanceConfig(
            time_bars=5,  # High time threshold
            distance_atr_mult=0.5,  # Low distance threshold
            momentum_threshold=0.5,
        )
        engine = AcceptanceEngine(config)

        level = 1.1000
        atr = 0.0020  # 20 pips ATR

        # Create strong bullish bar with momentum
        bar = create_mock_bar(
            high=1.1030,  # 30 pips above level
            low=1.1005,
            close=1.1025,  # Strong close (25 pips above level = 1.25 ATR)
            open_=1.1010,  # Body shows momentum
            volume=2000,
        )

        engine.update(bar, atr=atr, avg_volume=1000)

        result = engine.evaluate(level, Direction.LONG, lookback_bars=1)

        assert result.max_distance_atr > config.distance_atr_mult

    def test_volume_confirmation(self):
        """Test volume confirmation multiplier."""
        config = AcceptanceConfig(time_bars=2, volume_mult=1.5)
        engine = AcceptanceEngine(config)

        level = 1.1000
        avg_volume = 1000

        # Bars with high volume
        bars = [
            create_mock_bar(1.1020, 1.0995, 1.1015, 1.1000, 2000),  # High volume
            create_mock_bar(1.1025, 1.1005, 1.1020, 1.1010, 2000),  # High volume
        ]

        for bar in bars:
            engine.update(bar, atr=0.0010, avg_volume=avg_volume)

        result = engine.evaluate(level, Direction.LONG, lookback_bars=2)

        assert result.volume_confirmed is True

    def test_no_volume_confirmation_with_low_volume(self):
        """Test no volume confirmation with low volume."""
        config = AcceptanceConfig(time_bars=2, volume_mult=1.5)
        engine = AcceptanceEngine(config)

        level = 1.1000
        avg_volume = 1000

        # Bars with low volume
        bars = [
            create_mock_bar(1.1020, 1.0995, 1.1015, 1.1000, 500),  # Low volume
            create_mock_bar(1.1025, 1.1005, 1.1020, 1.1010, 500),  # Low volume
        ]

        for bar in bars:
            engine.update(bar, atr=0.0010, avg_volume=avg_volume)

        result = engine.evaluate(level, Direction.LONG, lookback_bars=2)

        assert result.volume_confirmed is False

    def test_short_direction_acceptance(self):
        """Test acceptance in short direction (below level)."""
        config = AcceptanceConfig(time_bars=2)
        engine = AcceptanceEngine(config)

        level = 1.1000

        # Bars closing below level
        bars = [
            create_mock_bar(1.1005, 1.0970, 1.0980, 1.1000, 1000),  # Close below
            create_mock_bar(1.0990, 1.0960, 1.0970, 1.0980, 1000),  # Close below
        ]

        for bar in bars:
            engine.update(bar, atr=0.0010, avg_volume=1000)

        result = engine.evaluate(level, Direction.SHORT, lookback_bars=2)

        assert result.accepted is True
        assert result.consecutive_closes >= config.time_bars

    def test_combined_time_and_distance_acceptance(self):
        """Test combined time and distance acceptance gives highest confidence."""
        config = AcceptanceConfig(
            time_bars=2,
            distance_atr_mult=0.5,
            momentum_threshold=0.5,
            volume_mult=1.5,
        )
        engine = AcceptanceEngine(config)

        level = 1.1000
        atr = 0.0010

        # Strong bars with time, distance, and volume
        bars = [
            create_mock_bar(1.1020, 1.1005, 1.1018, 1.1008, 2000),  # Momentum bar
            create_mock_bar(1.1030, 1.1015, 1.1028, 1.1018, 2000),  # Momentum bar
        ]

        for bar in bars:
            engine.update(bar, atr=atr, avg_volume=1000)

        result = engine.evaluate(level, Direction.LONG, lookback_bars=2)

        assert result.accepted is True
        if result.acceptance_type == AcceptanceType.TIME_AND_DISTANCE:
            assert result.confidence == 1.0
        assert result.volume_confirmed is True

    def test_reset_clears_state(self):
        """Test reset clears all internal state."""
        config = AcceptanceConfig()
        engine = AcceptanceEngine(config)

        # Add some data
        bar = create_mock_bar(1.1020, 1.0990, 1.1010, 1.1000, 1000)
        engine.update(bar, atr=0.0010, avg_volume=1000)

        # Reset
        engine.reset()

        # Should have no data
        result = engine.evaluate(1.1000, Direction.LONG, lookback_bars=5)
        assert result.accepted is False
