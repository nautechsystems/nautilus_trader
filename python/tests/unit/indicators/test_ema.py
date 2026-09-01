# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Test ema behavior.
"""

import pytest

from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.model import PriceType
from tests.stubs import TestDataProviderPyo3


@pytest.fixture
def ema() -> ExponentialMovingAverage:
    """
    Ema.
    """
    return ExponentialMovingAverage(10)


def test_name_returns_expected_string(ema: ExponentialMovingAverage) -> None:
    """
    Test name returns expected string.
    """
    # Arrange, Act, Assert
    assert ema.name == "ExponentialMovingAverage"


def test_str_repr_returns_expected_string(ema: ExponentialMovingAverage) -> None:
    """
    Test str repr returns expected string.
    """
    # Arrange, Act, Assert
    assert str(ema) == "ExponentialMovingAverage(10)"
    assert repr(ema) == "ExponentialMovingAverage(10)"


def test_period_returns_expected_value(ema: ExponentialMovingAverage) -> None:
    """
    Test period returns expected value.
    """
    # Arrange, Act, Assert
    assert ema.period == 10


def test_multiplier_returns_expected_value(ema: ExponentialMovingAverage) -> None:
    """
    Test multiplier returns expected value.
    """
    # Arrange, Act, Assert
    assert ema.alpha == 0.18181818181818182


def test_initialized_without_inputs_returns_false(ema: ExponentialMovingAverage) -> None:
    """
    Test initialized without inputs returns false.
    """
    # Arrange, Act, Assert
    assert not ema.initialized


def test_initialized_with_required_inputs_returns_true(ema: ExponentialMovingAverage) -> None:
    """
    Test initialized with required inputs returns true.
    """
    # Arrange
    ema.update_raw(1.00000)
    ema.update_raw(2.00000)
    ema.update_raw(3.00000)
    ema.update_raw(4.00000)
    ema.update_raw(5.00000)
    ema.update_raw(6.00000)
    ema.update_raw(7.00000)
    ema.update_raw(8.00000)
    ema.update_raw(9.00000)
    ema.update_raw(10.00000)

    # Act

    # Assert
    assert ema.initialized


def test_handle_quote_tick_updates_indicator() -> None:
    """
    Test handle quote tick updates indicator.
    """
    # Arrange
    indicator = ExponentialMovingAverage(10, PriceType.MID)

    tick = TestDataProviderPyo3.quote_tick()

    # Act
    indicator.handle_quote_tick(tick)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == pytest.approx(1987.5)


def test_handle_trade_tick_updates_indicator(ema: ExponentialMovingAverage) -> None:
    """
    Test handle trade tick updates indicator.
    """
    # Arrange

    tick = TestDataProviderPyo3.trade_tick()

    # Act
    ema.handle_trade_tick(tick)

    # Assert
    assert ema.has_inputs
    assert ema.value == pytest.approx(1987.0)


def test_handle_bar_updates_indicator(ema: ExponentialMovingAverage) -> None:
    """
    Test handle bar updates indicator.
    """
    # Arrange
    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    ema.handle_bar(bar)

    # Assert
    assert ema.has_inputs
    assert ema.value == 1.00003


def test_value_with_one_input_returns_expected_value(ema: ExponentialMovingAverage) -> None:
    """
    Test value with one input returns expected value.
    """
    # Arrange
    ema.update_raw(1.00000)

    # Act, Assert
    assert ema.value == 1.0


def test_value_with_three_inputs_returns_expected_value(ema: ExponentialMovingAverage) -> None:
    """
    Test value with three inputs returns expected value.
    """
    # Arrange
    ema.update_raw(1.00000)
    ema.update_raw(2.00000)
    ema.update_raw(3.00000)

    # Act, Assert
    assert ema.value == 1.5123966942148759


def test_reset_successfully_returns_indicator_to_fresh_state(ema: ExponentialMovingAverage) -> None:
    """
    Test reset successfully returns indicator to fresh state.
    """
    # Arrange
    for _i in range(1000):
        ema.update_raw(1.00000)

    # Act
    ema.reset()

    # Assert
    assert not ema.initialized
    assert ema.value == 0.0
