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
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH

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

        event = TestStubs.event_order_filled(
            order1,
            position_id=PositionId("P-1"),
            fill_price=Price(0.80011, 5),
        )

        order1.apply(event)

        orders = [order1, order2]

        # Act
        report = report_provider.generate_orders_report(orders)

        # Assert
        self.assertEqual(2, len(report))
        self.assertEqual("cl_ord_id", report.index.name)
        self.assertEqual(order1.cl_ord_id.value, report.index[0])
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

        filled = TestStubs.event_order_filled(order1, PositionId("P-1"), Price(0.80011, 5))

        order1.apply(filled)

        orders = [order1, order2]

        # Act
        report = report_provider.generate_order_fills_report(orders)

        # Assert
        self.assertEqual(1, len(report))
        self.assertEqual("cl_ord_id", report.index.name)
        self.assertEqual(order1.cl_ord_id.value, report.index[0])
        self.assertEqual("AUD/USD", report.iloc[0]["symbol"])
        self.assertEqual("BUY", report.iloc[0]["side"])
        self.assertEqual("LIMIT", report.iloc[0]["type"])
        self.assertEqual(1500000, report.iloc[0]["quantity"])
        self.assertAlmostEqual(0.80011, report.iloc[0]["avg_price"])
        self.assertEqual(0.00001, report.iloc[0]["slippage"])

    def test_generate_trades_report(self):
        # Arrange
        report_provider = ReportProvider()

        position1 = TestStubs.position_which_is_closed(PositionId("P-1"))
        position2 = TestStubs.position_which_is_closed(PositionId("P-2"))

        positions = [position1, position2]

        # Act
        report = report_provider.generate_positions_report(positions)

        # Assert
        self.assertEqual(2, len(report))
        self.assertEqual("position_id", report.index.name)
        self.assertEqual(position1.id.value, report.index[0])
        self.assertEqual("AUD/USD", report.iloc[0]["symbol"])
        self.assertEqual("SELL", report.iloc[0]["entry"])
        self.assertEqual(100000, report.iloc[0]["peak_quantity"])
        self.assertEqual(1.0001, report.iloc[0]["avg_open_price"])
        self.assertEqual(1.0001, report.iloc[0]["avg_close_price"])
        self.assertEqual(UNIX_EPOCH + timedelta(minutes=5), report.iloc[0]["opened_time"])
        self.assertEqual(UNIX_EPOCH + timedelta(minutes=5), report.iloc[0]["closed_time"])
        self.assertEqual(0.0, report.iloc[0]["realized_points"])
        self.assertEqual(0.0, report.iloc[0]["realized_return"])
