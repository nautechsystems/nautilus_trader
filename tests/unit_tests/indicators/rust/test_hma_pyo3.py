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
from nautilus_trader.core.nautilus_pyo3 import HullMovingAverage
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3


@pytest.fixture(scope="function")
def hma():
    return HullMovingAverage(10)


def test_hma(hma: HullMovingAverage):
    assert hma.name == "HullMovingAverage"


def test_str_repr_returns_expected_string(hma: HullMovingAverage):
    # Arrange, Act, Assert
    assert str(hma) == "HullMovingAverage(10)"
    assert repr(hma) == "HullMovingAverage(10)"


def test_period_returns_expected_value(hma: HullMovingAverage):
    # Arrange, Act, Assert
    assert hma.period == 10


def test_initialized_without_inputs_returns_false(hma: HullMovingAverage):
    # Arrange, Act, Assert
    assert hma.initialized is False


def test_initialized_with_required_inputs_returns_true(hma: HullMovingAverage):
    # Arrange
    hma.update_raw(1.00000)
    hma.update_raw(1.00010)
    hma.update_raw(1.00020)
    hma.update_raw(1.00030)
    hma.update_raw(1.00040)
    hma.update_raw(1.00050)
    hma.update_raw(1.00040)
    hma.update_raw(1.00030)
    hma.update_raw(1.00020)
    hma.update_raw(1.00010)
    hma.update_raw(1.00000)

    # Act, Assert
    assert hma.initialized is True
    assert hma.count == 11
    assert hma.value == 1.0001403928170598


def test_handle_quote_tick_updates_indicator(hma: HullMovingAverage):
    # Arrange
    indicator = HullMovingAverage(10, PriceType.MID)

    tick = TestDataProviderPyo3.quote_tick()

    # Act
    indicator.handle_quote_tick(tick)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == 1987.5


def test_handle_trade_tick_updates_indicator(hma: HullMovingAverage):
    # Arrange
    indicator = HullMovingAverage(10)

    tick = TestDataProviderPyo3.trade_tick()

    # Act
    indicator.handle_trade_tick(tick)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == 1987.0


def test_handle_bar_updates_indicator(hma: HullMovingAverage):
    # Arrange
    indicator = HullMovingAverage(10)

    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    indicator.handle_bar(bar)

    # Assert
    assert indicator.has_inputs
    assert indicator.value == 1.00003


def test_value_with_one_input_returns_expected_value(hma: HullMovingAverage):
    # Arrange
    hma.update_raw(1.0)

    # Act, Assert
    assert hma.value == 1.0


def test_value_with_three_inputs_returns_expected_value(hma: HullMovingAverage):
    # Arrange
    hma.update_raw(1.0)
    hma.update_raw(2.0)
    hma.update_raw(3.0)

    # Act, Assert
    assert hma.value == 1.824561403508772


def test_handle_quote_tick_updates_with_expected_value(hma: HullMovingAverage):
    # Arrange
    hma_for_ticks1 = HullMovingAverage(10, PriceType.ASK)
    hma_for_ticks2 = HullMovingAverage(10, PriceType.MID)
    hma_for_ticks3 = HullMovingAverage(10, PriceType.BID)

    tick = TestDataProviderPyo3.quote_tick(
        bid_price=1.00001,
        ask_price=1.00003,
    )

    # Act
    hma_for_ticks1.handle_quote_tick(tick)
    hma_for_ticks2.handle_quote_tick(tick)
    hma_for_ticks3.handle_quote_tick(tick)

    # Assert
    assert hma_for_ticks1.has_inputs
    assert hma_for_ticks2.has_inputs
    assert hma_for_ticks3.has_inputs
    assert hma_for_ticks1.value == 1.00003
    assert hma_for_ticks2.value == 1.00002
    assert hma_for_ticks3.value == 1.00001


def test_handle_trade_tick_updates_with_expected_value(hma: HullMovingAverage):
    # Arrange
    hma_for_ticks = HullMovingAverage(10)

    tick = TestDataProviderPyo3.trade_tick()

    # Act
    hma_for_ticks.handle_trade_tick(tick)

    # Assert
    assert hma_for_ticks.has_inputs
    assert hma_for_ticks.value == 1987.0


def test_reset_successfully_returns_indicator_to_fresh_state(hma: HullMovingAverage):
    # Arrange
    for _i in range(1000):
        hma.update_raw(1.0)

    # Act
    hma.reset()

    # Assert
    assert not hma.initialized
    assert hma.value == 0
