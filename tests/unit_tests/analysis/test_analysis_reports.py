# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import timedelta
import unittest

from nautilus_trader.analysis.reports import ReportProvider
from nautilus_trader.backtest.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.enums import Currency
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import PositionIdBroker
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import UNIX_EPOCH
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = Symbol("AUD/USD", Venue('FXCM'))
GBPUSD_FXCM = Symbol("GBP/USD", Venue('FXCM'))


class ReportProviderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.account_id = TestStubs.account_id()
        self.order_factory = OrderFactory(
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
            clock=TestClock())

    def test_generate_orders_report(self):
        # Arrange
        report_provider = ReportProvider()
        order1 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(1500000),
            Price(0.80010, 5))

        order1.apply(TestStubs.event_order_submitted(order1))
        order1.apply(TestStubs.event_order_accepted(order1))
        order1.apply(TestStubs.event_order_working(order1))

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(1500000),
            Price(0.80000, 5))

        order2.apply(TestStubs.event_order_submitted(order2))
        order2.apply(TestStubs.event_order_accepted(order2))
        order2.apply(TestStubs.event_order_working(order2))

        event = OrderFilled(
            self.account_id,
            order1.id,
            ExecutionId("SOME_EXEC_ID_1"),
            PositionIdBroker("SOME_EXEC_TICKET_1"),
            order1.symbol,
            order1.side,
            order1.quantity,
            Price(0.80011, 5),
            Currency.AUD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        order1.apply(event)

        orders = {order1.id: order1,
                  order2.id: order2}
        # Act
        report = report_provider.generate_orders_report(orders)

        # Assert
        self.assertEqual(2, len(report))
        self.assertEqual("order_id", report.index.name)
        self.assertEqual(order1.id.value, report.index[0])
        self.assertEqual("AUD/USD", report.iloc[0]["symbol"])
        self.assertEqual("BUY", report.iloc[0]["side"])
        self.assertEqual("LIMIT", report.iloc[0]["type"])
        self.assertEqual(1500000, report.iloc[0]["quantity"])
        self.assertEqual(0.80011, report.iloc[0]["avg_price"])
        self.assertEqual(0.00001, report.iloc[0]["slippage"])
        self.assertEqual("None", report.iloc[1]["avg_price"])

    def test_generate_order_fills_report(self):
        # Arrange
        report_provider = ReportProvider()

        order1 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.BUY,
            Quantity(1500000),
            Price(0.80010, 5))

        order1.apply(TestStubs.event_order_submitted(order1))
        order1.apply(TestStubs.event_order_accepted(order1))
        order1.apply(TestStubs.event_order_working(order1))

        order2 = self.order_factory.limit(
            AUDUSD_FXCM,
            OrderSide.SELL,
            Quantity(1500000),
            Price(0.80000, 5))

        submitted2 = TestStubs.event_order_submitted(order2)
        accepted2 = TestStubs.event_order_accepted(order2)
        working2 = TestStubs.event_order_working(order2)

        order2.apply(submitted2)
        order2.apply(accepted2)
        order2.apply(working2)

        filled = OrderFilled(
            self.account_id,
            order1.id,
            ExecutionId("SOME_EXEC_ID_1"),
            PositionIdBroker("SOME_EXEC_TICKET_1"),
            order1.symbol,
            order1.side,
            order1.quantity,
            Price(0.80011, 5),
            Currency.AUD,
            UNIX_EPOCH,
            uuid4(),
            UNIX_EPOCH)

        order1.apply(filled)

        orders = {order1.id: order1,
                  order2.id: order2}
        # Act
        report = report_provider.generate_order_fills_report(orders)

        # Assert
        # print(report.iloc[0])
        self.assertEqual(1, len(report))
        self.assertEqual("order_id", report.index.name)
        self.assertEqual(order1.id.value, report.index[0])
        self.assertEqual("AUD/USD", report.iloc[0]["symbol"])
        self.assertEqual("BUY", report.iloc[0]["side"])
        self.assertEqual("LIMIT", report.iloc[0]["type"])
        self.assertEqual(1500000, report.iloc[0]["quantity"])
        self.assertAlmostEqual(0.80011, report.iloc[0]["avg_price"])
        self.assertEqual(0.00001, report.iloc[0]["slippage"])

    def test_generate_trades_report(self):
        # Arrange
        report_provider = ReportProvider()

        position1 = TestStubs.position_which_is_closed(number=1)
        position2 = TestStubs.position_which_is_closed(number=2)

        positions = {position1.id: position1,
                     position2.id: position2}

        # Act
        report = report_provider.generate_positions_report(positions)

        # Assert
        # print(report.iloc[0])
        self.assertEqual(2, len(report))
        self.assertEqual("position_id", report.index.name)
        self.assertEqual(position1.id.value, report.index[0])
        self.assertEqual("AUD/USD", report.iloc[0]["symbol"])
        self.assertEqual("BUY", report.iloc[0]["direction"])
        self.assertEqual(100000, report.iloc[0]["peak_quantity"])
        self.assertEqual(1.00000, report.iloc[0]["avg_open_price"])
        self.assertEqual(1.0001, report.iloc[0]["avg_close_price"])
        self.assertEqual(UNIX_EPOCH, report.iloc[0]["opened_time"])
        self.assertEqual(UNIX_EPOCH + timedelta(minutes=5), report.iloc[0]["closed_time"])
        self.assertEqual(9.999999999998899e-05, report.iloc[0]["realized_points"])
        self.assertEqual(9.999999999998899e-05, report.iloc[0]["realized_return"])
