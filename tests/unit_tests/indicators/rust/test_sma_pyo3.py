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
from nautilus_trader.core.nautilus_pyo3 import SimpleMovingAverage
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


@pytest.fixture(scope="function")
def sma():
    return SimpleMovingAverage(10)


def test_sma(sma: SimpleMovingAverage):
    assert sma.name == "SimpleMovingAverage"


def test_str_repr_returns_expected_string(sma: SimpleMovingAverage):
    # Arrange, Act, Assert
    assert str(sma) == "SimpleMovingAverage(10)"
    assert repr(sma) == "SimpleMovingAverage(10)"


def test_period_returns_expected_value(sma: SimpleMovingAverage):
    # Arrange, Act, Assert
    assert sma.period == 10


def test_initialized_without_inputs_returns_false(sma: SimpleMovingAverage):
    # Arrange, Act, Assert
    assert sma.initialized is False


def test_initialized_with_required_inputs_returns_true(sma: SimpleMovingAverage):
    # Arrange
    sma.update_raw(1.0)
    sma.update_raw(2.0)
    sma.update_raw(3.0)
    sma.update_raw(4.0)
    sma.update_raw(5.0)
    sma.update_raw(6.0)
    sma.update_raw(7.0)
    sma.update_raw(8.0)
    sma.update_raw(9.0)
    sma.update_raw(10.0)

    # Act, Assert
    assert sma.initialized is True
    assert sma.count == 10
    assert sma.value == 5.5


def test_handle_quote_tick_updates_indicator(sma: SimpleMovingAverage):
    # Arrange
    indicator = SimpleMovingAverage(10, PriceType.MID)

    tick = TestDataProviderPyo3.quote_tick()

    # Act
    indicator.handle_quote_tick(tick)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == 1987.5


def test_handle_trade_tick_updates_indicator(sma: SimpleMovingAverage):
    # Arrange
    indicator = SimpleMovingAverage(10)

    tick = TestDataProviderPyo3.trade_tick()

    # Act
    indicator.handle_trade_tick(tick)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == 1987.0


def test_handle_bar_updates_indicator(sma: SimpleMovingAverage):
    # Arrange
    indicator = SimpleMovingAverage(10)

    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    indicator.handle_bar(bar)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == 1.00003


def test_value_with_one_input_returns_expected_value(sma: SimpleMovingAverage):
    # Arrange
    sma.update_raw(1.0)

    # Act, Assert
    assert sma.value == 1.0


def test_value_with_three_inputs_returns_expected_value(sma: SimpleMovingAverage):
    # Arrange
    sma.update_raw(1.0)
    sma.update_raw(2.0)
    sma.update_raw(3.0)

    # Act, Assert
    assert sma.value == 2.0


def test_value_at_returns_expected_value(sma: SimpleMovingAverage):
    # Arrange
    sma.update_raw(1.0)
    sma.update_raw(2.0)
    sma.update_raw(3.0)

    # Act, Assert
    assert sma.value == 2.0


def test_handle_quote_tick_updates_with_expected_value(sma: SimpleMovingAverage):
    # Arrange
    sma_for_ticks1 = SimpleMovingAverage(10, PriceType.ASK)
    sma_for_ticks2 = SimpleMovingAverage(10, PriceType.MID)
    sma_for_ticks3 = SimpleMovingAverage(10, PriceType.BID)

    tick = TestDataProviderPyo3.quote_tick(
        bid_price=1.00001,
        ask_price=1.00003,
    )

    # Act
    sma_for_ticks1.handle_quote_tick(tick)
    sma_for_ticks2.handle_quote_tick(tick)
    sma_for_ticks3.handle_quote_tick(tick)

    # Assert
    assert sma_for_ticks1.has_inputs
    assert sma_for_ticks2.has_inputs
    assert sma_for_ticks3.has_inputs
    assert sma_for_ticks1.value == 1.00003
    assert sma_for_ticks2.value == 1.00002
    assert sma_for_ticks3.value == 1.00001


def test_handle_trade_tick_updates_with_expected_value(sma: SimpleMovingAverage):
    # Arrange
    sma_for_ticks = SimpleMovingAverage(10)

    tick = TestDataProviderPyo3.trade_tick()

    # Act
    sma_for_ticks.handle_trade_tick(tick)

    # Assert
    assert sma_for_ticks.has_inputs
    assert sma_for_ticks.value == 1987.0


def test_reset_successfully_returns_indicator_to_fresh_state(sma: SimpleMovingAverage):
    # Arrange
    for _i in range(1000):
        sma.update_raw(1.0)

    # Act
    sma.reset()

    # Assert
    assert not sma.initialized
    assert sma.value == 0
