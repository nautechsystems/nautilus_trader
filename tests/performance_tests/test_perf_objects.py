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

import pytest

from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.performance import PerformanceBench
from nautilus_trader.test_kit.performance import PerformanceHarness
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestObjectPerformance(PerformanceHarness):
    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def test_create_symbol(self):
        self.benchmark.pedantic(
            target=Symbol,
            args=("AUD/USD",),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~0.4μs / 400ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    def test_create_instrument_id(self):
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
            args=(TestIdStubs.audusd_id(),),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~0.2μs / 198ns minimum of 100,000 runs @ 1 iteration each run.

    def test_create_bar(self):
        self.benchmark.pedantic(
            target=Bar,
            args=(
                TestDataStubs.bartype_audusd_1min_bid(),
                Price.from_str("1.00001"),
                Price.from_str("1.00004"),
                Price.from_str("1.00000"),
                Price.from_str("1.00003"),
                Quantity.from_str("100000"),
                0,
                0,
            ),
            iterations=100_000,
            rounds=1,
        )
        # ~0.0ms / ~2.7μs / 2717ns minimum of 100,000 runs @ 1 iteration each run.

    def test_create_quote_tick(self):
        audusd_sim = TestInstrumentProvider.default_fx_ccy("AUD/USD")

        def create_quote_tick():
            QuoteTick(
                instrument_id=audusd_sim.id,
                bid=Price.from_str("1.00000"),
                ask=Price.from_str("1.00001"),
                bid_size=Quantity.from_int(1),
                ask_size=Quantity.from_int(1),
                ts_event=0,
                ts_init=0,
            )

        PerformanceBench.profile_function(
            target=create_quote_tick,
            runs=100000,
            iterations=1,
        )
        # ~0.0ms / ~2.8μs / 2798ns minimum of 100,000 runs @ 1 iteration each run.

    def test_create_quote_tick_raw(self):
        audusd_sim = TestInstrumentProvider.default_fx_ccy("AUD/USD")

        def create_quote_tick():
            QuoteTick.from_raw(
                audusd_sim.id,
                1000000000,
                1000010000,
                5,
                5,
                1000000000,
                1000000000,
                0,
                0,
                0,
                0,
            )

        PerformanceBench.profile_function(
            target=create_quote_tick,
            runs=100000,
            iterations=1,
        )
        # ~0.0ms / ~0.2μs / 218ns minimum of 100,000 runs @ 1 iteration each run.

    def test_create_trade_tick(self):
        audusd_sim = TestInstrumentProvider.default_fx_ccy("AUD/USD")

        def create_trade_tick():
            TradeTick(
                instrument_id=audusd_sim.id,
                price=Price.from_str("1.00000"),
                size=Quantity.from_int(1),
                aggressor_side=AggressorSide.BUYER,
                trade_id=TradeId("123458"),
                ts_event=0,
                ts_init=0,
            )

        PerformanceBench.profile_function(
            target=create_trade_tick,
            runs=100000,
            iterations=1,
        )
        # ~0.0ms / ~2.5μs / 2492ns minimum of 100,000 runs @ 1 iteration each run.

    def test_create_trade_tick_from_raw(self):
        audusd_sim = TestInstrumentProvider.default_fx_ccy("AUD/USD")

        def create_trade_tick():
            TradeTick.from_raw(
                audusd_sim.id,
                1000000000,
                5,
                1000000000,
                0,
                AggressorSide.BUYER,
                TradeId("123458"),
                0,
                0,
            )

        PerformanceBench.profile_function(
            target=create_trade_tick,
            runs=100000,
            iterations=1,
        )
        # ~0.0ms / ~0.7μs / 718ns minimum of 100,000 runs @ 1 iteration each run.
