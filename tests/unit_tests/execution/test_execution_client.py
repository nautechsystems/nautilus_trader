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
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.uuid import UUIDFactory
from nautilus_trader.execution.client import ExecutionClient
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.commands import CancelOrder
from nautilus_trader.model.commands import SubmitBracketOrder
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.commands import UpdateOrder
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import VenueType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class ExecutionClientTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.clock = TestClock()
        self.uuid_factory = UUIDFactory()
        self.logger = Logger(self.clock)

        self.trader_id = TraderId("TESTER-000")
        self.account_id = TestStubs.account_id()

        self.cache = TestStubs.cache()

        self.portfolio = Portfolio(
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.exec_engine = ExecutionEngine(
            portfolio=self.portfolio,
            cache=self.cache,
            clock=self.clock,
            logger=self.logger,
        )

        self.venue = Venue("SIM")

        self.client = ExecutionClient(
            client_id=ClientId(self.venue.value),
            venue_type=VenueType.ECN,
            account_id=TestStubs.account_id(),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER-000"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_venue_when_brokerage_returns_client_id_value_as_venue(self):
        assert self.client.venue == self.venue

    def test_venue_when_brokerage_multi_venue_returns_none(self):
        # Arrange
        client = ExecutionClient(
            client_id=ClientId("IB"),
            venue_type=VenueType.BROKERAGE_MULTI_VENUE,
            account_id=AccountId("IB", "U1258001"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            engine=self.exec_engine,
            clock=self.clock,
            logger=self.logger,
        )

        # Act, Assert
        assert client.venue is None

    def test_connect_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.connect)

    def test_disconnect_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.disconnect)

    def test_reset_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.reset)

    def test_dispose_when_not_implemented_raises_exception(self):
        self.assertRaises(NotImplementedError, self.client.dispose)

    def test_submit_order_raises_exception(self):
        order = self.order_factory.limit(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
            Price.from_str("1.00000"),
        )

        command = SubmitOrder(
            self.trader_id,
            order.strategy_id,
            PositionId.null(),
            order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.assertRaises(NotImplementedError, self.client.submit_order, command)

    def test_submit_bracket_order_raises_not_implemented_error(self):
        entry_order = self.order_factory.stop_market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
            Price.from_str("0.99995"),
        )

        # Act
        bracket_order = self.order_factory.bracket(
            entry_order,
            Price.from_str("0.99990"),
            Price.from_str("1.00010"),
        )

        command = SubmitBracketOrder(
            self.trader_id,
            entry_order.strategy_id,
            bracket_order,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        self.assertRaises(NotImplementedError, self.client.submit_bracket_order, command)

    def test_update_order_raises_not_implemented_error(self):
        # Arrange
        # Act
        command = UpdateOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            ClientOrderId("O-123456789"),
            VenueOrderId("001"),
            Quantity.from_int(120000),
            Price.from_str("1.00000"),
            None,
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Assert
        self.assertRaises(NotImplementedError, self.client.update_order, command)

    def test_cancel_order_raises_not_implemented_error(self):
        # Arrange
        # Act
        command = CancelOrder(
            self.trader_id,
            StrategyId("SCALPER-001"),
            AUDUSD_SIM.id,
            ClientOrderId("O-123456789"),
            VenueOrderId("001"),
            self.uuid_factory.generate(),
            self.clock.timestamp_ns(),
        )

        # Assert
        self.assertRaises(NotImplementedError, self.client.cancel_order, command)
