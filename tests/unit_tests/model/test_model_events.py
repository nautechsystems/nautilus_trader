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
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderUpdateRejected
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestEvents:
    def test_account_state(self):
        # Arrange
        uuid = uuid4()
        balance = AccountBalance(
            currency=USD,
            total=Money(1525000, USD),
            locked=Money(0, USD),
            free=Money(1525000, USD),
        )
        event = AccountState(
            account_id=AccountId("SIM", "000"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[balance],
            info={},
            event_id=uuid,
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        # Act, Assert
        assert AccountState.from_dict(AccountState.to_dict(event)) == event
        assert (
            f"AccountState(account_id=SIM-000, account_type=MARGIN, base_currency=USD, is_reported=True, balances=[AccountBalance(total=1_525_000.00 USD, locked=0.00 USD, free=1_525_000.00 USD)], event_id={uuid})"  # noqa
            == str(event)
        )
        assert (
            f"AccountState(account_id=SIM-000, account_type=MARGIN, base_currency=USD, is_reported=True, balances=[AccountBalance(total=1_525_000.00 USD, locked=0.00 USD, free=1_525_000.00 USD)], event_id={uuid})"  # noqa
            == repr(event)
        )

    def test_order_initialized(self):
        # Arrange
        uuid = uuid4()
        event = OrderInitialized(
            client_order_id=ClientOrderId("O-2020872378423"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE")),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_str("0.561000"),
            time_in_force=TimeInForce.DAY,
            event_id=uuid,
            timestamp_ns=0,
            options={"Price": "15200.10"},
        )

        # Act, Assert
        assert OrderInitialized.from_dict(OrderInitialized.to_dict(event)) == event
        assert (
            f"OrderInitialized(client_order_id=O-2020872378423, strategy_id=SCALPER-001, event_id={uuid})"
            == str(event)
        )
        assert (
            f"OrderInitialized(client_order_id=O-2020872378423, strategy_id=SCALPER-001, event_id={uuid})"
            == repr(event)
        )

    def test_order_denied(self):
        # Arrange
        uuid = uuid4()
        event = OrderDenied(
            client_order_id=ClientOrderId("O-2020872378423"),
            reason="Exceeded MAX_ORDER_RATE",
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderDenied.from_dict(OrderDenied.to_dict(event)) == event
        assert f"OrderDenied(client_order_id=O-2020872378423, reason=Exceeded MAX_ORDER_RATE, event_id={uuid})", (
            str(event)
        )
        assert f"OrderDenied(client_order_id=O-2020872378423, reason=Exceeded MAX_ORDER_RATE, event_id={uuid})", (
            repr(event)
        )

    def test_order_submitted(self):
        # Arrange
        uuid = uuid4()
        event = OrderSubmitted(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            ts_submitted_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderSubmitted.from_dict(OrderSubmitted.to_dict(event)) == event
        assert (
            f"OrderSubmitted(account_id=SIM-000, client_order_id=O-2020872378423, event_id={uuid})"
        ), str(event)
        assert (
            f"OrderSubmitted(account_id=SIM-000, client_order_id=O-2020872378423, event_id={uuid})"
        ), repr(event)

    def test_order_rejected(self):
        # Arrange
        uuid = uuid4()
        event = OrderRejected(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            reason="INSUFFICIENT_MARGIN",
            ts_rejected_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderRejected.from_dict(OrderRejected.to_dict(event)) == event
        assert f"OrderRejected(account_id=SIM-000, client_order_id=O-2020872378423, reason='INSUFFICIENT_MARGIN', event_id={uuid})", str(
            event
        )  # noqa
        assert f"OrderRejected(account_id=SIM-000, client_order_id=O-2020872378423, reason='INSUFFICIENT_MARGIN', event_id={uuid})", repr(
            event
        )  # noqa

    def test_order_accepted(self, venue_order_id=None):
        if venue_order_id is None:
            venue_order_id = VenueOrderId("123456")

        # Arrange
        uuid = uuid4()
        event = OrderAccepted(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=venue_order_id,
            ts_accepted_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderAccepted.from_dict(OrderAccepted.to_dict(event)) == event
        assert f"OrderAccepted(account_id=SIM-000, client_order_id=O-2020872378423, venue_order_id={123456}, event_id={uuid})", str(
            event
        )  # noqa
        assert f"OrderAccepted(account_id=SIM-000, client_order_id=O-2020872378423, venue_order_id={123456}, event_id={uuid})", repr(
            event
        )  # noqa

    def test_order_update_rejected(self):
        # Arrange
        uuid = uuid4()
        event = OrderUpdateRejected(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            response_to="O-2020872378423",
            reason="ORDER_DOES_NOT_EXIST",
            ts_rejected_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderUpdateRejected.from_dict(OrderUpdateRejected.to_dict(event)) == event
        assert (
            f"OrderUpdateRejected(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})" == str(event)
        )
        assert (
            f"OrderUpdateRejected(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})" == repr(event)
        )

    def test_order_cancel_rejected(self):
        # Arrange
        uuid = uuid4()
        event = OrderCancelRejected(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            response_to="O-2020872378423",
            reason="ORDER_DOES_NOT_EXIST",
            ts_rejected_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderCancelRejected.from_dict(OrderCancelRejected.to_dict(event)) == event
        assert (
            f"OrderCancelRejected(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})" == str(event)
        )
        assert (
            f"OrderCancelRejected(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})" == repr(event)
        )

    def test_order_canceled(self):
        # Arrange
        uuid = uuid4()
        event = OrderCanceled(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            ts_canceled_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderCanceled.from_dict(OrderCanceled.to_dict(event)) == event
        assert (
            f"OrderCanceled(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, event_id={uuid})" == str(event)
        )
        assert (
            f"OrderCanceled(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, event_id={uuid})" == repr(event)
        )

    def test_order_updated(self):
        # Arrange
        uuid = uuid4()
        event = OrderUpdated(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            quantity=Quantity.from_int(500000),
            price=Price.from_str("1.95000"),
            trigger=None,
            ts_updated_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderUpdated.from_dict(OrderUpdated.to_dict(event)) == event
        assert (
            f"OrderUpdated(account_id=SIM-000, cl_order_id=O-2020872378423, "
            f"venue_order_id=123456, qty=500_000, price=1.95000, trigger=None, event_id={uuid})"
            == str(event)
        )
        assert (
            f"OrderUpdated(account_id=SIM-000, cl_order_id=O-2020872378423, "
            f"venue_order_id=123456, qty=500_000, price=1.95000, trigger=None, event_id={uuid})"
            == repr(event)
        )

    def test_order_expired(self):
        # Arrange
        uuid = uuid4()
        event = OrderExpired(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            ts_expired_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderExpired.from_dict(OrderExpired.to_dict(event)) == event
        assert (
            f"OrderExpired(account_id=SIM-000, client_order_id=O-2020872378423, venue_order_id=123456, event_id={uuid})"
            == str(event)
        )
        assert (
            f"OrderExpired(account_id=SIM-000, client_order_id=O-2020872378423, venue_order_id=123456, event_id={uuid})"
            == repr(event)
        )

    def test_order_filled(self):
        # Arrange
        uuid = uuid4()
        event = OrderFilled(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            execution_id=ExecutionId("1"),
            position_id=PositionId("2"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE")),
            order_side=OrderSide.BUY,
            last_qty=Quantity.from_str("0.561000"),
            last_px=Price.from_str("15600.12445"),
            currency=USDT,
            commission=Money(12.20000000, USDT),
            liquidity_side=LiquiditySide.MAKER,
            ts_filled_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderFilled.from_dict(OrderFilled.to_dict(event)) == event
        assert (
            f"OrderFilled(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, position_id=2, strategy_id=SCALPER-001, "
            f"instrument_id=BTC/USDT.BINANCE, side=BUY-MAKER, last_qty=0.561000, "
            f"last_px=15600.12445 USDT, "
            f"commission=12.20000000 USDT, event_id={uuid})" == str(event)
        )
        assert (
            f"OrderFilled(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, position_id=2, strategy_id=SCALPER-001, "
            f"instrument_id=BTC/USDT.BINANCE, side=BUY-MAKER, last_qty=0.561000, "
            f"last_px=15600.12445 USDT, "
            f"commission=12.20000000 USDT, event_id={uuid})" == repr(event)
        )
