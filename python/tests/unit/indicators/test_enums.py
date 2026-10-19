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
Test indicator enum behavior.
"""

import pytest

from nautilus_trader.indicators import CandleBodySize
from nautilus_trader.indicators import CandleDirection
from nautilus_trader.indicators import CandleSize
from nautilus_trader.indicators import CandleWickSize
from nautilus_trader.indicators import MovingAverageType
from nautilus_trader.indicators import RateOfChangeMode
from nautilus_trader.indicators import StochasticsDMethod


@pytest.mark.parametrize(
    ("member", "value"),
    [
        (RateOfChangeMode.Percentage, 0),
        (RateOfChangeMode.Fraction, 1),
        (RateOfChangeMode.Ratio, 2),
        (RateOfChangeMode.RatioPercent, 3),
        (RateOfChangeMode.Log, 4),
        (MovingAverageType.Simple, 0),
        (MovingAverageType.Exponential, 1),
        (MovingAverageType.DoubleExponential, 2),
        (MovingAverageType.Wilder, 3),
        (MovingAverageType.Hull, 4),
        (StochasticsDMethod.Ratio, 0),
        (StochasticsDMethod.MovingAverage, 1),
        (getattr(CandleBodySize, "None"), 0),
        (CandleBodySize.Small, 1),
        (CandleBodySize.Medium, 2),
        (CandleBodySize.Large, 3),
        (CandleBodySize.Trend, 4),
        (CandleDirection.Bull, 1),
        (getattr(CandleDirection, "None"), 0),
        (CandleDirection.Bear, -1),
        (getattr(CandleSize, "None"), 0),
        (CandleSize.VerySmall, 1),
        (CandleSize.Small, 2),
        (CandleSize.Medium, 3),
        (CandleSize.Large, 4),
        (CandleSize.VeryLarge, 5),
        (CandleSize.ExtremelyLarge, 6),
        (getattr(CandleWickSize, "None"), 0),
        (CandleWickSize.Small, 1),
        (CandleWickSize.Medium, 2),
        (CandleWickSize.Large, 3),
    ],
)
def test_enum_hash_matches_equal_int(member: object, value: int) -> None:
    """
    Test members hash like the integers they compare equal to, so mixed keys resolve.
    """
    # Arrange
    keys: dict[object, str] = {value: "int"}

    # Act
    resolved = keys.get(member)

    # Assert
    assert member == value
    assert hash(member) == hash(value)
    assert resolved == "int"
    assert len({member, value}) == 1
