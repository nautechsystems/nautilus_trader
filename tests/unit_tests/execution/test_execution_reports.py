# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerMethod
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestStubs.audusd_id()


class TestExecutionReports:
    def test_instantiate_order_status_report(self):
        # Arrange, Act
        report = OrderStatusReport(
            instrument_id=AUDUSD_SIM,
            client_order_id=ClientOrderId("O-123456"),
            order_list_id=OrderListId("1"),
            venue_order_id=VenueOrderId("2"),
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_LIMIT,
            contingency=ContingencyType.OCO,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.REJECTED,
            price=Price.from_str("0.90090"),
            trigger_price=Price.from_str("0.90100"),
            trigger=TriggerMethod.DEFAULT,
            quantity=Quantity.from_int(1_000_000),
            filled_qty=Quantity.from_int(0),
            display_qty=None,
            avg_px=None,
            is_post_only=True,
            is_reduce_only=False,
            reject_reason="SOME_REASON",
            ts_accepted=1_000_000,
            ts_last=2_000_000,
            ts_init=3_000_000,
        )

        # Assert
        assert (
            str(report)
            == "OrderStatusReport(client_order_id=O-123456, order_list_id=1, venue_order_id=2, order_side=SELL, order_type=STOP_LIMIT, contingency=OCO, time_in_force=DAY, order_status=REJECTED, price=0.90090, trigger_price=0.90100, trigger=DEFAULT, quantity=1000000, filled_qty=0, leaves_qty=1000000, display_qty=None, avg_px=None, is_post_only=True, is_reduce_only=False, reject_reason=SOME_REASON, ts_accepted=1000000, ts_last=2000000, ts_init=3000000)"  # noqa
        )
        assert (
            repr(report)
            == "OrderStatusReport(client_order_id=O-123456, order_list_id=1, venue_order_id=2, order_side=SELL, order_type=STOP_LIMIT, contingency=OCO, time_in_force=DAY, order_status=REJECTED, price=0.90090, trigger_price=0.90100, trigger=DEFAULT, quantity=1000000, filled_qty=0, leaves_qty=1000000, display_qty=None, avg_px=None, is_post_only=True, is_reduce_only=False, reject_reason=SOME_REASON, ts_accepted=1000000, ts_last=2000000, ts_init=3000000)"  # noqa
        )

    def test_instantiate_trade_report(self):
        # Arrange, Act
        report = TradeReport(
            instrument_id=AUDUSD_SIM,
            client_order_id=ClientOrderId("O-123456789"),
            venue_order_id=VenueOrderId("1"),
            venue_position_id=PositionId("2"),
            trade_id=TradeId("3"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("100.50"),
            commission=Money("4.50", USD),
            liquidity_side=LiquiditySide.TAKER,
            ts_event=0,
            ts_init=0,
        )

        # Assert
        assert (
            str(report)
            == "TradeReport(instrument_id=AUD/USD.SIM, client_order_id=O-123456789, venue_order_id=1, venue_position_id=2, trade_id=3, order_side=BUY, last_qty=100, last_px=100.50, commission=4.50 USD, liquidity_side=TAKER, ts_event=0, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == "TradeReport(instrument_id=AUD/USD.SIM, client_order_id=O-123456789, venue_order_id=1, venue_position_id=2, trade_id=3, order_side=BUY, last_qty=100, last_px=100.50, commission=4.50 USD, liquidity_side=TAKER, ts_event=0, ts_init=0)"  # noqa
        )

    def test_instantiate_position_status_report(self):
        # Arrange, Act
        report = PositionStatusReport(
            instrument_id=AUDUSD_SIM,
            venue_position_id=PositionId("1"),
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(1_000_000),
            ts_last=0,
            ts_init=0,
        )

        # Assert
        assert (
            str(report)
            == "PositionStatusReport(instrument_id=AUD/USD.SIM, venue_position_id=1, position_side=LONG, quantity=1000000, ts_last=0, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == "PositionStatusReport(instrument_id=AUD/USD.SIM, venue_position_id=1, position_side=LONG, quantity=1000000, ts_last=0, ts_init=0)"  # noqa
        )

    def test_instantiate_execution_mass_status_report(self):
        # Arrange
        client_id = ClientId("IB")
        account_id = TestStubs.account_id()

        # Act
        report = ExecutionMassStatus(
            client_id=client_id,
            account_id=account_id,
            ts_init=0,
        )

        # Assert
        assert report.client_id == client_id
        assert report.account_id == account_id
        assert report.ts_init == 0
        assert report.order_reports() == {}
        assert report.position_reports() == {}
        assert (
            str(report)
            == "ExecutionMassStatus(client_id=IB, account_id=SIM-000, order_reports={}, trade_reports={}, position_reports={}, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == "ExecutionMassStatus(client_id=IB, account_id=SIM-000, order_reports={}, trade_reports={}, position_reports={}, ts_init=0)"  # noqa
        )

    def test_add_order_status_reports(self):
        # Arrange
        mass_status = ExecutionMassStatus(
            client_id=ClientId("IB"),
            account_id=TestStubs.account_id(),
            ts_init=0,
        )

        venue_order_id = VenueOrderId("2")
        report = OrderStatusReport(
            instrument_id=AUDUSD_SIM,
            client_order_id=ClientOrderId("O-123456"),
            order_list_id=OrderListId("1"),
            venue_order_id=venue_order_id,
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_LIMIT,
            contingency=ContingencyType.OCO,
            time_in_force=TimeInForce.DAY,
            order_status=OrderStatus.REJECTED,
            price=Price.from_str("0.90090"),
            trigger_price=Price.from_str("0.90100"),
            trigger=TriggerMethod.DEFAULT,
            quantity=Quantity.from_int(1_000_000),
            filled_qty=Quantity.from_int(0),
            display_qty=None,
            avg_px=None,
            is_post_only=True,
            is_reduce_only=False,
            reject_reason="SOME_REASON",
            ts_accepted=1_000_000,
            ts_last=2_000_000,
            ts_init=3_000_000,
        )

        # Act
        mass_status.add_order_reports([report])

        # Assert
        assert mass_status.order_reports()[venue_order_id] == report
        assert (
            repr(mass_status)
            == "ExecutionMassStatus(client_id=IB, account_id=SIM-000, order_reports={VenueOrderId('2'): OrderStatusReport(client_order_id=O-123456, order_list_id=1, venue_order_id=2, order_side=SELL, order_type=STOP_LIMIT, contingency=OCO, time_in_force=DAY, order_status=REJECTED, price=0.90090, trigger_price=0.90100, trigger=DEFAULT, quantity=1000000, filled_qty=0, leaves_qty=1000000, display_qty=None, avg_px=None, is_post_only=True, is_reduce_only=False, reject_reason=SOME_REASON, ts_accepted=1000000, ts_last=2000000, ts_init=3000000)}, trade_reports={}, position_reports={}, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == "OrderStatusReport(client_order_id=O-123456, order_list_id=1, venue_order_id=2, order_side=SELL, order_type=STOP_LIMIT, contingency=OCO, time_in_force=DAY, order_status=REJECTED, price=0.90090, trigger_price=0.90100, trigger=DEFAULT, quantity=1000000, filled_qty=0, leaves_qty=1000000, display_qty=None, avg_px=None, is_post_only=True, is_reduce_only=False, reject_reason=SOME_REASON, ts_accepted=1000000, ts_last=2000000, ts_init=3000000)"  # noqa
        )

    def test_add_position_state_reports(self):
        mass_status = ExecutionMassStatus(
            client_id=ClientId("IB"),
            account_id=TestStubs.account_id(),
            ts_init=0,
        )

        report = PositionStatusReport(
            instrument_id=AUDUSD_SIM,
            venue_position_id=PositionId("1"),
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(1_000_000),
            ts_last=0,
            ts_init=0,
        )

        # Act
        mass_status.add_position_reports([report])

        # Assert
        assert mass_status.position_reports()[AUDUSD_SIM] == [report]
        assert (
            repr(mass_status)
            == "ExecutionMassStatus(client_id=IB, account_id=SIM-000, order_reports={}, trade_reports={}, position_reports={InstrumentId('AUD/USD.SIM'): [PositionStatusReport(instrument_id=AUD/USD.SIM, venue_position_id=1, position_side=LONG, quantity=1000000, ts_last=0, ts_init=0)]}, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == "PositionStatusReport(instrument_id=AUD/USD.SIM, venue_position_id=1, position_side=LONG, quantity=1000000, ts_last=0, ts_init=0)"  # noqa
        )
