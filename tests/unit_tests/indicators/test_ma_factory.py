# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.indicators.average.dema import DoubleExponentialMovingAverage
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.indicators.average.hma import HullMovingAverage
from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.moving_average import MovingAverageType
from nautilus_trader.indicators.average.rma import WilderMovingAverage
from nautilus_trader.indicators.average.sma import SimpleMovingAverage
from nautilus_trader.indicators.average.vidya import VariableIndexDynamicAverage
from nautilus_trader.indicators.average.wma import WeightedMovingAverage
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestMaFactory:
    def test_simple_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.SIMPLE)

        # Assert
        assert isinstance(indicator, SimpleMovingAverage)

    def test_exponential_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.EXPONENTIAL)

        # Assert
        assert isinstance(indicator, ExponentialMovingAverage)

    def test_hull_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.HULL)

        # Assert
        assert isinstance(indicator, HullMovingAverage)

    def test_weighted_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.WEIGHTED)

        # Assert
        assert isinstance(indicator, WeightedMovingAverage)

    def test_wilde_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.WILDER)

        # Assert
        assert isinstance(indicator, WilderMovingAverage)

    def test_double_exponential_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(10, MovingAverageType.DOUBLE_EXPONENTIAL)

        # Assert
        assert isinstance(indicator, DoubleExponentialMovingAverage)

    def test_variable_index_dynamic_returns_expected_indicator(self):
        # Arrange, Act
        indicator = MovingAverageFactory.create(
            10,
            MovingAverageType.VARIABLE_INDEX_DYNAMIC,
            cmo_ma_type=MovingAverageType.SIMPLE,
        )

        # Assert
        assert isinstance(indicator, VariableIndexDynamicAverage)
