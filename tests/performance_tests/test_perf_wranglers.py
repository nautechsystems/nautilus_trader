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

from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.performance import PerformanceBench
from nautilus_trader.test_kit.performance import PerformanceHarness
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


class TestDataWranglersPerformance(PerformanceHarness):
    def test_quote_tick_data_wrangler_process_tick_data(self):
        usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        wrangler = QuoteTickDataWrangler(instrument=usdjpy)
        provider = TestDataProvider()

        def wrangler_process():
            # 1000 ticks in data
            wrangler.process(
                data=provider.read_csv_ticks("truefx-usdjpy-ticks.csv"),
                default_volume=1_000_000,
            )

        PerformanceBench.profile_function(
            target=wrangler_process,
            runs=100,
            iterations=1,
        )
        # ~7.8ms / ~7766.6μs / 7766626ns minimum of 100 runs @ 1 iteration each run.

    def test_trade_tick_data_wrangler_process(self):
        ethusdt = TestInstrumentProvider.ethusdt_binance()
        wrangler = TradeTickDataWrangler(instrument=ethusdt)
        provider = TestDataProvider()

        def wrangler_process():
            # 69806 ticks in data
            wrangler.process(data=provider.read_csv_ticks("binance-ethusdt-trades.csv"))

        PerformanceBench.profile_function(
            target=wrangler_process,
            runs=10,
            iterations=1,
        )
        # ~500.2ms / ~500210.6μs / 500210608ns minimum of 10 runs @ 1 iteration each run.
