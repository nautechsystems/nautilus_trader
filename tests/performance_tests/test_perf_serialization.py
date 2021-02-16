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

import unittest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.serialization.serializers import MsgPackCommandSerializer
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

AUDUSD = TestStubs.symbol_audusd()


class SerializationPerformanceTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.venue = Venue("SIM")
        self.trader_id = TestStubs.trader_id()
        self.account_id = TestStubs.account_id()
        self.serializer = MsgPackCommandSerializer()
        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

        self.order = self.order_factory.market(
            AUDUSD,
            OrderSide.BUY,
            Quantity(100000),
        )

        self.command = SubmitOrder(
            self.venue,
            self.trader_id,
            self.account_id,
            StrategyId("SCALPER", "01"),
            PositionId("P-123456"),
            self.order,
            uuid4(),
            UNIX_EPOCH,
        )

    def serialize_submit_order(self):
        # Arrange
        self.serializer.serialize(self.command)

    def test_make_builtin_uuid(self):
        PerformanceHarness.profile_function(self.serialize_submit_order, 10000, 1)
        # ~0.0ms / ~4.1μs / 4105ns minimum of 10,000 runs @ 1 iteration each run.
