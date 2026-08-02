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

from nautilus_trader.indicators import AdaptiveMovingAverage
from nautilus_trader.indicators import DoubleExponentialMovingAverage
from nautilus_trader.indicators import ExponentialMovingAverage
from nautilus_trader.indicators import HullMovingAverage
from nautilus_trader.indicators import SimpleMovingAverage
from nautilus_trader.indicators import SpreadAnalyzer
from nautilus_trader.indicators import VariableIndexDynamicAverage
from nautilus_trader.indicators import WeightedMovingAverage
from nautilus_trader.indicators import WilderMovingAverage
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import PriceType


def test_adaptive_moving_average_inspection_properties() -> None:
    indicator = AdaptiveMovingAverage(
        period_efficiency_ratio=10,
        period_fast=2,
        period_slow=30,
        price_type=PriceType.MID,
    )
    indicator.update_raw(12.5)

    assert indicator.period_efficiency_ratio == 10
    assert indicator.period_fast == 2
    assert indicator.period_slow == 30
    assert indicator.alpha_fast == pytest.approx(2 / 3)
    assert indicator.alpha_slow == pytest.approx(2 / 31)
    assert indicator.alpha_diff == pytest.approx((2 / 3) - (2 / 31))
    assert indicator.price_type == PriceType.MID
    assert indicator.value == 12.5


def test_weighted_moving_average_inspection_properties() -> None:
    indicator = WeightedMovingAverage(
        period=3,
        weights=[0.2, 0.3, 0.5],
        price_type=PriceType.BID,
    )
    indicator.update_raw(12.5)

    assert indicator.price_type == PriceType.BID
    assert indicator.value == 12.5
    assert indicator.weights == [0.2, 0.3, 0.5]


def test_spread_analyzer_instrument_id_readback() -> None:
    instrument_id = InstrumentId.from_str("AUD/USD.SIM")
    indicator = SpreadAnalyzer(instrument_id=instrument_id, capacity=10)

    assert indicator.instrument_id == instrument_id


@pytest.mark.parametrize(
    "indicator_type",
    [
        DoubleExponentialMovingAverage,
        ExponentialMovingAverage,
        HullMovingAverage,
        SimpleMovingAverage,
        VariableIndexDynamicAverage,
        WilderMovingAverage,
    ],
)
def test_moving_average_price_type_readback(indicator_type) -> None:
    indicator = indicator_type(period=10, price_type=PriceType.ASK)

    assert indicator.price_type == PriceType.ASK
