import pytest

from nautilus_trader.core.nautilus_pyo3 import AroonOscillator
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


@pytest.fixture(scope="function")
def aroon():
    return AroonOscillator(10)


def test_name_returns_expected_string(aroon: AroonOscillator):
    assert aroon.name == "AroonOscillator"


def test_period(aroon: AroonOscillator):
    # Arrange, Act, Assert
    assert aroon.period == 10


def test_initialized_without_inputs_returns_false(aroon: AroonOscillator):
    # Arrange, Act, Assert
    assert aroon.initialized is False


def test_initialized_with_required_inputs_returns_true(aroon: AroonOscillator):
    # Arrange, Act
    for _i in range(20):
        aroon.update_raw(110.08, 109.61)

    # Assert
    assert aroon.initialized is True


def test_handle_bar_updates_indicator(aroon: AroonOscillator):
    # Arrange
    indicator = AroonOscillator(1)
    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    indicator.handle_bar(bar)

    # Assert
    assert indicator.has_inputs
    assert indicator.aroon_up == 100.0
    assert indicator.aroon_down == 100.0
    assert indicator.value == 0


def test_value_with_one_input(aroon: AroonOscillator):
    # Arrange
    aroon = AroonOscillator(1)

    # Act
    aroon.update_raw(110.08, 109.61)

    # Assert
    assert aroon.aroon_up == 100.0
    assert aroon.aroon_down == 100.0
    assert aroon.value == 0


def test_value_with_twenty_inputs(aroon: AroonOscillator):
    # Arrange, Act
    aroon.update_raw(110.08, 109.61)
    aroon.update_raw(110.15, 109.91)
    aroon.update_raw(110.1, 109.73)
    aroon.update_raw(110.06, 109.77)
    aroon.update_raw(110.29, 109.88)
    aroon.update_raw(110.53, 110.29)
    aroon.update_raw(110.61, 110.26)
    aroon.update_raw(110.28, 110.17)
    aroon.update_raw(110.3, 110.0)
    aroon.update_raw(110.25, 110.01)
    aroon.update_raw(110.25, 109.81)
    aroon.update_raw(109.92, 109.71)
    aroon.update_raw(110.21, 109.84)
    aroon.update_raw(110.08, 109.95)
    aroon.update_raw(110.2, 109.96)
    aroon.update_raw(110.16, 109.95)
    aroon.update_raw(109.99, 109.75)
    aroon.update_raw(110.2, 109.73)
    aroon.update_raw(110.1, 109.81)
    aroon.update_raw(110.04, 109.96)

    # Assert
    assert aroon.aroon_up == 10.0
    assert aroon.aroon_down == 20.0
    assert aroon.value == -10.0


def test_reset_successfully_returns_indicator_to_fresh_state(aroon: AroonOscillator):
    # Arrange
    for _i in range(1000):
        aroon.update_raw(110.08, 109.61)

    # Act
    aroon.reset()

    # Assert
    assert not aroon.initialized
    assert aroon.aroon_up == 0
    assert aroon.aroon_down == 0
    assert aroon.value == 0
