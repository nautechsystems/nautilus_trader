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
from nautilus_trader.common.messages import Subscribe
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.serialization.base import CommandSerializer
from nautilus_trader.serialization.base import EventSerializer
from nautilus_trader.serialization.base import InstrumentSerializer
from nautilus_trader.serialization.base import OrderSerializer
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd_fxcm())


class SerializationBaseTests(unittest.TestCase):

    def test_instrument_serializer_methods_raise_not_implemented_error(self):
        # Arrange
        serializer = InstrumentSerializer()

        # Act
        # Assert
        self.assertRaises(NotImplementedError, serializer.serialize, AUDUSD_SIM)
        self.assertRaises(NotImplementedError, serializer.deserialize, bytes())

    def test_order_serializer_methods_raise_not_implemented_error(self):
        # Arrange
        order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

        order = order_factory.market(
            AUDUSD_SIM.symbol,
            OrderSide.BUY,
            Quantity(100000),
        )

        serializer = OrderSerializer()

        # Act
        # Assert
        self.assertRaises(NotImplementedError, serializer.serialize, order)
        self.assertRaises(NotImplementedError, serializer.deserialize, bytes())

    def test_command_serializer_methods_raise_not_implemented_error(self):
        # Arrange
        command = Subscribe(
            venue=Venue("SIM"),
            data_type=QuoteTick,
            metadata={},
            handler=[].append,
            command_id=uuid4(),
            command_timestamp=UNIX_EPOCH,
        )

        serializer = CommandSerializer()

        # Act
        # Assert
        self.assertRaises(NotImplementedError, serializer.serialize, command)
        self.assertRaises(NotImplementedError, serializer.deserialize, bytes())

    def test_event_serializer_methods_raise_not_implemented_error(self):
        # Arrange
        event = TestStubs.event_account_state(TestStubs.account_id())
        serializer = EventSerializer()

        # Act
        # Assert
        self.assertRaises(NotImplementedError, serializer.serialize, event)
        self.assertRaises(NotImplementedError, serializer.deserialize, bytes())
