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

import os
from decimal import Decimal

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.modules import FXRolloverInterestModule
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.providers import TestDataProvider
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.strategies import EMACross
from tests.test_kit.strategies import EMACrossConfig


class TestBacktestAcceptanceTestsWithCache:
    def setup(self):
        # Fixture Setup
        self.engine = BacktestEngine(
            bypass_logging=True,
            use_data_cache=True,
        )

        self.venue = Venue("SIM")
        self.usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

        self.engine.add_instrument(self.usdjpy)
        self.engine.add_bars_as_ticks(
            self.usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.BID,
            TestDataProvider.usdjpy_1min_bid()[:2000],
        )
        self.engine.add_bars_as_ticks(
            self.usdjpy.id,
            BarAggregation.MINUTE,
            PriceType.ASK,
            TestDataProvider.usdjpy_1min_ask()[:2000],
        )

        interest_rate_data = pd.read_csv(
            os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv")
        )
        fx_rollover_interest = FXRolloverInterestModule(rate_data=interest_rate_data)

        self.engine.add_venue(
            venue=self.venue,
            venue_type=VenueType.ECN,
            oms_type=OMSType.HEDGING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            modules=[fx_rollover_interest],
        )

    def teardown(self):
        self.engine.dispose()

    def test_run_ema_cross_strategy(self):
        # Arrange
        config = EMACrossConfig(
            instrument_id=str(self.usdjpy.id),
            bar_type="USD/JPY.SIM-15-MINUTE-BID-INTERNAL",
            trade_size=Decimal(1_000_000),
            fast_ema=10,
            slow_ema=20,
        )
        strategy = EMACross(config=config)

        # Act
        self.engine.run(strategies=[strategy])

        # Assert - Should return expected PnL
        assert strategy.fast_ema.count == 326
        assert self.engine.iteration == 7999
        assert self.engine.portfolio.account(self.venue).balance_total(USD) == Money(997542.89, USD)
