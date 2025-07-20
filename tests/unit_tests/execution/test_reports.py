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

from decimal import Decimal

from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_IDEALPRO = TestIdStubs.audusd_idealpro_id()


class TestExecutionReports:
    def test_instantiate_order_status_report(self):
        # Arrange, Act
        report_id = UUID4()
        report = OrderStatusReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_IDEALPRO,
            client_order_id=ClientOrderId("O-123456"),
            order_list_id=OrderListId("1"),
            venue_order_id=VenueOrderId("2"),
            venue_position_id=("123456789"),
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_LIMIT,
            contingency_type=ContingencyType.OCO,
            time_in_force=TimeInForce.DAY,
            expire_time=None,
            order_status=OrderStatus.REJECTED,
            price=Price.from_str("0.90090"),
            trigger_price=Price.from_str("0.90100"),
            trigger_type=TriggerType.DEFAULT,
            limit_offset=None,
            trailing_offset=Decimal("0.00010"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            quantity=Quantity.from_int(1_000_000),
            filled_qty=Quantity.from_int(0),
            display_qty=None,
            avg_px=None,
            post_only=True,
            reduce_only=False,
            cancel_reason="SOME_REASON",
            report_id=report_id,
            ts_accepted=1_000_000,
            ts_triggered=1_500_000,
            ts_last=2_000_000,
            ts_init=3_000_000,
        )

        # Assert
        assert (
            str(report)
            == f"OrderStatusReport(account_id=SIM-001, instrument_id=AUD/USD.IDEALPRO, client_order_id=O-123456, order_list_id=1, venue_order_id=2, venue_position_id=123456789, order_side=SELL, order_type=STOP_LIMIT, contingency_type=OCO, time_in_force=DAY, expire_time=None, order_status=REJECTED, price=0.90090, trigger_price=0.90100, trigger_type=DEFAULT, limit_offset=None, trailing_offset=0.00010, trailing_offset_type=PRICE, quantity=1_000_000, filled_qty=0, leaves_qty=1_000_000, display_qty=None, avg_px=None, post_only=True, reduce_only=False, cancel_reason=SOME_REASON, report_id={report_id}, ts_accepted=1000000, ts_triggered=1500000, ts_last=2000000, ts_init=3000000)"  # noqa
        )
        assert (
            repr(report)
            == f"OrderStatusReport(account_id=SIM-001, instrument_id=AUD/USD.IDEALPRO, client_order_id=O-123456, order_list_id=1, venue_order_id=2, venue_position_id=123456789, order_side=SELL, order_type=STOP_LIMIT, contingency_type=OCO, time_in_force=DAY, expire_time=None, order_status=REJECTED, price=0.90090, trigger_price=0.90100, trigger_type=DEFAULT, limit_offset=None, trailing_offset=0.00010, trailing_offset_type=PRICE, quantity=1_000_000, filled_qty=0, leaves_qty=1_000_000, display_qty=None, avg_px=None, post_only=True, reduce_only=False, cancel_reason=SOME_REASON, report_id={report_id}, ts_accepted=1000000, ts_triggered=1500000, ts_last=2000000, ts_init=3000000)"  # noqa
        )

    def test_instantiate_fill_report(self):
        # Arrange, Act
        report_id = UUID4()
        report = FillReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_IDEALPRO,
            client_order_id=ClientOrderId("O-123456789"),
            venue_order_id=VenueOrderId("1"),
            venue_position_id=PositionId("2"),
            trade_id=TradeId("3"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(10_000_000),
            last_px=Price.from_str("100.50"),
            commission=Money("4.50", USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=report_id,
            ts_event=0,
            ts_init=0,
        )

        # Assert
        assert (
            str(report)
            == f"FillReport(account_id=SIM-001, instrument_id=AUD/USD.IDEALPRO, client_order_id=O-123456789, venue_order_id=1, venue_position_id=2, trade_id=3, order_side=BUY, last_qty=10_000_000, last_px=100.50, commission=4.50 USD, liquidity_side=TAKER, report_id={report_id}, ts_event=0, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == f"FillReport(account_id=SIM-001, instrument_id=AUD/USD.IDEALPRO, client_order_id=O-123456789, venue_order_id=1, venue_position_id=2, trade_id=3, order_side=BUY, last_qty=10_000_000, last_px=100.50, commission=4.50 USD, liquidity_side=TAKER, report_id={report_id}, ts_event=0, ts_init=0)"  # noqa
        )

    def test_instantiate_position_status_report(self):
        # Arrange, Act
        report_id1 = UUID4()
        report1 = PositionStatusReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_IDEALPRO,
            venue_position_id=PositionId("1"),
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(1_000_000),
            report_id=report_id1,
            ts_last=0,
            ts_init=0,
        )

        report_id2 = UUID4()
        report2 = PositionStatusReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_IDEALPRO,
            venue_position_id=PositionId("2"),
            position_side=PositionSide.SHORT,
            quantity=Quantity.from_int(1_000_000),
            report_id=report_id2,
            ts_last=0,
            ts_init=0,
        )

        # Assert
        assert (
            str(report1)
            == f"PositionStatusReport(account_id=SIM-001, instrument_id=AUD/USD.IDEALPRO, venue_position_id=1, position_side=LONG, quantity=1_000_000, signed_decimal_qty=1000000, report_id={report_id1}, ts_last=0, ts_init=0)"  # noqa
        )
        assert (
            repr(report1)
            == f"PositionStatusReport(account_id=SIM-001, instrument_id=AUD/USD.IDEALPRO, venue_position_id=1, position_side=LONG, quantity=1_000_000, signed_decimal_qty=1000000, report_id={report_id1}, ts_last=0, ts_init=0)"  # noqa
        )

        assert (
            str(report2)
            == f"PositionStatusReport(account_id=SIM-001, instrument_id=AUD/USD.IDEALPRO, venue_position_id=2, position_side=SHORT, quantity=1_000_000, signed_decimal_qty=-1000000, report_id={report_id2}, ts_last=0, ts_init=0)"  # noqa
        )
        assert (
            repr(report2)
            == f"PositionStatusReport(account_id=SIM-001, instrument_id=AUD/USD.IDEALPRO, venue_position_id=2, position_side=SHORT, quantity=1_000_000, signed_decimal_qty=-1000000, report_id={report_id2}, ts_last=0, ts_init=0)"  # noqa
        )

    def test_instantiate_execution_mass_status_report(self):
        # Arrange
        client_id = ClientId("IB")
        account_id = AccountId("IB-U123456789")

        # Act
        report_id = UUID4()
        report = ExecutionMassStatus(
            client_id=client_id,
            account_id=account_id,
            venue=Venue("IDEALPRO"),
            report_id=report_id,
            ts_init=0,
        )

        # Assert
        assert report.client_id == client_id
        assert report.account_id == account_id
        assert report.ts_init == 0
        assert report.order_reports == {}
        assert report.position_reports == {}
        assert (
            str(report)
            == f"ExecutionMassStatus(client_id=IB, account_id=IB-U123456789, venue=IDEALPRO, order_reports={{}}, fill_reports={{}}, position_reports={{}}, report_id={report_id}, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == f"ExecutionMassStatus(client_id=IB, account_id=IB-U123456789, venue=IDEALPRO, order_reports={{}}, fill_reports={{}}, position_reports={{}}, report_id={report_id}, ts_init=0)"  # noqa
        )

    def test_add_order_status_reports(self):
        # Arrange
        report_id1 = UUID4()
        mass_status = ExecutionMassStatus(
            client_id=ClientId("IB"),
            account_id=AccountId("IB-U123456789"),
            venue=Venue("IDEALPRO"),
            report_id=report_id1,
            ts_init=0,
        )

        venue_order_id = VenueOrderId("2")
        report_id2 = UUID4()
        report = OrderStatusReport(
            account_id=AccountId("IB-U123456789"),
            instrument_id=AUDUSD_IDEALPRO,
            client_order_id=ClientOrderId("O-123456"),
            order_list_id=OrderListId("1"),
            venue_order_id=venue_order_id,
            order_side=OrderSide.SELL,
            order_type=OrderType.STOP_LIMIT,
            contingency_type=ContingencyType.OCO,
            time_in_force=TimeInForce.DAY,
            expire_time=None,
            order_status=OrderStatus.REJECTED,
            price=Price.from_str("0.90090"),
            trigger_price=Price.from_str("0.90100"),
            trigger_type=TriggerType.DEFAULT,
            limit_offset=None,
            trailing_offset=Decimal("0.00010"),
            trailing_offset_type=TrailingOffsetType.PRICE,
            quantity=Quantity.from_int(1_000_000),
            filled_qty=Quantity.from_int(0),
            display_qty=None,
            avg_px=None,
            post_only=True,
            reduce_only=False,
            cancel_reason="SOME_REASON",
            report_id=report_id2,
            ts_accepted=1_000_000,
            ts_triggered=0,
            ts_last=2_000_000,
            ts_init=3_000_000,
        )

        # Act
        mass_status.add_order_reports([report])

        # Assert
        assert mass_status.order_reports[venue_order_id] == report
        assert (
            repr(mass_status)
            == f"ExecutionMassStatus(client_id=IB, account_id=IB-U123456789, venue=IDEALPRO, order_reports={{VenueOrderId('2'): OrderStatusReport(account_id=IB-U123456789, instrument_id=AUD/USD.IDEALPRO, client_order_id=O-123456, order_list_id=1, venue_order_id=2, venue_position_id=None, order_side=SELL, order_type=STOP_LIMIT, contingency_type=OCO, time_in_force=DAY, expire_time=None, order_status=REJECTED, price=0.90090, trigger_price=0.90100, trigger_type=DEFAULT, limit_offset=None, trailing_offset=0.00010, trailing_offset_type=PRICE, quantity=1_000_000, filled_qty=0, leaves_qty=1_000_000, display_qty=None, avg_px=None, post_only=True, reduce_only=False, cancel_reason=SOME_REASON, report_id={report_id2}, ts_accepted=1000000, ts_triggered=0, ts_last=2000000, ts_init=3000000)}}, fill_reports={{}}, position_reports={{}}, report_id={report_id1}, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == f"OrderStatusReport(account_id=IB-U123456789, instrument_id=AUD/USD.IDEALPRO, client_order_id=O-123456, order_list_id=1, venue_order_id=2, venue_position_id=None, order_side=SELL, order_type=STOP_LIMIT, contingency_type=OCO, time_in_force=DAY, expire_time=None, order_status=REJECTED, price=0.90090, trigger_price=0.90100, trigger_type=DEFAULT, limit_offset=None, trailing_offset=0.00010, trailing_offset_type=PRICE, quantity=1_000_000, filled_qty=0, leaves_qty=1_000_000, display_qty=None, avg_px=None, post_only=True, reduce_only=False, cancel_reason=SOME_REASON, report_id={report_id2}, ts_accepted=1000000, ts_triggered=0, ts_last=2000000, ts_init=3000000)"  # noqa
        )

    def test_add_fill_reports(self):
        report_id1 = UUID4()
        mass_status = ExecutionMassStatus(
            client_id=ClientId("IB"),
            account_id=AccountId("IB-U123456789"),
            venue=Venue("IDEALPRO"),
            report_id=report_id1,
            ts_init=0,
        )

        report_id2 = UUID4()
        report1 = FillReport(
            account_id=AccountId("IB-U123456789"),
            instrument_id=AUDUSD_IDEALPRO,
            client_order_id=ClientOrderId("O-123456789"),
            venue_order_id=VenueOrderId("1"),
            venue_position_id=PositionId("2"),
            trade_id=TradeId("3"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("100.50"),
            commission=Money("4.50", USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=report_id2,
            ts_event=0,
            ts_init=0,
        )

        report_id3 = UUID4()
        report2 = FillReport(
            account_id=AccountId("IB-U123456789"),
            instrument_id=AUDUSD_IDEALPRO,
            client_order_id=ClientOrderId("O-123456790"),
            venue_order_id=VenueOrderId("1"),
            venue_position_id=PositionId("2"),
            trade_id=TradeId("4"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("100.60"),
            commission=Money("4.50", USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=report_id3,
            ts_event=0,
            ts_init=0,
        )

        # Act
        mass_status.add_fill_reports([report1, report2])

        # Assert
        assert mass_status.fill_reports[VenueOrderId("1")] == [report1, report2]

    def test_add_position_state_reports(self):
        report_id1 = UUID4()
        mass_status = ExecutionMassStatus(
            client_id=ClientId("IB"),
            account_id=AccountId("IB-U123456789"),
            venue=Venue("IDEALPRO"),
            report_id=report_id1,
            ts_init=0,
        )

        report_id2 = UUID4()
        report = PositionStatusReport(
            account_id=AccountId("IB-U123456789"),
            instrument_id=AUDUSD_IDEALPRO,
            venue_position_id=PositionId("1"),
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(1_000_000),
            report_id=report_id2,
            ts_last=0,
            ts_init=0,
        )

        # Act
        mass_status.add_position_reports([report])

        # Assert
        assert mass_status.position_reports[AUDUSD_IDEALPRO] == [report]
        assert (
            repr(mass_status)
            == f"ExecutionMassStatus(client_id=IB, account_id=IB-U123456789, venue=IDEALPRO, order_reports={{}}, fill_reports={{}}, position_reports={{InstrumentId('AUD/USD.IDEALPRO'): [PositionStatusReport(account_id=IB-U123456789, instrument_id=AUD/USD.IDEALPRO, venue_position_id=1, position_side=LONG, quantity=1_000_000, signed_decimal_qty=1000000, report_id={report_id2}, ts_last=0, ts_init=0)]}}, report_id={report_id1}, ts_init=0)"  # noqa
        )
        assert (
            repr(report)
            == f"PositionStatusReport(account_id=IB-U123456789, instrument_id=AUD/USD.IDEALPRO, venue_position_id=1, position_side=LONG, quantity=1_000_000, signed_decimal_qty=1000000, report_id={report_id2}, ts_last=0, ts_init=0)"  # noqa
        )

    def test_order_status_report_serialization(self):
        # Arrange
        report_id = UUID4()
        report = OrderStatusReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_IDEALPRO,
            venue_order_id=VenueOrderId("2"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(100_000),
            filled_qty=Quantity.from_int(0),
            report_id=report_id,
            ts_accepted=1_000_000,
            ts_last=2_000_000,
            ts_init=3_000_000,
            client_order_id=ClientOrderId("O-123456"),
        )

        # Act
        serialized = report.to_dict()
        deserialized = OrderStatusReport.from_dict(serialized)

        # Assert
        assert deserialized == report
        assert str(deserialized) == str(report)

    def test_fill_report_serialization(self):
        # Arrange
        report_id = UUID4()
        report = FillReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_IDEALPRO,
            venue_order_id=VenueOrderId("1"),
            trade_id=TradeId("3"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(100),
            last_px=Price.from_str("100.50"),
            commission=Money("4.50", USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=report_id,
            ts_event=1_000_000,
            ts_init=2_000_000,
            client_order_id=ClientOrderId("O-123456"),
            venue_position_id=PositionId("2"),
        )

        # Act
        serialized = report.to_dict()
        deserialized = FillReport.from_dict(serialized)

        # Assert
        assert deserialized == report
        assert str(deserialized) == str(report)

    def test_position_status_report_serialization(self):
        # Arrange
        report_id = UUID4()
        report = PositionStatusReport(
            account_id=AccountId("SIM-001"),
            instrument_id=AUDUSD_IDEALPRO,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(100_000),
            report_id=report_id,
            ts_last=1_000_000,
            ts_init=2_000_000,
            venue_position_id=PositionId("1"),
        )

        # Act
        serialized = report.to_dict()
        deserialized = PositionStatusReport.from_dict(serialized)

        # Assert
        assert deserialized.account_id == report.account_id
        assert deserialized.instrument_id == report.instrument_id
        assert deserialized.position_side == report.position_side
        assert deserialized.quantity == report.quantity
        assert deserialized.id == report.id
        assert deserialized.ts_last == report.ts_last
        assert deserialized.ts_init == report.ts_init
        assert deserialized.venue_position_id == report.venue_position_id

    def test_execution_mass_status_serialization(self):
        # Arrange
        report_id = UUID4()
        mass_status = ExecutionMassStatus(
            client_id=ClientId("IB"),
            account_id=AccountId("IB-U123456789"),
            venue=Venue("IDEALPRO"),
            report_id=report_id,
            ts_init=1_000_000,
        )

        # Add some reports
        order_report_id = UUID4()
        order_report = OrderStatusReport(
            account_id=AccountId("IB-U123456789"),
            instrument_id=AUDUSD_IDEALPRO,
            venue_order_id=VenueOrderId("2"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            order_status=OrderStatus.ACCEPTED,
            quantity=Quantity.from_int(100_000),
            filled_qty=Quantity.from_int(0),
            report_id=order_report_id,
            ts_accepted=1_000_000,
            ts_last=2_000_000,
            ts_init=3_000_000,
        )
        mass_status.add_order_reports([order_report])

        fill_report_id = UUID4()
        fill_report = FillReport(
            account_id=AccountId("IB-U123456789"),
            instrument_id=AUDUSD_IDEALPRO,
            venue_order_id=VenueOrderId("2"),
            trade_id=TradeId("3"),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_int(50_000),
            last_px=Price.from_str("100.50"),
            commission=Money("2.25", USD),
            liquidity_side=LiquiditySide.TAKER,
            report_id=fill_report_id,
            ts_event=2_500_000,
            ts_init=3_000_000,
        )
        mass_status.add_fill_reports([fill_report])

        position_report_id = UUID4()
        position_report = PositionStatusReport(
            account_id=AccountId("IB-U123456789"),
            instrument_id=AUDUSD_IDEALPRO,
            position_side=PositionSide.LONG,
            quantity=Quantity.from_int(50_000),
            report_id=position_report_id,
            ts_last=2_500_000,
            ts_init=3_000_000,
        )
        mass_status.add_position_reports([position_report])

        # Act
        serialized = mass_status.to_dict()
        deserialized = ExecutionMassStatus.from_dict(serialized)

        # Assert
        assert deserialized.client_id == mass_status.client_id
        assert deserialized.account_id == mass_status.account_id
        assert deserialized.venue == mass_status.venue
        assert deserialized.id == mass_status.id
        assert deserialized.ts_init == mass_status.ts_init
        assert len(deserialized.order_reports) == 1
        assert len(deserialized.fill_reports) == 1
        assert len(deserialized.position_reports) == 1
        assert VenueOrderId("2") in deserialized.order_reports
        assert VenueOrderId("2") in deserialized.fill_reports
        assert AUDUSD_IDEALPRO in deserialized.position_reports
