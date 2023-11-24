# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import WilderMovingAverage
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


@pytest.fixture(scope="function")
def rma():
    return WilderMovingAverage(10)


def test_name_returns_expected_string(rma: WilderMovingAverage):
    # Arrange, Act, Assert
    assert rma.name == "WilderMovingAverage"


def test_str_repr_returns_expected_string(rma: WilderMovingAverage):
    # Arrange, Act, Assert
    assert str(rma) == "WilderMovingAverage(10)"
    assert repr(rma) == "WilderMovingAverage(10)"


def test_period_returns_expected_value(rma: WilderMovingAverage):
    # Arrange, Act, Assert
    assert rma.period == 10


def test_multiplier_returns_expected_value(rma: WilderMovingAverage):
    # Arrange, Act, Assert
    assert rma.alpha == 0.1


def test_initialized_without_inputs_returns_false(rma: WilderMovingAverage):
    # Arrange, Act, Assert
    assert rma.initialized is False


def test_initialized_with_required_inputs_returns_true(rma: WilderMovingAverage):
    # Arrange
    rma.update_raw(1.00000)
    rma.update_raw(2.00000)
    rma.update_raw(3.00000)
    rma.update_raw(4.00000)
    rma.update_raw(5.00000)
    rma.update_raw(6.00000)
    rma.update_raw(7.00000)
    rma.update_raw(8.00000)
    rma.update_raw(9.00000)
    rma.update_raw(10.00000)

    # Act

    # Assert
    assert rma.initialized is True


def test_handle_quote_tick_updates_indicator():
    # Arrange
    indicator = WilderMovingAverage(10, PriceType.MID)

    tick = TestDataProviderPyo3.quote_tick()

    # Act
    indicator.handle_quote_tick(tick)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == 1987.5


def test_handle_trade_tick_updates_indicator(rma: WilderMovingAverage):
    # Arrange

    tick = TestDataProviderPyo3.trade_tick()

    # Act
    rma.handle_trade_tick(tick)

    # Assert
    assert rma.has_inputs
    assert rma.value == 1987.0


def test_handle_bar_updates_indicator(rma: WilderMovingAverage):
    # Arrange
    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    rma.handle_bar(bar)

    # Assert
    assert rma.has_inputs
    assert rma.value == 1.00003


def test_value_with_one_input_returns_expected_value(rma: WilderMovingAverage):
    # Arrange
    rma.update_raw(1.00000)

    # Act, Assert
    assert rma.value == 1.0


def test_value_with_three_inputs_returns_expected_value(rma: WilderMovingAverage):
    # Arrange
    rma.update_raw(1.00000)
    rma.update_raw(2.00000)
    rma.update_raw(3.00000)

    # Act, Assert
    assert rma.value == 1.29


def test_value_with_ten_inputs_returns_expected_value(rma: WilderMovingAverage):
    # Arrange
    rma.update_raw(1.0)
    rma.update_raw(2.0)
    rma.update_raw(3.0)
    rma.update_raw(4.0)
    rma.update_raw(5.0)
    rma.update_raw(6.0)
    rma.update_raw(7.0)
    rma.update_raw(8.0)
    rma.update_raw(9.0)
    rma.update_raw(10.0)

    # Act, Assert
    assert rma.value == 4.486784401


def test_reset_successfully_returns_indicator_to_fresh_state(rma: WilderMovingAverage):
    # Arrange
    for _i in range(10):
        rma.update_raw(1.00000)

    # Act
    rma.reset()

    # Assert
    assert not rma.initialized
    assert rma.value == 0.0
