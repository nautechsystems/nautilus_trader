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
Test MACD behavior.
"""

import pytest

from nautilus_trader.indicators import MovingAverageConvergenceDivergence
from nautilus_trader.indicators import MovingAverageType


def test_signal_and_histogram_follow_macd_line() -> None:
    """
    Test the signal averages the MACD line and the histogram is their difference.
    """
    # Arrange
    macd = MovingAverageConvergenceDivergence(2, 3, 2, MovingAverageType.Simple)

    # Act
    for price in (1.0, 2.0, 4.0, 7.0):
        macd.update_raw(price)
        if price < 7.0:
            assert not macd.initialized

    # Assert
    assert macd.initialized
    assert macd.value == pytest.approx(7.0 / 6.0)
    assert macd.signal == pytest.approx(11.0 / 12.0)
    assert macd.histogram == pytest.approx(0.25)


def test_signal_period_defaults_to_nine() -> None:
    """
    Test the default signal period.
    """
    # Arrange, Act
    macd = MovingAverageConvergenceDivergence(2, 3)

    # Assert
    assert macd.signal_period == 9


def test_invalid_periods_raise_value_error() -> None:
    """
    Test the fast period must be shorter than the slow period.
    """
    with pytest.raises(ValueError, match="fast_period must be > 0 and < slow_period"):
        MovingAverageConvergenceDivergence(3, 3)
