# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Risk Management Tests
# -------------------------------------------------------------------------------------------------
"""Tests for risk management components."""

from datetime import datetime
from datetime import timedelta
from datetime import timezone
from decimal import Decimal
from unittest.mock import MagicMock

import pytest

from vwap_wave.config.settings import RiskConfig
from vwap_wave.risk.correlation_manager import CorrelationManager
from vwap_wave.risk.drawdown_manager import DrawdownManager
from vwap_wave.risk.position_sizer import PositionSizer
from vwap_wave.risk.position_sizer import PositionSizeResult
from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection


class TestPositionSizer:
    """Test cases for PositionSizer."""

    def setup_method(self):
        """Set up test fixtures."""
        self.config = RiskConfig(
            base_risk_per_trade=Decimal("0.01"),
            max_risk_per_trade=Decimal("0.02"),
            min_risk_per_trade=Decimal("0.005"),
            max_daily_drawdown=Decimal("0.05"),
            max_weekly_drawdown=Decimal("0.10"),
            high_confidence_threshold=0.8,
            medium_confidence_threshold=0.6,
            low_confidence_threshold=0.4,
            correlation_risk_reduction=0.5,
        )

        self.drawdown_manager = MagicMock()
        self.drawdown_manager.daily_drawdown = 0.0
        self.drawdown_manager.weekly_drawdown = 0.0

        self.correlation_manager = MagicMock()
        self.correlation_manager.get_adjustment.return_value = 1.0

        self.sizer = PositionSizer(
            self.config,
            self.drawdown_manager,
            self.correlation_manager,
        )

    def test_high_confidence_gets_max_risk(self):
        """Test high confidence signals get maximum risk allocation."""
        signal = SetupSignal(
            valid=True,
            setup_type="TEST",
            direction=TradeDirection.LONG,
            entry_price=1.1000,
            stop_price=1.0990,  # 10 pip stop
            target_price=1.1030,
            confidence=0.9,  # High confidence
            metadata={},
        )

        equity = Decimal("100000")
        result = self.sizer.calculate(signal, equity, "EUR/USD")

        assert result.risk_percent == self.config.max_risk_per_trade
        assert result.confidence_multiplier == 1.0

    def test_low_confidence_gets_min_risk(self):
        """Test low confidence signals get minimum risk allocation."""
        signal = SetupSignal(
            valid=True,
            setup_type="TEST",
            direction=TradeDirection.LONG,
            entry_price=1.1000,
            stop_price=1.0990,
            target_price=1.1030,
            confidence=0.45,  # Low confidence
            metadata={},
        )

        equity = Decimal("100000")
        result = self.sizer.calculate(signal, equity, "EUR/USD")

        # Should be at or near min risk
        assert result.risk_percent <= self.config.base_risk_per_trade
        assert result.confidence_multiplier < 1.0

    def test_below_threshold_confidence_returns_zero(self):
        """Test confidence below threshold returns zero position size."""
        signal = SetupSignal(
            valid=True,
            setup_type="TEST",
            direction=TradeDirection.LONG,
            entry_price=1.1000,
            stop_price=1.0990,
            target_price=1.1030,
            confidence=0.3,  # Below low threshold
            metadata={},
        )

        equity = Decimal("100000")
        result = self.sizer.calculate(signal, equity, "EUR/USD")

        assert result.quantity == Decimal(0)
        assert result.risk_amount == Decimal(0)

    def test_drawdown_reduces_position_size(self):
        """Test that drawdown reduces position size."""
        self.drawdown_manager.daily_drawdown = 0.04  # 80% of max daily

        signal = SetupSignal(
            valid=True,
            setup_type="TEST",
            direction=TradeDirection.LONG,
            entry_price=1.1000,
            stop_price=1.0990,
            target_price=1.1030,
            confidence=0.9,
            metadata={},
        )

        equity = Decimal("100000")
        result = self.sizer.calculate(signal, equity, "EUR/USD")

        assert result.drawdown_multiplier == 0.25  # 80% of max = 0.25 multiplier

    def test_correlation_reduces_position_size(self):
        """Test that correlation adjustment reduces position size."""
        self.correlation_manager.get_adjustment.return_value = 0.5

        signal = SetupSignal(
            valid=True,
            setup_type="TEST",
            direction=TradeDirection.LONG,
            entry_price=1.1000,
            stop_price=1.0990,
            target_price=1.1030,
            confidence=0.9,
            metadata={},
        )

        equity = Decimal("100000")
        result = self.sizer.calculate(signal, equity, "EUR/USD")

        assert result.correlation_multiplier == 0.5

    def test_position_quantity_calculation(self):
        """Test position quantity is calculated correctly."""
        signal = SetupSignal(
            valid=True,
            setup_type="TEST",
            direction=TradeDirection.LONG,
            entry_price=1.1000,
            stop_price=1.0990,  # 10 pip stop
            target_price=1.1030,
            confidence=0.9,
            metadata={},
        )

        equity = Decimal("100000")
        tick_value = Decimal("10")  # $10 per pip for 1 lot

        result = self.sizer.calculate(signal, equity, "EUR/USD", tick_value)

        # Risk amount = 100000 * 0.02 = 2000
        # Stop distance = 0.001
        # Quantity = 2000 / (0.001 * 10) = 200000
        assert result.risk_amount == Decimal("2000")


class TestDrawdownManager:
    """Test cases for DrawdownManager."""

    def setup_method(self):
        """Set up test fixtures."""
        self.config = RiskConfig(
            max_daily_drawdown=Decimal("0.05"),
            max_weekly_drawdown=Decimal("0.10"),
        )
        self.manager = DrawdownManager(self.config)

    def test_initial_state_not_halted(self):
        """Test initial state is not halted."""
        assert self.manager.is_halted is False
        assert self.manager.daily_drawdown == 0.0
        assert self.manager.weekly_drawdown == 0.0

    def test_high_water_mark_updates(self):
        """Test high water mark updates on equity increase."""
        now = datetime.now(timezone.utc)

        # Initial equity
        self.manager.update(Decimal("100000"), now)
        assert self.manager.state.daily_high_water_mark == Decimal("100000")

        # Equity increases
        self.manager.update(Decimal("105000"), now + timedelta(hours=1))
        assert self.manager.state.daily_high_water_mark == Decimal("105000")

    def test_daily_drawdown_calculation(self):
        """Test daily drawdown is calculated correctly."""
        now = datetime.now(timezone.utc)

        # Start at 100000
        self.manager.update(Decimal("100000"), now)

        # Drop to 97000 (3% drawdown)
        self.manager.update(Decimal("97000"), now + timedelta(hours=1))

        assert self.manager.daily_drawdown == pytest.approx(0.03, abs=0.001)

    def test_halts_at_daily_limit(self):
        """Test trading halts when daily limit is exceeded."""
        now = datetime.now(timezone.utc)

        self.manager.update(Decimal("100000"), now)
        self.manager.update(Decimal("95000"), now + timedelta(hours=1))  # 5% drawdown

        assert self.manager.is_halted is True
        assert self.manager.halt_reason == "daily_limit"

    def test_halts_at_weekly_limit(self):
        """Test trading halts when weekly limit is exceeded."""
        now = datetime.now(timezone.utc)

        self.manager.update(Decimal("100000"), now)
        self.manager.update(Decimal("90000"), now + timedelta(hours=1))  # 10% drawdown

        assert self.manager.is_halted is True
        assert self.manager.halt_reason == "weekly_limit"

    def test_daily_reset(self):
        """Test daily high water mark resets on new day."""
        day1 = datetime(2024, 1, 1, 12, 0, 0, tzinfo=timezone.utc)
        day2 = datetime(2024, 1, 2, 12, 0, 0, tzinfo=timezone.utc)

        # Day 1
        self.manager.update(Decimal("100000"), day1)
        self.manager.update(Decimal("97000"), day1 + timedelta(hours=1))

        # Day 2 - new high water mark
        self.manager.update(Decimal("97000"), day2)

        assert self.manager.state.daily_high_water_mark == Decimal("97000")
        assert self.manager.daily_drawdown == 0.0

    def test_remaining_allowance(self):
        """Test remaining drawdown allowance calculation."""
        now = datetime.now(timezone.utc)

        self.manager.update(Decimal("100000"), now)
        self.manager.update(Decimal("98000"), now + timedelta(hours=1))  # 2% drawdown

        assert self.manager.daily_remaining == pytest.approx(0.03, abs=0.001)  # 5% - 2% = 3%


class TestCorrelationManager:
    """Test cases for CorrelationManager."""

    def setup_method(self):
        """Set up test fixtures."""
        self.config = RiskConfig(correlation_risk_reduction=0.5)
        self.manager = CorrelationManager(self.config)

    def test_no_adjustment_for_uncorrelated(self):
        """Test no adjustment for uncorrelated instruments."""
        # XAU/USD is not in any correlation group
        adjustment = self.manager.get_adjustment("XAU/USD", "long")
        assert adjustment == 1.0

    def test_adjustment_for_first_correlated_position(self):
        """Test no adjustment for first position in correlation group."""
        # First position in USD_PAIRS group
        adjustment = self.manager.get_adjustment("EUR/USD", "long")
        assert adjustment == 1.0

    def test_adjustment_for_second_correlated_position(self):
        """Test adjustment for second correlated position."""
        # Register first position
        self.manager.register_position("EUR/USD", "long")

        # Second position in same direction
        adjustment = self.manager.get_adjustment("GBP/USD", "long")
        assert adjustment == self.config.correlation_risk_reduction  # 0.5

    def test_no_adjustment_for_opposite_direction(self):
        """Test no reduction for opposite direction (hedging)."""
        # Register long position
        self.manager.register_position("EUR/USD", "long")

        # Short position is hedging
        is_hedged = self.manager.is_hedged("GBP/USD", "short")
        assert is_hedged is True

    def test_blocks_third_correlated_position(self):
        """Test third correlated position is blocked."""
        # Register two positions
        self.manager.register_position("EUR/USD", "long")
        self.manager.register_position("GBP/USD", "long")

        # Third position should be heavily reduced
        adjustment = self.manager.get_adjustment("USD/JPY", "long")
        assert adjustment <= self.config.correlation_risk_reduction * 0.5

    def test_can_open_position_check(self):
        """Test can_open_position correctly checks constraints."""
        # First position is always allowed
        assert self.manager.can_open_position("EUR/USD", "long") is True

        # After multiple correlated positions, may be blocked
        self.manager.register_position("EUR/USD", "long")
        self.manager.register_position("GBP/USD", "long")
        self.manager.register_position("USD/CHF", "long")

        assert self.manager.can_open_position("USD/JPY", "long") is False

    def test_unregister_position(self):
        """Test position can be unregistered."""
        self.manager.register_position("EUR/USD", "long")
        self.manager.register_position("GBP/USD", "long")

        # Unregister one
        self.manager.unregister_position("EUR/USD")

        # Should only have one correlated position now
        adjustment = self.manager.get_adjustment("USD/JPY", "long")
        assert adjustment == self.config.correlation_risk_reduction

    def test_correlation_state(self):
        """Test correlation state reporting."""
        self.manager.register_position("EUR/USD", "long")
        self.manager.register_position("GBP/USD", "short")

        state = self.manager.state

        assert state.total_long_exposure == 1
        assert state.total_short_exposure == 1
        assert "USD_PAIRS" in state.group_exposures

    def test_reset_clears_positions(self):
        """Test reset clears all tracked positions."""
        self.manager.register_position("EUR/USD", "long")
        self.manager.register_position("GBP/USD", "long")

        self.manager.reset()

        state = self.manager.state
        assert state.total_long_exposure == 0
        assert state.total_short_exposure == 0
