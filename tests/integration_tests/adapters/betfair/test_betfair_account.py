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

import asyncio
from decimal import Decimal

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LiveLogger
from nautilus_trader.common.logging import LogLevel
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.msgbus.bus import MessageBus
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit.stubs import TestStubs


class TestBetfairAccount:
    def setup(self):
        # Fixture Setup
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)

        self.clock = LiveClock()
        self.venue = BETFAIR_VENUE
        self.account = TestStubs.betting_account()
        self.instrument = BetfairTestStubs.betting_instrument()

        # Setup logging
        self.logger = LiveLogger(loop=self.loop, clock=self.clock, level_stdout=LogLevel.DEBUG)

        self.msgbus = MessageBus(
            trader_id=TestStubs.trader_id(),
            clock=self.clock,
            logger=self.logger,
        )

        self.cache = TestStubs.cache()
        self.cache.add_instrument(BetfairTestStubs.betting_instrument())

    def test_betting_instrument_notional_value(self):
        notional = self.instrument.notional_value(
            quantity=Quantity.from_int(100),
            price=Price.from_str("0.5").as_decimal(),
            inverse_as_quote=False,
        ).as_decimal()
        # We are long 100 at 0.5 probability, aka 2.0 in odds terms
        assert notional == Decimal("200.0")
