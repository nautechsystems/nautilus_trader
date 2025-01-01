# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.serialization.serializer import MsgSpecSerializer
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestSerializationPerformance:
    def setup(self):
        # Fixture Setup
        self.venue = Venue("SIM")
        self.trader_id = TestIdStubs.trader_id()
        self.account_id = TestIdStubs.account_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

        self.order = self.order_factory.market(
            TestIdStubs.audusd_id(),
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        self.command = SubmitOrder(
            trader_id=self.trader_id,
            strategy_id=StrategyId("SCALPER-001"),
            position_id=PositionId("P-123456"),
            order=self.order,
            command_id=UUID4(),
            ts_init=0,
        )

        self.serializer = MsgSpecSerializer(encoding=msgspec.msgpack)

    def test_serialize_submit_order(self, benchmark):
        benchmark(self.serializer.serialize, self.command)
