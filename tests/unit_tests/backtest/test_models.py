# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel


class TestFillModel:
    def test_instantiate_with_no_random_seed(self):
        # Arrange
        fill_model = FillModel()

        # Act, Assert
        assert not fill_model.is_slipped()
        assert fill_model.is_limit_filled()
        assert fill_model.is_stop_filled()

    def test_instantiate_with_random_seed(self):
        # Arrange
        fill_model = FillModel(random_seed=42)

        # Act, Assert
        assert not fill_model.is_slipped()
        assert fill_model.is_limit_filled()
        assert fill_model.is_stop_filled()

    def test_is_stop_filled_with_random_seed(self):
        # Arrange
        fill_model = FillModel(
            prob_fill_on_stop=0.5,
            random_seed=42,
        )

        # Act, Assert
        assert not fill_model.is_stop_filled()

    def test_is_limit_filled_with_random_seed(self):
        # Arrange
        fill_model = FillModel(
            prob_fill_on_limit=0.5,
            random_seed=42,
        )

        # Act, Assert
        assert not fill_model.is_limit_filled()

    def test_is_slipped_with_random_seed(self):
        # Arrange
        fill_model = FillModel(
            prob_slippage=0.5,
            random_seed=42,
        )

        # Act, Assert
        assert not fill_model.is_slipped()


class TestExchangeLatency:
    NANOSECONDS_IN_MILLISECOND = 1_000_000

    def test_instantiate_with_no_random_seed(self):
        latency = LatencyModel()
        assert latency.base_latency_nanos == self.NANOSECONDS_IN_MILLISECOND
        assert latency.insert_latency_nanos == self.NANOSECONDS_IN_MILLISECOND
        assert latency.update_latency_nanos == self.NANOSECONDS_IN_MILLISECOND
        assert latency.cancel_latency_nanos == self.NANOSECONDS_IN_MILLISECOND
