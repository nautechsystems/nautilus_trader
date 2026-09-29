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
Test rate of change behavior.
"""

import pytest

from nautilus_trader.indicators import RateOfChange
from nautilus_trader.indicators import RateOfChangeMode


@pytest.mark.parametrize(
    ("mode", "expected"),
    [
        (RateOfChangeMode.Percentage, 100.0 * (121.0 / 100.0 - 1.0)),
        (RateOfChangeMode.Fraction, 121.0 / 100.0 - 1.0),
        (RateOfChangeMode.Ratio, 121.0 / 100.0),
        (RateOfChangeMode.RatioPercent, 100.0 * (121.0 / 100.0)),
    ],
)
def test_mode_selects_output_convention(mode: RateOfChangeMode, expected: float) -> None:
    """
    Test each mode reports the price change over exactly period updates.
    """
    # Arrange
    roc = RateOfChange(2, mode=mode)

    # Act
    for price in (100.0, 110.0, 121.0):
        roc.update_raw(price)

    # Assert
    assert roc.initialized
    assert roc.value == expected


def test_default_mode_is_percentage() -> None:
    """
    Test the default output is a percentage.
    """
    # Arrange
    roc = RateOfChange(1)

    # Act
    roc.update_raw(50.0)
    roc.update_raw(75.0)

    # Assert
    assert roc.value == 100.0 * (75.0 / 50.0 - 1.0)
