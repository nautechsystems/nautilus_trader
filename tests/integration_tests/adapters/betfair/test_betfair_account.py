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

from decimal import Decimal

import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price_c
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity_c
from nautilus_trader.common.clock import Clock
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.rust.common import LogLevel
from nautilus_trader.msgbus.bus import MessageBus
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider


class TestBetfairAccount:
    def setup(self):
        # Fixture Setup
        self.clock = Clock()
        self.venue = BETFAIR_VENUE
        self.account = TestExecStubs.betting_account()
        self.instrument = BetfairDataProvider.betting_instrument()

        # Setup logging
        self.logger = Logger(clock=self.clock, level_stdout=LogLevel.DEBUG)

        self.msgbus = MessageBus(
            trader_id=TestIdStubs.trader_id(),
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestComponentStubs.cache()
        self.cache.add_instrument(self.instrument)

    @pytest.mark.skip(reason="needs accounting fixes")
    def test_betting_instrument_notional_value(self):
        notional = self.instrument.notional_value(
            price=betfair_float_to_price_c(2.0),
            quantity=betfair_float_to_quantity_c(100.0),
        )
        assert notional == Decimal("200.0")
