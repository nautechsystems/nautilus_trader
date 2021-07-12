# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.stubs import TestStubs


class TestObjectPerformance(PerformanceHarness):
    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def test_make_symbol(self):
        self.benchmark.pedantic(
            target=Symbol,
            args=("AUD/USD",),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~0.4μs / 400ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def test_make_instrument_id(self):
        self.benchmark.pedantic(
            target=InstrumentId,
            args=(Symbol("AUD/USD"), Venue("IDEALPRO")),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~1.3μs / 1251ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def test_instrument_id_to_str(self):
        self.benchmark.pedantic(
            target=str,
            args=(TestStubs.audusd_id(),),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~0.2μs / 198ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def test_build_bar_no_checking(self):
        self.benchmark.pedantic(
            target=Bar,
            args=(
                TestStubs.bartype_audusd_1min_bid(),
                Price.from_str("1.00001"),
                Price.from_str("1.00004"),
                Price.from_str("1.00002"),
                Price.from_str("1.00003"),
                Quantity.from_str("100000"),
                0,
                False,  # <-- no check
            ),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~2.5μs / 2512ns minimum of 100,000 runs @ 1 iteration each run.

    def test_build_bar_with_checking(self):
        self.benchmark.pedantic(
            target=Bar,
            args=(
                TestStubs.bartype_audusd_1min_bid(),
                Price.from_str("1.00001"),
                Price.from_str("1.00004"),
                Price.from_str("1.00002"),
                Price.from_str("1.00003"),
                Quantity.from_str("100000"),
                0,
                True,  # <-- check
            ),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~2.7μs / 2717ns minimum of 100,000 runs @ 1 iteration each run.
