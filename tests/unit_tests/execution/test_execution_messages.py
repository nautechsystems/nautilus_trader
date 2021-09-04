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

from nautilus_trader.execution.messages import ExecutionMassStatus
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.execution.messages import PositionStatusReport
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestStubs.audusd_id()


class TestExecutionStateReport:
    def test_instantiate_execution_report(self):
        # Arrange, Act
        report = ExecutionReport(
            client_order_id=ClientOrderId("O-123456789"),
            venue_order_id=VenueOrderId("1"),
            venue_position_id=PositionId("1"),
            execution_id=ExecutionId("1"),
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
            == "ExecutionReport(client_order_id=O-123456789, venue_order_id=1, venue_position_id=1, id=1, last_qty=100, last_px=100.50, commission=4.50 USD, liquidity_side=TAKER, ts_event=0, ts_init=0)"  # noqa
        )  # noqa
        assert (
            repr(report)
            == "ExecutionReport(client_order_id=O-123456789, venue_order_id=1, venue_position_id=1, id=1, last_qty=100, last_px=100.50, commission=4.50 USD, liquidity_side=TAKER, ts_event=0, ts_init=0)"  # noqa
        )  # noqa

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
            repr(report)
            == "ExecutionMassStatus(client_id=IB, account_id=SIM-000, order_reports={}, exec_reports={}, position_reports={}, ts_init=0)"  # noqa
        )  # noqa

    def test_add_order_status_report(self):
        # Arrange
        report = ExecutionMassStatus(
            client_id=ClientId("IB"),
            account_id=TestStubs.account_id(),
            ts_init=0,
        )

        venue_order_id = VenueOrderId("1")
        order_report = OrderStatusReport(
            client_order_id=ClientOrderId("O-123456"),
            venue_order_id=venue_order_id,
            order_status=OrderStatus.REJECTED,
            filled_qty=Quantity.zero(),
            ts_init=0,
        )

        # Act
        report.add_order_report(order_report)

        # Assert
        assert report.order_reports()[venue_order_id] == order_report
        assert (
            repr(report)
            == "ExecutionMassStatus(client_id=IB, account_id=SIM-000, order_reports={VenueOrderId('1'): OrderStatusReport(client_order_id=O-123456, venue_order_id=1, order_status=REJECTED, filled_qty=0, ts_init=0)}, exec_reports={}, position_reports={}, ts_init=0)"  # noqa
        )
        assert (
            repr(order_report)
            == "OrderStatusReport(client_order_id=O-123456, venue_order_id=1, order_status=REJECTED, filled_qty=0, ts_init=0)"  # noqa
        )

    def test_add_position_state_report(self):
        report = ExecutionMassStatus(
            client_id=ClientId("IB"),
            account_id=TestStubs.account_id(),
            ts_init=0,
        )

        position_report = PositionStatusReport(
            instrument_id=AUDUSD_SIM,
            position_side=PositionSide.FLAT,
            qty=Quantity.zero(),
            ts_init=0,
        )

        # Act
        report.add_position_report(position_report)

        # Assert
        assert report.position_reports()[AUDUSD_SIM] == position_report
        assert (
            repr(report)
            == "ExecutionMassStatus(client_id=IB, account_id=SIM-000, order_reports={}, exec_reports={}, position_reports={InstrumentId('AUD/USD.SIM'): PositionStatusReport(instrument_id=AUD/USD.SIM, side=FLAT, qty=0, ts_init=0)}, ts_init=0)"  # noqa
        )  # noqa
        assert (
            repr(position_report)
            == "PositionStatusReport(instrument_id=AUD/USD.SIM, side=FLAT, qty=0, ts_init=0)"  # noqa
        )  # noqa
