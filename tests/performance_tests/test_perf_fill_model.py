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

from nautilus_trader.backtest.models import FillModel
from nautilus_trader.test_kit.performance import PerformanceHarness


model = FillModel(
    prob_fill_on_stop=0.95,
    prob_fill_on_limit=0.5,
    random_seed=42,
)


class TestFillModelPerformance(PerformanceHarness):
    def test_is_limit_filled(self):
        self.benchmark.pedantic(
            target=model.is_limit_filled,
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~0.1μs / 106ns minimum of 100,000 runs @ 1 iteration each run.

    def test_is_stop_filled(self):
        self.benchmark.pedantic(
            target=model.is_stop_filled,
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~0.1μs / 106ns minimum of 100,000 runs @ 1 iteration each run.
