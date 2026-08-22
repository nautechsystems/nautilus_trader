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

import sys

import pytest

from nautilus_trader.indicators import AdaptiveMovingAverage
from nautilus_trader.indicators import EfficiencyRatio
from nautilus_trader.model import Bar
from nautilus_trader.model import Currency
from nautilus_trader.model import CurrencyType
from nautilus_trader.model import Money
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from tests.stubs import TestDataProviderPyo3


HIGH_PRECISION_CURRENCY = Currency(
    code="ERP",
    precision=18,
    iso4217=0,
    name="Efficiency ratio precision test",
    currency_type=CurrencyType.CRYPTO,
)


@pytest.fixture
def er() -> EfficiencyRatio:
    return EfficiencyRatio(10)


def test_er(er: EfficiencyRatio) -> None:
    assert er.name == "EfficiencyRatio"


@pytest.mark.parametrize("period", [0, 1])
def test_invalid_period_raises_value_error(period: int) -> None:
    with pytest.raises(ValueError, match="`period` must be at least 2"):
        EfficiencyRatio(period)


@pytest.mark.parametrize("period", [0, 1])
def test_adaptive_moving_average_rejects_invalid_er_period(period: int) -> None:
    with pytest.raises(ValueError, match="`period` must be at least 2"):
        AdaptiveMovingAverage(period, 2, 30)


def test_adaptive_moving_average_rejects_max_slow_period() -> None:
    maximum = 2 * sys.maxsize + 1

    with pytest.raises(ValueError, match="`period_slow` must be less") as exc_info:
        AdaptiveMovingAverage(2, maximum - 1, maximum)

    assert str(exc_info.value) == "`period_slow` must be less than `usize::MAX`"


def test_str_repr_returns_expected_string(er: EfficiencyRatio) -> None:
    # Arrange, Act, Assert
    assert str(er) == "EfficiencyRatio(10)"
    assert repr(er) == "EfficiencyRatio(10)"


def test_period_returns_expected_value(er: EfficiencyRatio) -> None:
    # Arrange, Act, Assert
    assert er.period == 10


def test_initialized_without_inputs_returns_false(er: EfficiencyRatio) -> None:
    # Arrange, Act, Assert
    assert not er.initialized


def test_initialized_with_required_inputs_returns_true(er: EfficiencyRatio) -> None:
    # Arrange, Act
    for _ in range(10):
        er.update_raw(1.00000)

    # Assert
    assert er.initialized


def test_handle_bar_updates_indicator(er: EfficiencyRatio) -> None:
    # Arrange
    er = EfficiencyRatio(10)

    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    er.handle_bar(bar)

    # Assert
    assert er.has_inputs
    assert er.value == 0


def test_handle_bar_rejects_price_above_float_precision(er: EfficiencyRatio) -> None:
    source = TestDataProviderPyo3.bar_5decimal()
    price = Price.from_raw(1_000_000_000_000_000_000, 18)
    bar = Bar(
        bar_type=source.bar_type,
        open=price,
        high=price,
        low=price,
        close=price,
        volume=source.volume,
        ts_event=source.ts_event,
        ts_init=source.ts_init,
    )

    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        er.handle_bar(bar)

    assert str(exc_info.value) == "Fixed-point precision 18 exceeds maximum float precision 16"


@pytest.mark.parametrize(
    "value",
    [
        pytest.param(Price.from_raw(1_000_000_000_000_000_000, 18), id="price"),
        pytest.param(Quantity.from_raw(1_000_000_000_000_000_000, 18), id="quantity"),
        pytest.param(
            Money.from_raw(1_000_000_000_000_000_000, HIGH_PRECISION_CURRENCY),
            id="money",
        ),
    ],
)
def test_update_raw_rejects_fixed_value_above_float_precision(
    er: EfficiencyRatio,
    value: Price | Quantity | Money,
) -> None:
    with pytest.raises(ValueError, match="maximum float precision") as exc_info:
        er.update_raw(value)

    assert str(exc_info.value) == "Fixed-point precision 18 exceeds maximum float precision 16"


def test_value_with_one_input(er: EfficiencyRatio) -> None:
    # Arrange
    er.update_raw(1.00000)

    # Act, Assert
    assert er.value == 0.0


def test_value_with_efficient_higher_inputs(er: EfficiencyRatio) -> None:
    # Arrange
    initial_price = 1.00000

    # Act
    for _ in range(10):
        initial_price += 0.00001
        er.update_raw(initial_price)

    # Assert
    assert er.value == 1.0


def test_value_with_efficient_lower_inputs(er: EfficiencyRatio) -> None:
    # Arrange
    initial_price = 1.00000

    # Act
    for _ in range(10):
        initial_price -= 0.00001
        er.update_raw(initial_price)

    # Assert
    assert er.value == 1.0


def test_value_with_oscillating_inputs_returns_zero(er: EfficiencyRatio) -> None:
    # Arrange
    er.update_raw(1.00000)
    er.update_raw(1.00010)
    er.update_raw(1.00000)
    er.update_raw(0.99990)
    er.update_raw(1.00000)

    # Act, Assert
    assert er.value == 0.0


def test_value_with_half_oscillating_inputs_returns_zero(er: EfficiencyRatio) -> None:
    # Arrange
    er.update_raw(1.00000)
    er.update_raw(1.00020)
    er.update_raw(1.00010)
    er.update_raw(1.00030)
    er.update_raw(1.00020)

    # Act, Assert
    assert er.value == 0.3333333333333333


def test_value_with_noisy_inputs(er: EfficiencyRatio) -> None:
    # Arrange
    er.update_raw(1.00000)
    er.update_raw(1.00010)
    er.update_raw(1.00008)
    er.update_raw(1.00007)
    er.update_raw(1.00012)
    er.update_raw(1.00005)
    er.update_raw(1.00015)

    # Act, Assert
    assert er.value == 0.42857142857215363


@pytest.mark.parametrize(
    ("prices", "expected"),
    [
        ([10.0, 11.0, 12.0, 13.0, 14.0, 15.0], 1.0),
        ([15.0, 14.0, 13.0, 12.0, 11.0, 10.0], 1.0),
        ([10.0, 12.0, 10.0, 12.0, 10.0, 12.0], 0.2),
        ([10.0, 11.0, 10.5, 12.0, 11.5, 13.0], 0.6),
        ([10.0, 10.0, 10.0, 10.0, 10.0, 10.0], 0.0),
    ],
)
def test_value_uses_period_deltas(prices: list[float], expected: float) -> None:
    er = EfficiencyRatio(5)

    for price in prices:
        er.update_raw(price)

    assert er.value == expected


def test_reset_successfully_returns_indicator_to_fresh_state(er: EfficiencyRatio) -> None:
    # Arrange
    for _ in range(10):
        er.update_raw(1.00000)

    # Act
    er.reset()

    # Assert
    assert not er.initialized
    assert er.value == 0
