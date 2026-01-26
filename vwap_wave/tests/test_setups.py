# -------------------------------------------------------------------------------------------------
#  VWAP Wave Trading System - Setup Tests
# -------------------------------------------------------------------------------------------------
"""Tests for the trading setups."""

from unittest.mock import MagicMock
from unittest.mock import PropertyMock

import pytest

from vwap_wave.analysis.regime_classifier import MarketRegime
from vwap_wave.analysis.regime_classifier import RegimeState
from vwap_wave.config.settings import VWAPWaveConfig
from vwap_wave.setups.base_setup import SetupSignal
from vwap_wave.setups.base_setup import TradeDirection
from vwap_wave.setups.discovery_continuation import DiscoveryContinuationSetup
from vwap_wave.setups.fade_extremes import FadeExtremesSetup
from vwap_wave.setups.return_to_value import ReturnToValueSetup
from vwap_wave.setups.vwap_bounce import VWAPBounceSetup


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
    sd1_upper: float,
    sd1_lower: float,
    sd2_upper: float,
    sd2_lower: float,
) -> MagicMock:
    """Create a mock VWAP state."""
    state = MagicMock()
    state.vwap = vwap
    state.sd1_upper = sd1_upper
    state.sd1_lower = sd1_lower
    state.sd2_upper = sd2_upper
    state.sd2_lower = sd2_lower
    state.sd3_upper = vwap + (sd2_upper - vwap) * 1.5
    state.sd3_lower = vwap - (vwap - sd2_lower) * 1.5
    return state


def create_mock_ib_state(ib_high: float, ib_low: float) -> MagicMock:
    """Create a mock IB state."""
    state = MagicMock()
    ib_range = ib_high - ib_low
    state.ib_high = ib_high
    state.ib_low = ib_low
    state.ib_range = ib_range
    state.x1_upper = ib_high + ib_range
    state.x1_lower = ib_low - ib_range
    state.x2_upper = ib_high + ib_range * 2
    state.x2_lower = ib_low - ib_range * 2
    state.x3_upper = ib_high + ib_range * 3
    state.x3_lower = ib_low - ib_range * 3
    return state


class TestDiscoveryContinuationSetup:
    """Test cases for Price Discovery Continuation setup."""

    def setup_method(self):
        """Set up test fixtures."""
        self.config = VWAPWaveConfig()

        self.vwap_engine = MagicMock()
        vwap_state = create_mock_vwap_state(1.1000, 1.1020, 1.0980, 1.1040, 1.0960)
        type(self.vwap_engine).state = PropertyMock(return_value=vwap_state)

        self.ib_tracker = MagicMock()
        ib_state = create_mock_ib_state(1.1015, 1.0985)
        type(self.ib_tracker).state = PropertyMock(return_value=ib_state)
        type(self.ib_tracker).is_complete = PropertyMock(return_value=True)

        self.acceptance_engine = MagicMock()

        self.setup = DiscoveryContinuationSetup(
            self.config,
            self.vwap_engine,
            self.ib_tracker,
            self.acceptance_engine,
        )

    def test_eligible_in_imbalance_bullish(self):
        """Test setup is eligible in bullish imbalance regime."""
        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BULLISH,
            bars_in_regime=5,
            acceptance_confidence=0.8,
            volume_confirmed=True,
        )

        assert self.setup.is_eligible(regime_state) is True

    def test_eligible_in_imbalance_bearish(self):
        """Test setup is eligible in bearish imbalance regime."""
        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BEARISH,
            bars_in_regime=5,
            acceptance_confidence=0.8,
            volume_confirmed=True,
        )

        assert self.setup.is_eligible(regime_state) is True

    def test_not_eligible_in_balance(self):
        """Test setup is not eligible in balance regime."""
        regime_state = RegimeState(
            regime=MarketRegime.BALANCE,
            bars_in_regime=5,
            acceptance_confidence=0.5,
            volume_confirmed=True,
        )

        assert self.setup.is_eligible(regime_state) is False

    def test_not_eligible_with_insufficient_bars(self):
        """Test setup is not eligible with insufficient bars in trend."""
        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BULLISH,
            bars_in_regime=1,  # Below MIN_BARS_IN_TREND
            acceptance_confidence=0.8,
            volume_confirmed=True,
        )

        assert self.setup.is_eligible(regime_state) is False

    def test_bullish_signal_on_pullback(self):
        """Test bullish signal generated on pullback to support."""
        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BULLISH,
            bars_in_regime=5,
            acceptance_confidence=0.8,
            volume_confirmed=True,
        )

        # Bar at SD1 upper (pullback target) with bullish close
        bar = create_mock_bar(
            high=1.1025,
            low=1.1015,
            close=1.1022,  # Bullish close
            open_=1.1017,
            volume=1000,
        )

        atr = 0.0010

        signal = self.setup.evaluate(regime_state, bar, atr)

        assert signal.valid is True
        assert signal.direction == TradeDirection.LONG
        assert signal.setup_type == "DISCOVERY_CONTINUATION"

    def test_no_signal_when_not_pulled_back(self):
        """Test no signal when price hasn't pulled back enough."""
        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BULLISH,
            bars_in_regime=5,
            acceptance_confidence=0.8,
            volume_confirmed=True,
        )

        # Bar far above SD1 upper
        bar = create_mock_bar(
            high=1.1045,
            low=1.1035,
            close=1.1040,
            open_=1.1037,
            volume=1000,
        )

        atr = 0.0010

        signal = self.setup.evaluate(regime_state, bar, atr)

        assert signal.valid is False


class TestFadeExtremesSetup:
    """Test cases for Fade Value Area Extremes setup."""

    def setup_method(self):
        """Set up test fixtures."""
        self.config = VWAPWaveConfig()

        self.exhaustion_engine = MagicMock()
        self.vwap_engine = MagicMock()
        self.volume_profile = MagicMock()

        vwap_state = create_mock_vwap_state(1.1000, 1.1020, 1.0980, 1.1040, 1.0960)
        type(self.vwap_engine).state = PropertyMock(return_value=vwap_state)

        type(self.volume_profile).state = PropertyMock(return_value=None)
        type(self.volume_profile).poc = PropertyMock(return_value=1.1000)

        self.setup = FadeExtremesSetup(
            self.config,
            self.exhaustion_engine,
            self.vwap_engine,
            self.volume_profile,
        )

    def test_eligible_when_exhaustion_pending(self):
        """Test setup is eligible when exhaustion signal is pending."""
        type(self.exhaustion_engine).has_pending_signal = MagicMock(return_value=True)

        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BULLISH,
            bars_in_regime=5,
            acceptance_confidence=0.8,
            volume_confirmed=True,
        )

        assert self.setup.is_eligible(regime_state) is True

    def test_eligible_in_balance_regime(self):
        """Test setup is eligible in balance regime."""
        type(self.exhaustion_engine).has_pending_signal = MagicMock(return_value=False)

        regime_state = RegimeState(
            regime=MarketRegime.BALANCE,
            bars_in_regime=5,
            acceptance_confidence=0.5,
            volume_confirmed=True,
        )

        assert self.setup.is_eligible(regime_state) is True


class TestReturnToValueSetup:
    """Test cases for Return to Value setup."""

    def setup_method(self):
        """Set up test fixtures."""
        self.config = VWAPWaveConfig()

        self.vwap_engine = MagicMock()
        self.acceptance_engine = MagicMock()
        self.rejection_engine = MagicMock()

        vwap_state = create_mock_vwap_state(1.1000, 1.1020, 1.0980, 1.1040, 1.0960)
        type(self.vwap_engine).state = PropertyMock(return_value=vwap_state)

        self.setup = ReturnToValueSetup(
            self.config,
            self.vwap_engine,
            self.acceptance_engine,
            self.rejection_engine,
        )

    def test_eligible_on_breakout_unconfirmed(self):
        """Test setup is eligible on unconfirmed breakout."""
        regime_state = RegimeState(
            regime=MarketRegime.BREAKOUT_UNCONFIRMED,
            bars_in_regime=3,
            acceptance_confidence=0.4,
            volume_confirmed=False,
        )

        assert self.setup.is_eligible(regime_state) is True

    def test_eligible_on_regime_transition_to_balance(self):
        """Test setup is eligible when transitioning from imbalance to balance."""
        regime_state = RegimeState(
            regime=MarketRegime.BALANCE,
            bars_in_regime=2,
            acceptance_confidence=0.5,
            volume_confirmed=True,
            previous_regime=MarketRegime.IMBALANCE_BULLISH,
        )

        assert self.setup.is_eligible(regime_state) is True


class TestVWAPBounceSetup:
    """Test cases for VWAP Bounce setup."""

    def setup_method(self):
        """Set up test fixtures."""
        self.config = VWAPWaveConfig()

        self.vwap_engine = MagicMock()
        self.cvd_calculator = MagicMock()
        self.regime_classifier = MagicMock()

        vwap_state = create_mock_vwap_state(1.1000, 1.1020, 1.0980, 1.1040, 1.0960)
        type(self.vwap_engine).state = PropertyMock(return_value=vwap_state)

        type(self.cvd_calculator).is_cvd_rising = PropertyMock(return_value=True)
        type(self.cvd_calculator).is_cvd_falling = PropertyMock(return_value=False)
        type(self.cvd_calculator).cvd_trend = PropertyMock(return_value=100.0)

        from vwap_wave.core.cvd_calculator import CVDDivergence
        self.cvd_calculator.detect_bearish_divergence.return_value = CVDDivergence(
            detected=False, divergence_type="", price_extreme=0, cvd_extreme=0, divergence_magnitude=0
        )
        self.cvd_calculator.detect_bullish_divergence.return_value = CVDDivergence(
            detected=False, divergence_type="", price_extreme=0, cvd_extreme=0, divergence_magnitude=0
        )

        self.setup = VWAPBounceSetup(
            self.config,
            self.vwap_engine,
            self.cvd_calculator,
            self.regime_classifier,
        )

    def test_eligible_in_bullish_imbalance_with_duration(self):
        """Test setup is eligible in bullish imbalance with sufficient duration."""
        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BULLISH,
            bars_in_regime=10,  # Above MIN_BARS_IN_TREND
            acceptance_confidence=0.8,
            volume_confirmed=True,
        )

        assert self.setup.is_eligible(regime_state) is True

    def test_not_eligible_without_volume_confirmation(self):
        """Test setup is not eligible without volume confirmation."""
        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BULLISH,
            bars_in_regime=10,
            acceptance_confidence=0.8,
            volume_confirmed=False,  # No volume confirmation
        )

        assert self.setup.is_eligible(regime_state) is False

    def test_bullish_bounce_signal(self):
        """Test bullish bounce signal on VWAP touch."""
        regime_state = RegimeState(
            regime=MarketRegime.IMBALANCE_BULLISH,
            bars_in_regime=10,
            acceptance_confidence=0.8,
            volume_confirmed=True,
        )

        # Bar that touches VWAP (1.1000) and bounces up
        bar = create_mock_bar(
            high=1.1015,
            low=1.0998,  # Touches VWAP
            close=1.1012,  # Strong close (bullish)
            open_=1.1002,
            volume=1000,
        )

        atr = 0.0010

        signal = self.setup.evaluate(regime_state, bar, atr)

        assert signal.valid is True
        assert signal.direction == TradeDirection.LONG
        assert signal.setup_type == "VWAP_BOUNCE"


class TestSetupSignal:
    """Test cases for SetupSignal dataclass."""

    def test_no_signal_factory(self):
        """Test no_signal factory method."""
        signal = SetupSignal.no_signal()

        assert signal.valid is False
        assert signal.confidence == 0.0

    def test_risk_reward_ratio_calculation(self):
        """Test risk-reward ratio calculation."""
        signal = SetupSignal(
            valid=True,
            setup_type="TEST",
            direction=TradeDirection.LONG,
            entry_price=1.1000,
            stop_price=1.0990,  # 10 pip risk
            target_price=1.1030,  # 30 pip reward
            confidence=0.8,
            metadata={},
        )

        assert signal.risk_reward_ratio == 3.0  # 30/10 = 3:1

    def test_risk_amount_calculation(self):
        """Test risk amount calculation."""
        signal = SetupSignal(
            valid=True,
            setup_type="TEST",
            direction=TradeDirection.LONG,
            entry_price=1.1000,
            stop_price=1.0990,
            target_price=1.1030,
            confidence=0.8,
            metadata={},
        )

        assert signal.risk_amount == pytest.approx(0.0010, abs=0.0001)
