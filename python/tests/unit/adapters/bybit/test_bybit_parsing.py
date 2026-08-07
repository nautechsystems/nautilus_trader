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

from nautilus_trader.adapters.bybit import bybit_bar_spec_to_interval
from nautilus_trader.model import BarAggregation


@pytest.mark.parametrize(
    ("aggregation", "step", "expected"),
    [
        (BarAggregation.MINUTE, 1, "1"),
        (BarAggregation.MINUTE, 30, "30"),
        (BarAggregation.HOUR, 12, "720"),
        (BarAggregation.DAY, 1, "D"),
        (BarAggregation.WEEK, 1, "W"),
        (BarAggregation.MONTH, 1, "M"),
    ],
)
def test_bybit_bar_spec_to_interval_accepts_bar_aggregation(
    aggregation: BarAggregation,
    step: int,
    expected: str,
) -> None:
    assert bybit_bar_spec_to_interval(aggregation, step) == expected


def test_bybit_bar_spec_to_interval_rejects_int_aggregation() -> None:
    # The raw discriminant is the shape the Cython-era signature accepted
    with pytest.raises(TypeError, match="BarAggregation"):
        bybit_bar_spec_to_interval(BarAggregation.MINUTE.value, 1)


def test_bybit_bar_spec_to_interval_rejects_unsupported_step() -> None:
    with pytest.raises(ValueError, match="interval"):
        bybit_bar_spec_to_interval(BarAggregation.MINUTE, 2)
