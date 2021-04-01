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

from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderCancelled
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderInvalid
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderUpdateRejected
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestEvents:
    def test_account_state_str_repr(self):
        # Arrange
        uuid = uuid4()
        event = AccountState(
            account_id=AccountId("SIM", "000"),
            balances=[Money(1525000, USD)],
            balances_free=[Money(1525000, USD)],
            balances_locked=[Money(0, USD)],
            info={},
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        # Assert
        assert (
            f"AccountState(account_id=SIM-000, free=[1,525,000.00 USD], locked=[0.00 USD], event_id={uuid})"
            == str(event)
        )
        assert (
            f"AccountState(account_id=SIM-000, free=[1,525,000.00 USD], locked=[0.00 USD], event_id={uuid})"
            == repr(event)
        )

    def test_order_initialized(self):
        # Arrange
        uuid = uuid4()
        event = OrderInitialized(
            cl_ord_id=ClientOrderId("O-2020872378423"),
            strategy_id=StrategyId("SCALPER", "001"),
            instrument_id=InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE")),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity("0.561000"),
            time_in_force=TimeInForce.DAY,
            event_id=uuid,
            timestamp_ns=0,
            options={"Price": "15200.10"},
        )

        # Act
        # Assert
        assert (
            f"OrderInitialized(cl_ord_id=O-2020872378423, strategy_id=SCALPER-001, event_id={uuid})"
            == str(event)
        )
        assert (
            f"OrderInitialized(cl_ord_id=O-2020872378423, strategy_id=SCALPER-001, event_id={uuid})"
            == repr(event)
        )

    def test_order_invalid(self):
        # Arrange
        uuid = uuid4()
        event = OrderInvalid(
            cl_ord_id=ClientOrderId("O-2020872378423"),
            reason="DUPLICATE_CL_ORD_ID",
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        # Assert
        assert (
            f"OrderInvalid(cl_ord_id=O-2020872378423, reason='DUPLICATE_CL_ORD_ID', event_id={uuid})"
            == str(event)
        )
        assert (
            f"OrderInvalid(cl_ord_id=O-2020872378423, reason='DUPLICATE_CL_ORD_ID', event_id={uuid})"
            == repr(event)
        )

    def test_order_denied(self):
        # Arrange
        uuid = uuid4()
        event = OrderDenied(
            cl_ord_id=ClientOrderId("O-2020872378423"),
            reason="SINGLE_ORDER_RISK_EXCEEDED",
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert f"OrderDenied(cl_ord_id=O-2020872378423, reason='SINGLE_ORDER_RISK_EXCEEDED', event_id={uuid})", (
            str(event)
        )
        assert f"OrderDenied(cl_ord_id=O-2020872378423, reason='SINGLE_ORDER_RISK_EXCEEDED', event_id={uuid})", (
            repr(event)
        )

    def test_order_submitted(self):
        # Arrange
        uuid = uuid4()
        event = OrderSubmitted(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            submitted_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert f"OrderSubmitted(account_id=SIM-000, cl_ord_id=O-2020872378423, event_id={uuid})", (
            str(event)
        )
        assert f"OrderSubmitted(account_id=SIM-000, cl_ord_id=O-2020872378423, event_id={uuid})", (
            repr(event)
        )

    def test_order_rejected(self):
        # Arrange
        uuid = uuid4()
        event = OrderRejected(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            rejected_ns=0,
            reason="INSUFFICIENT_MARGIN",
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert f"OrderRejected(account_id=SIM-000, cl_ord_id=O-2020872378423, reason='INSUFFICIENT_MARGIN', event_id={uuid})", str(
            event
        )  # noqa
        assert f"OrderRejected(account_id=SIM-000, cl_ord_id=O-2020872378423, reason='INSUFFICIENT_MARGIN', event_id={uuid})", repr(
            event
        )  # noqa

    def test_order_accepted(self, order_id=None):
        if order_id is None:
            order_id = OrderId("123456")

        # Arrange
        uuid = uuid4()
        event = OrderAccepted(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            order_id=order_id,
            accepted_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert f"OrderAccepted(account_id=SIM-000, cl_ord_id=O-2020872378423, order_id={123456}, event_id={uuid})", str(
            event
        )  # noqa
        assert f"OrderAccepted(account_id=SIM-000, cl_ord_id=O-2020872378423, order_id={123456}, event_id={uuid})", repr(
            event
        )  # noqa

    def test_order_update_reject(self):
        # Arrange
        uuid = uuid4()
        event = OrderUpdateRejected(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            order_id=OrderId("123456"),
            rejected_ns=0,
            response_to="O-2020872378423",
            reason="ORDER_DOES_NOT_EXIST",
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert (
            f"OrderUpdateRejected(account_id=SIM-000, cl_ord_id=O-2020872378423, "
            f"response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})" == str(event)
        )
        assert (
            f"OrderUpdateRejected(account_id=SIM-000, cl_ord_id=O-2020872378423, "
            f"response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})" == repr(event)
        )

    def test_order_cancel_reject(self):
        # Arrange
        uuid = uuid4()
        event = OrderCancelRejected(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            order_id=OrderId("123456"),
            rejected_ns=0,
            response_to="O-2020872378423",
            reason="ORDER_DOES_NOT_EXIST",
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert (
            f"OrderCancelRejected(account_id=SIM-000, cl_ord_id=O-2020872378423, "
            f"response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})" == str(event)
        )
        assert (
            f"OrderCancelRejected(account_id=SIM-000, cl_ord_id=O-2020872378423, "
            f"response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})" == repr(event)
        )

    def test_order_cancelled(self):
        # Arrange
        uuid = uuid4()
        event = OrderCancelled(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            order_id=OrderId("123456"),
            cancelled_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert (
            f"OrderCancelled(account_id=SIM-000, cl_ord_id=O-2020872378423, "
            f"order_id=123456, event_id={uuid})" == str(event)
        )
        assert (
            f"OrderCancelled(account_id=SIM-000, cl_ord_id=O-2020872378423, "
            f"order_id=123456, event_id={uuid})" == repr(event)
        )

    def test_order_amended(self):
        # Arrange
        uuid = uuid4()
        event = OrderUpdated(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            order_id=OrderId("123456"),
            quantity=Quantity(500000),
            price=Price("1.95000"),
            updated_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert (
            f"OrderUpdated(account_id=SIM-000, cl_order_id=O-2020872378423, "
            f"order_id=123456, qty=500,000, price=1.95000, event_id={uuid})"
            == str(event)
        )
        assert (
            f"OrderUpdated(account_id=SIM-000, cl_order_id=O-2020872378423, "
            f"order_id=123456, qty=500,000, price=1.95000, event_id={uuid})"
            == repr(event)
        )

    def test_order_expired(self):
        # Arrange
        uuid = uuid4()
        event = OrderExpired(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            order_id=OrderId("123456"),
            expired_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act
        assert (
            f"OrderExpired(account_id=SIM-000, cl_ord_id=O-2020872378423, order_id=123456, event_id={uuid})"
            == str(event)
        )
        assert (
            f"OrderExpired(account_id=SIM-000, cl_ord_id=O-2020872378423, order_id=123456, event_id={uuid})"
            == repr(event)
        )

    def test_order_filled(self):
        # Arrange
        uuid = uuid4()
        event = OrderFilled(
            account_id=AccountId("SIM", "000"),
            cl_ord_id=ClientOrderId("O-2020872378423"),
            order_id=OrderId("123456"),
            execution_id=ExecutionId("1"),
            position_id=PositionId("2"),
            strategy_id=StrategyId("SCALPER", "001"),
            instrument_id=InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE")),
            order_side=OrderSide.BUY,
            last_qty=Quantity("0.561000"),
            last_px=Price("15600.12445"),
            cum_qty=Quantity("0.561000"),
            leaves_qty=Quantity(0),
            currency=USDT,
            is_inverse=False,
            commission=Money("12.20000000", USDT),
            liquidity_side=LiquiditySide.MAKER,
            execution_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        print(event)
        # Act
        assert (
            f"OrderFilled(account_id=SIM-000, cl_ord_id=O-2020872378423, "
            f"order_id=123456, position_id=2, strategy_id=SCALPER-001, "
            f"instrument_id=BTC/USDT.BINANCE, side=BUY-MAKER, last_qty=0.561000, "
            f"last_px=15600.12445 USDT, cum_qty=0.561000, leaves_qty=0, "
            f"commission=12.20000000 USDT, event_id={uuid})" == str(event)
        )
        assert (
            f"OrderFilled(account_id=SIM-000, cl_ord_id=O-2020872378423, "
            f"order_id=123456, position_id=2, strategy_id=SCALPER-001, "
            f"instrument_id=BTC/USDT.BINANCE, side=BUY-MAKER, last_qty=0.561000, "
            f"last_px=15600.12445 USDT, cum_qty=0.561000, leaves_qty=0, "
            f"commission=12.20000000 USDT, event_id={uuid})" == repr(event)
        )
