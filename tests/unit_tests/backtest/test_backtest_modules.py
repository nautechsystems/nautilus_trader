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
import pandas as pd

from nautilus_trader.backtest.data.providers import TestDataProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.backtest.modules import SimulationModule
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


class TestSimulationModules:
    def create_engine(self, modules: list):
        engine = BacktestEngine()
        engine.add_venue(
            venue=Venue("SIM"),
            oms_type=OmsType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=modules,
        )
        wrangler = QuoteTickDataWrangler(USDJPY_SIM)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv")[:10],
            ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv")[:10],
        )
        engine.add_instrument(USDJPY_SIM)
        engine.add_data(ticks)
        return engine

    def test_fx_rollover_interest_module(self):
        # Arrange
        module = FXRolloverInterestModule(pd.DataFrame(columns=["LOCATION"]))
        engine = self.create_engine(modules=[module])

        # Act, Assert
        [venue] = engine.list_venues()
        assert venue

    def test_python_module(self):
        # Arrange
        class PythonModule(SimulationModule):
            def process(self, now_ns: int):
                assert self.exchange

            def log_diagnostics(self, log: LoggerAdapter):
                pass

        engine = self.create_engine(modules=[PythonModule()])

        # Act
        engine.run()
