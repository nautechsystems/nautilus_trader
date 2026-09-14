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
Test zscore behavior.
"""

from collections import deque
from math import isfinite
from math import isnan
from math import sqrt

import pytest

from nautilus_trader.indicators import ZScore
from nautilus_trader.model import PriceType
from tests.stubs import TestDataProviderPyo3


def _batch_zscore(window: list[float]) -> tuple[float, float, float]:
    n = len(window)
    mean = sum(window) / n
    m2 = sum((x - mean) ** 2 for x in window)
    std = (m2 / (n - 1)) ** 0.5
    x = window[-1]
    is_constant = all(isfinite(value) and value == x for value in window)
    z = 0.0 if is_constant or std == 0.0 else (x - mean) / std
    return mean, std, z


@pytest.fixture
def zscore() -> ZScore:
    """
    Z-score.
    """
    return ZScore(10)


def test_zscore_name(zscore: ZScore) -> None:
    """
    Test zscore name.
    """
    assert zscore.name == "ZScore"


def test_str_repr_returns_expected_string(zscore: ZScore) -> None:
    """
    Test str repr returns expected string.
    """
    # Arrange, Act, Assert
    assert str(zscore) == "ZScore(10)"
    assert repr(zscore) == "ZScore(10)"


def test_period_returns_expected_value(zscore: ZScore) -> None:
    """
    Test period returns expected value.
    """
    # Arrange, Act, Assert
    assert zscore.period == 10


def test_invalid_period_raises_value_error() -> None:
    """
    Test invalid period raises value error.
    """
    with pytest.raises(ValueError, match="`period` must be at least 2"):
        ZScore(1)
    with pytest.raises(ValueError, match="`period` must be at least 2"):
        ZScore(0)
    with pytest.raises(ValueError, match="`period` must be at least 2"):
        ZScore(-1)


def test_price_type_readback() -> None:
    """
    Test price type readback.
    """
    # Arrange, Act, Assert
    assert ZScore(10, PriceType.ASK).price_type == PriceType.ASK
    assert ZScore(10).price_type == PriceType.LAST


def test_initialized_without_inputs_returns_false(zscore: ZScore) -> None:
    """
    Test initialized without inputs returns false.
    """
    # Arrange, Act, Assert
    assert not zscore.initialized


def test_initialized_with_required_inputs_returns_true(zscore: ZScore) -> None:
    """
    Test initialized with required inputs returns true.
    """
    # Arrange
    for i in range(1, 11):
        zscore.update_raw(float(i))

    # Act, Assert
    assert zscore.initialized
    assert zscore.count == 10
    assert zscore.mean == pytest.approx(5.5)


def test_value_with_one_input_returns_expected_value(zscore: ZScore) -> None:
    """
    Test value with one input returns expected value.
    """
    # Arrange
    zscore.update_raw(2.0)

    # Act, Assert
    assert not zscore.initialized
    assert zscore.count == 1
    assert zscore.mean == 2.0
    assert zscore.std == 0.0
    assert zscore.value == 0.0


def test_value_with_two_inputs_uses_expanding_window() -> None:
    """
    Test value with two inputs uses expanding window.
    """
    # Arrange
    indicator = ZScore(5)

    # Act
    indicator.update_raw(2.0)
    indicator.update_raw(4.0)

    # Assert
    assert not indicator.initialized
    assert indicator.count == 2
    assert indicator.mean == 3.0
    assert indicator.std == pytest.approx(sqrt(2.0))
    assert indicator.value == pytest.approx(1.0 / sqrt(2.0))


def test_value_transitions_from_expanding_to_rolling() -> None:
    """
    Test value transitions from expanding to rolling.
    """
    # Arrange
    indicator = ZScore(3)

    # Act
    indicator.update_raw(2.0)
    indicator.update_raw(4.0)
    indicator.update_raw(6.0)

    # Assert
    assert indicator.initialized
    assert indicator.count == 3
    assert indicator.mean == 4.0
    assert indicator.std == 2.0
    assert indicator.value == 1.0

    indicator.update_raw(8.0)
    assert indicator.count == 3
    assert indicator.mean == 6.0
    assert indicator.std == 2.0
    assert indicator.value == 1.0


@pytest.mark.parametrize(
    ("period", "value"),
    [
        (4, 3.0),
        (10, 1.00003),
        (20, 0.1),
    ],
)
def test_constant_series_is_zero(period: int, value: float) -> None:
    """
    Test constant inputs produce a zero z-score.
    """
    # Arrange
    indicator = ZScore(period)

    # Act
    for _ in range(period):
        indicator.update_raw(value)

    # Assert
    assert indicator.value == 0.0


def test_constant_bars_produce_zero_zscore() -> None:
    """
    Test repeated bars with a non-exact close produce a zero z-score.
    """
    # Arrange
    indicator = ZScore(10)
    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    for _ in range(10):
        indicator.handle_bar(bar)

    # Assert
    assert indicator.initialized
    assert indicator.value == 0.0


def test_non_finite_input_propagates_to_value() -> None:
    """
    Test non-finite inputs do not produce a neutral z-score.
    """
    # Arrange
    indicator = ZScore(2)

    # Act
    indicator.update_raw(1.0)
    indicator.update_raw(float("nan"))

    # Assert
    assert isnan(indicator.std)
    assert isnan(indicator.value)


def test_handle_quote_tick_updates_indicator() -> None:
    """
    Test handle quote tick updates indicator.
    """
    # Arrange
    indicator = ZScore(10, PriceType.MID)
    tick = TestDataProviderPyo3.quote_tick()

    # Act
    indicator.handle_quote_tick(tick)

    # Assert
    assert indicator.has_inputs
    assert indicator.mean == 1987.5
    assert indicator.value == 0.0


def test_handle_trade_tick_updates_indicator() -> None:
    """
    Test handle trade tick updates indicator.
    """
    # Arrange
    indicator = ZScore(10)
    tick = TestDataProviderPyo3.trade_tick()

    # Act
    indicator.handle_trade_tick(tick)

    # Assert
    assert indicator.has_inputs
    assert indicator.mean == 1987.0
    assert indicator.value == 0.0


def test_handle_bar_updates_indicator() -> None:
    """
    Test handle bar updates indicator.
    """
    # Arrange
    indicator = ZScore(10)
    bar = TestDataProviderPyo3.bar_5decimal()

    # Act
    indicator.handle_bar(bar)

    # Assert
    assert indicator.has_inputs
    assert indicator.mean == 1.00003
    assert indicator.value == 0.0


def test_reset_successfully_returns_indicator_to_fresh_state(zscore: ZScore) -> None:
    """
    Test reset successfully returns indicator to fresh state.
    """
    # Arrange
    for _i in range(1000):
        zscore.update_raw(1.0)

    # Act
    zscore.reset()

    # Assert
    assert not zscore.initialized
    assert not zscore.has_inputs
    assert zscore.value == 0.0
    assert zscore.mean == 0.0
    assert zscore.std == 0.0
    assert zscore.count == 0


@pytest.mark.parametrize("period", [2, 5, 10])
def test_expanding_then_rolling_matches_batch_window(period: int) -> None:
    """
    Test expanding then rolling z-score matches a batch window.
    """
    # Arrange
    values = [100.0 + ((i * 17) % 50) - 10.0 + (i % 7) * 0.25 for i in range(40)]
    indicator = ZScore(period)
    window: deque[float] = deque(maxlen=period)

    # Act, Assert
    for x in values:
        window.append(x)
        indicator.update_raw(x)
        if len(window) < 2:
            assert indicator.value == 0.0
            continue
        mean, std, batch_z = _batch_zscore(list(window))
        assert indicator.mean == pytest.approx(mean, rel=1e-9, abs=1e-12)
        assert indicator.std == pytest.approx(std, rel=1e-9, abs=1e-12)
        assert indicator.value == pytest.approx(batch_z, rel=1e-9, abs=1e-12)
        assert indicator.initialized == (len(window) == period)
