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

from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3 import AccountId
from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import ContingencyType
from nautilus_trader.core.nautilus_pyo3 import ExecutionMassStatus
from nautilus_trader.core.nautilus_pyo3 import FillReport
from nautilus_trader.core.nautilus_pyo3 import LiquiditySide
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderListId
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderStatus
from nautilus_trader.core.nautilus_pyo3 import OrderStatusReport
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.core.nautilus_pyo3 import PositionStatusReport
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TradeId
from nautilus_trader.core.nautilus_pyo3 import TrailingOffsetType
from nautilus_trader.core.nautilus_pyo3 import TriggerType
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.core.nautilus_pyo3 import VenueOrderId
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


AUDUSD_IDEALPRO = TestIdProviderPyo3.audusd_idealpro_id()


def test_instantiate_order_status_report():
    # Arrange
    report_id = UUID4()

    # Act
    report = OrderStatusReport(
        account_id=AccountId("SIM-001"),
        instrument_id=AUDUSD_IDEALPRO,
        client_order_id=ClientOrderId("O-123456"),
        order_list_id=OrderListId("1"),
        venue_order_id=VenueOrderId("2"),
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
        ts_accepted=1_000_000,
        ts_triggered=1_500_000,
        ts_last=2_000_000,
        ts_init=3_000_000,
        report_id=report_id,
    )

    # Assert
    assert report.account_id == AccountId("SIM-001")
    assert report.instrument_id == AUDUSD_IDEALPRO
    assert report.client_order_id == ClientOrderId("O-123456")
    assert report.order_list_id == OrderListId("1")
    assert report.venue_order_id == VenueOrderId("2")
    assert report.order_side == OrderSide.SELL
    assert report.order_type == OrderType.STOP_LIMIT
    assert report.contingency_type == ContingencyType.OCO
    assert report.time_in_force == TimeInForce.DAY
    assert report.expire_time is None
    assert report.order_status == OrderStatus.REJECTED
    assert report.price == Price.from_str("0.90090")
    assert report.trigger_price == Price.from_str("0.90100")
    assert report.trigger_type == TriggerType.DEFAULT
    assert report.limit_offset is None
    assert report.trailing_offset == Price.from_str("0.00010")
    assert report.trailing_offset_type == TrailingOffsetType.PRICE
    assert report.quantity == Quantity.from_int(1_000_000)
    assert report.filled_qty == Quantity.from_int(0)
    assert report.display_qty is None
    assert report.avg_px is None
    assert report.post_only is True
    assert report.reduce_only is False
    assert report.cancel_reason == "SOME_REASON"
    assert report.ts_accepted == 1_000_000
    assert report.ts_triggered == 1_500_000
    assert report.ts_last == 2_000_000
    assert report.ts_init == 3_000_000
    assert report.report_id == report_id


def test_instantiate_fill_report():
    # Arrange
    report_id = UUID4()

    # Act
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
        commission=Money.from_str("4.50 USD"),
        liquidity_side=LiquiditySide.TAKER,
        ts_event=0,
        ts_init=0,
        report_id=report_id,
    )

    # Assert
    assert report.account_id == AccountId("SIM-001")
    assert report.instrument_id == AUDUSD_IDEALPRO
    assert report.client_order_id == ClientOrderId("O-123456789")
    assert report.venue_order_id == VenueOrderId("1")
    assert report.venue_position_id == PositionId("2")
    assert report.trade_id == TradeId("3")
    assert report.order_side == OrderSide.BUY
    assert report.last_qty == Quantity.from_int(10_000_000)
    assert report.last_px == Price.from_str("100.50")
    assert report.commission == Money.from_str("4.50 USD")
    assert report.liquidity_side == LiquiditySide.TAKER
    assert report.ts_event == 0
    assert report.ts_init == 0
    assert report.report_id == report_id


def test_instantiate_position_status_report():
    # Arrange
    report_id1 = UUID4()
    report_id2 = UUID4()

    # Act
    report1 = PositionStatusReport(
        account_id=AccountId("SIM-001"),
        instrument_id=AUDUSD_IDEALPRO,
        venue_position_id=PositionId("1"),
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1_000_000),
        ts_last=0,
        ts_init=0,
        report_id=report_id1,
    )

    report2 = PositionStatusReport(
        account_id=AccountId("SIM-001"),
        instrument_id=AUDUSD_IDEALPRO,
        venue_position_id=PositionId("2"),
        position_side=PositionSide.SHORT,
        quantity=Quantity.from_int(1_000_000),
        ts_last=0,
        ts_init=0,
        report_id=report_id2,
    )

    # Assert
    assert report1.account_id == AccountId("SIM-001")
    assert report1.instrument_id == AUDUSD_IDEALPRO
    assert report1.venue_position_id == PositionId("1")
    assert report1.position_side == PositionSide.LONG
    assert report1.quantity == Quantity.from_int(1_000_000)
    assert report1.ts_last == 0
    assert report1.ts_init == 0
    assert report1.report_id == report_id1

    assert report2.account_id == AccountId("SIM-001")
    assert report2.instrument_id == AUDUSD_IDEALPRO
    assert report2.venue_position_id == PositionId("2")
    assert report2.position_side == PositionSide.SHORT
    assert report2.quantity == Quantity.from_int(1_000_000)
    assert report2.ts_last == 0
    assert report2.ts_init == 0
    assert report2.report_id == report_id2


def test_instantiate_execution_mass_status_report():
    # Arrange
    client_id = ClientId("IB")
    account_id = AccountId("IB-U123456789")
    venue = Venue("IDEALPRO")
    report_id = UUID4()

    # Act
    report = ExecutionMassStatus(
        client_id=client_id,
        account_id=account_id,
        venue=venue,
        ts_init=0,
        report_id=report_id,
    )

    # Assert
    assert report.client_id == client_id
    assert report.account_id == account_id
    assert report.venue == venue
    assert report.ts_init == 0
    assert report.report_id == report_id
    assert report.order_reports == {}
    assert report.fill_reports == {}
    assert report.position_reports == {}


def test_add_order_status_reports():
    # Arrange
    venue_order_id = VenueOrderId("2")
    report_id1 = UUID4()
    report_id2 = UUID4()

    mass_status = ExecutionMassStatus(
        client_id=ClientId("IB"),
        account_id=AccountId("IB-U123456789"),
        venue=Venue("IDEALPRO"),
        ts_init=0,
        report_id=report_id1,
    )

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
        ts_accepted=1_000_000,
        ts_triggered=0,
        ts_last=2_000_000,
        ts_init=3_000_000,
        report_id=report_id2,
    )

    # Act
    mass_status.add_order_reports([report])

    # Assert
    assert len(mass_status.order_reports) == 1
    assert mass_status.order_reports[venue_order_id] == report
    assert mass_status.fill_reports == {}
    assert mass_status.position_reports == {}


def test_add_fill_reports():
    # Arrange
    venue_order_id = VenueOrderId("1")
    report_id1 = UUID4()
    report_id2 = UUID4()
    report_id3 = UUID4()

    mass_status = ExecutionMassStatus(
        client_id=ClientId("IB"),
        account_id=AccountId("IB-U123456789"),
        venue=Venue("IDEALPRO"),
        ts_init=0,
        report_id=report_id1,
    )

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
        commission=Money.from_str("4.50 USD"),
        liquidity_side=LiquiditySide.TAKER,
        ts_event=0,
        ts_init=0,
        report_id=report_id2,
    )

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
        commission=Money.from_str("4.50 USD"),
        liquidity_side=LiquiditySide.TAKER,
        ts_event=0,
        ts_init=0,
        report_id=report_id3,
    )

    # Act
    mass_status.add_fill_reports([report1, report2])

    # Assert
    assert len(mass_status.fill_reports) == 1
    assert len(mass_status.fill_reports[venue_order_id]) == 2
    assert mass_status.fill_reports[venue_order_id] == [report1, report2]
    assert mass_status.order_reports == {}
    assert mass_status.position_reports == {}


def test_add_position_state_reports():
    report_id1 = UUID4()
    mass_status = ExecutionMassStatus(
        client_id=ClientId("IB"),
        account_id=AccountId("IB-U123456789"),
        venue=Venue("IDEALPRO"),
        ts_init=0,
        report_id=report_id1,
    )

    report_id2 = UUID4()
    report = PositionStatusReport(
        account_id=AccountId("IB-U123456789"),
        instrument_id=AUDUSD_IDEALPRO,
        venue_position_id=PositionId("1"),
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1_000_000),
        ts_last=0,
        ts_init=0,
        report_id=report_id2,
    )

    # Act
    mass_status.add_position_reports([report])

    # Assert
    assert len(mass_status.position_reports) == 1
    assert len(mass_status.position_reports[AUDUSD_IDEALPRO]) == 1
    assert mass_status.position_reports[AUDUSD_IDEALPRO] == [report]
    assert mass_status.order_reports == {}
    assert mass_status.fill_reports == {}
