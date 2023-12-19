import importlib.util
import sys
from unittest.mock import Mock

import pytest

from nautilus_trader.model.data import BarType


if importlib.util.find_spec("talib") is None:
    if sys.platform == "linux":
        # Raise the exception (expecting talib to be available on Linux)
        error_message = (
            "Failed to import TA-Lib. This module requires TA-Lib to be installed. "
            "Please visit https://github.com/TA-Lib/ta-lib-python for installation instructions. "
            "If TA-Lib is already installed, ensure it is correctly added to your Python environment."
        )
        raise ImportError(error_message)
    pytestmark = pytest.mark.skip(reason="talib is not installed")
else:
    from nautilus_trader.indicators.ta_lib.manager import TAFunctionWrapper
    from nautilus_trader.indicators.ta_lib.manager import TALibIndicatorManager


@pytest.fixture(scope="session")
def bar_type() -> BarType:
    return BarType.from_str("EUR/USD.IDEALPRO-1-HOUR-MID-EXTERNAL")


@pytest.fixture()
def indicator_manager() -> "TALibIndicatorManager":
    return TALibIndicatorManager(
        bar_type=BarType.from_str("EUR/USD.IDEALPRO-1-HOUR-MID-EXTERNAL"),
        period=10,
    )


def test_setup():
    # Arrange
    bar_type = BarType.from_str("EUR/USD.IDEALPRO-1-HOUR-MID-EXTERNAL")
    period = 10

    # Act
    indicator_manager = TALibIndicatorManager(bar_type=bar_type, period=period)

    # Assert
    assert indicator_manager.bar_type == bar_type
    assert indicator_manager.period == period


def test_invalid_bar_type():
    # Arrange, Act, Assert
    with pytest.raises(TypeError):
        TALibIndicatorManager(bar_type="invalid_bar_type", period=10)


def test_not_positive_period(bar_type):
    # Arrange, Act, Assert
    with pytest.raises(ValueError):
        TALibIndicatorManager(bar_type=bar_type, period=0)


def test_not_positive_buffer_size(bar_type):
    # Arrange, Act, Assert
    with pytest.raises(ValueError):
        TALibIndicatorManager(bar_type=bar_type, period=10, buffer_size=0)


def test_skip_uniform_price_bar_default_true(indicator_manager):
    # Assert
    indicator_manager._skip_uniform_price_bar = True


def test_skip_zero_close_bar_default_true(indicator_manager):
    # Assert
    indicator_manager._skip_zero_close_bar = True


def test_set_indicators(indicator_manager):
    # Arrange
    indicators = (TAFunctionWrapper("SMA", {"timeperiod": 50}), TAFunctionWrapper("MACD"))

    # Act
    indicator_manager.set_indicators(indicators)

    # Assert
    assert len(indicator_manager._indicators) == len(indicators)


def test_output_names_generation(indicator_manager):
    # Arrange
    indicators = (TAFunctionWrapper("SMA", {"timeperiod": 50}),)
    # Act
    indicator_manager.set_indicators(indicators)
    expected_output_names = indicator_manager.input_names() + indicators[0].output_names

    # Assert
    assert indicator_manager.output_names == tuple(expected_output_names)


def test_increment_count_correctly_increases_counter(indicator_manager):
    # Arrange
    indicator_manager._stable_period = 5

    # Act
    for i in range(2):
        indicator_manager._increment_count()

    # Assert
    assert indicator_manager.count == 2


def test_indicator_remains_uninitialized_with_insufficient_input_count(indicator_manager):
    # Arrange
    indicator_manager._stable_period = 5

    # Act
    for i in range(4):
        indicator_manager._increment_count()

    # Assert
    assert indicator_manager.has_inputs is True
    assert indicator_manager.initialized is False


def test_indicator_initializes_after_receiving_required_input_count(indicator_manager):
    # Arrange
    indicator_manager._stable_period = 5
    indicator_manager._calculate_ta = Mock()

    # Act
    for i in range(5):
        indicator_manager._increment_count()

    # Assert
    assert indicator_manager.has_inputs is True
    assert indicator_manager.initialized is True


def test_calculate_ta_called_on_initialization(indicator_manager):
    # Arrange
    indicator_manager._stable_period = 5
    indicator_manager._calculate_ta = Mock()

    # Act
    for i in range(5):
        indicator_manager._increment_count()

    # Assert
    indicator_manager._calculate_ta.assert_called_once()
