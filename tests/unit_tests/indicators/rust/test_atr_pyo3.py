# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import sys

import pytest

from nautilus_trader.core.nautilus_pyo3 import AverageTrueRange
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


@pytest.fixture(scope="function")
def atr() -> AverageTrueRange:
    return AverageTrueRange(10)


def test_name_returns_expected_string(atr: AverageTrueRange) -> None:
    # Arrange, Act, Assert
    assert atr.name == "AverageTrueRange"


def test_str_repr_returns_expected_string(atr: AverageTrueRange) -> None:
    # Arrange, Act, Assert
    assert str(atr) == "AverageTrueRange(10,SIMPLE,true,0)"
    assert repr(atr) == "AverageTrueRange(10,SIMPLE,true,0)"


def test_period(atr: AverageTrueRange) -> None:
    # Arrange, Act, Assert
    assert atr.period == 10


def test_initialized_without_inputs_returns_false(atr: AverageTrueRange) -> None:
    # Arrange, Act, Assert
    assert atr.initialized is False


def test_initialized_with_required_inputs_returns_true(atr: AverageTrueRange) -> None:
    # Arrange, Act
    for _i in range(10):
        atr.update_raw(1.00000, 1.00000, 1.00000)

    # Assert
    assert atr.initialized is True


def test_handle_bar_updates_indicator(atr: AverageTrueRange) -> None:
    # Arrange
    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    atr.handle_bar(bar)

    # Assert
    assert atr.has_inputs
    assert atr.value == 2.999999999997449e-05


def test_value_with_no_inputs_returns_zero(atr: AverageTrueRange) -> None:
    # Arrange, Act, Assert
    assert atr.value == 0.0


def test_value_with_epsilon_input(atr: AverageTrueRange) -> None:
    # Arrange
    epsilon = sys.float_info.epsilon
    atr.update_raw(epsilon, epsilon, epsilon)

    # Act, Assert
    assert atr.value == 0.0


def test_value_with_one_ones_input(atr: AverageTrueRange) -> None:
    # Arrange
    atr.update_raw(1.00000, 1.00000, 1.00000)

    # Act, Assert
    assert atr.value == 0.0


def test_value_with_one_input(atr: AverageTrueRange) -> None:
    # Arrange
    atr.update_raw(1.00020, 1.00000, 1.00010)

    # Act, Assert
    assert atr.value == pytest.approx(0.00020)


def test_value_with_three_inputs(atr: AverageTrueRange) -> None:
    # Arrange
    atr.update_raw(1.00020, 1.00000, 1.00010)
    atr.update_raw(1.00020, 1.00000, 1.00010)
    atr.update_raw(1.00020, 1.00000, 1.00010)

    # Act, Assert
    assert atr.value == pytest.approx(0.00020)


def test_value_with_close_on_high(atr: AverageTrueRange) -> None:
    # Arrange
    high = 1.00010
    low = 1.00000

    # Act
    for _i in range(1000):
        high += 0.00010
        low += 0.00010
        close = high
        atr.update_raw(high, low, close)

    # Assert
    assert atr.value == pytest.approx(0.00010, 2)


def test_value_with_close_on_low(atr: AverageTrueRange) -> None:
    # Arrange
    high = 1.00010
    low = 1.00000

    # Act
    for _i in range(1000):
        high -= 0.00010
        low -= 0.00010
        close = low
        atr.update_raw(high, low, close)

    # Assert
    assert atr.value == pytest.approx(0.00010)


def test_floor_with_ten_ones_inputs() -> None:
    # Arrange
    floor = 0.00005
    floored_atr = AverageTrueRange(10, value_floor=floor)

    for _i in range(20):
        floored_atr.update_raw(1.00000, 1.00000, 1.00000)

    # Act, Assert
    assert floored_atr.value == 5e-05


def test_floor_with_exponentially_decreasing_high_inputs() -> None:
    # Arrange
    floor = 0.00005
    floored_atr = AverageTrueRange(10, value_floor=floor)

    high = 1.00020
    low = 1.00000
    close = 1.00000

    for _i in range(20):
        high -= (high - low) / 2
        floored_atr.update_raw(high, low, close)

    # Act, Assert
    assert floored_atr.value == 5e-05


def test_reset_successfully_returns_indicator_to_fresh_state(atr: AverageTrueRange) -> None:
    # Arrange
    for _i in range(1000):
        atr.update_raw(1.00010, 1.00000, 1.00005)

    # Act
    atr.reset()

    # Assert
    assert not atr.initialized
    assert atr.value == 0
