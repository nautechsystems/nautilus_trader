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

import pytest

from nautilus_trader.indicators import RelativeStrengthIndex
from tests.stubs import TestDataProviderPyo3


@pytest.fixture
def rsi() -> RelativeStrengthIndex:
    return RelativeStrengthIndex(10)


def test_rsi(rsi: RelativeStrengthIndex) -> None:
    assert rsi.name == "RelativeStrengthIndex"


def test_str_repr_returns_expected_string(rsi: RelativeStrengthIndex) -> None:
    # Arrange, Act, Assert
    assert str(rsi) == "RelativeStrengthIndex(10, EXPONENTIAL)"
    assert repr(rsi) == "RelativeStrengthIndex(10, EXPONENTIAL)"


def test_period_returns_expected_value(rsi: RelativeStrengthIndex) -> None:
    # Arrange, Act, Assert
    assert rsi.period == 10


def test_initialized_without_inputs_returns_false(rsi: RelativeStrengthIndex) -> None:
    # Arrange, Act, Assert
    assert not rsi.initialized


def test_initialized_with_required_inputs_returns_true(rsi: RelativeStrengthIndex) -> None:
    # Arrange
    rsi.update_raw(1.0)
    rsi.update_raw(2.0)
    rsi.update_raw(3.0)
    rsi.update_raw(4.0)
    rsi.update_raw(5.0)
    rsi.update_raw(6.0)
    rsi.update_raw(7.0)
    rsi.update_raw(8.0)
    rsi.update_raw(9.0)
    rsi.update_raw(10.0)

    # Act, Assert
    assert rsi.initialized


def test_handle_bar_updates_indicator() -> None:
    # Arrange
    indicator = RelativeStrengthIndex(10)

    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    indicator.handle_bar(bar)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == 1.0


def test_value_with_one_input_returns_expected_value(rsi: RelativeStrengthIndex) -> None:
    # Arrange
    rsi.update_raw(1.00000)

    # Act, Assert
    assert rsi.value == 1


def test_value_with_all_higher_inputs_returns_expected_value(rsi: RelativeStrengthIndex) -> None:
    # Arrange
    rsi.update_raw(1.00000)
    rsi.update_raw(2.00000)
    rsi.update_raw(3.00000)
    rsi.update_raw(4.00000)

    # Act, Assert
    assert rsi.value == 1


def test_value_with_all_lower_inputs_returns_expected_value(rsi: RelativeStrengthIndex) -> None:
    # Arrange
    rsi.update_raw(3.00000)
    rsi.update_raw(2.00000)
    rsi.update_raw(1.00000)
    rsi.update_raw(0.50000)

    # Act, Assert
    assert rsi.value == 0


def test_value_with_various_inputs_returns_expected_value(rsi: RelativeStrengthIndex) -> None:
    # Arrange
    rsi.update_raw(3.00000)
    rsi.update_raw(2.00000)
    rsi.update_raw(5.00000)
    rsi.update_raw(6.00000)
    rsi.update_raw(7.00000)
    rsi.update_raw(6.00000)

    # Act, Assert
    assert rsi.value == 0.6837363325825265


def test_value_at_returns_expected_value(rsi: RelativeStrengthIndex) -> None:
    # Arrange
    rsi.update_raw(3.00000)
    rsi.update_raw(2.00000)
    rsi.update_raw(5.00000)
    rsi.update_raw(6.00000)
    rsi.update_raw(7.00000)
    rsi.update_raw(6.00000)
    rsi.update_raw(6.00000)
    rsi.update_raw(7.00000)

    # Act, Assert
    assert rsi.value == 0.7615344667662725


def test_min_value_as_first(rsi: RelativeStrengthIndex) -> None:
    # Arrange
    rsi.update_raw(1.00000)
    rsi.update_raw(2.00000)
    rsi.update_raw(3.00000)
    rsi.update_raw(4.00000)
    rsi.update_raw(5.00000)
    rsi.update_raw(6.00000)
    rsi.update_raw(7.00000)
    rsi.update_raw(2.00000)

    # Act, Assert
    assert rsi.value == 0.38650828748031707


def test_reset_successfully_returns_indicator_to_fresh_state(rsi: RelativeStrengthIndex) -> None:
    # Arrange
    rsi.update_raw(1.00020)
    rsi.update_raw(1.00030)
    rsi.update_raw(1.00050)

    # Act
    rsi.reset()

    # Assert
    assert not rsi.initialized
    assert rsi.value == 0
