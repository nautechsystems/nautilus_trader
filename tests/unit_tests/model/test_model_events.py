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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
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
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingUpdate
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdateRejected
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import ExecutionId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestEvents:
    def test_account_state_event_to_from_dict_and_str_repr(self):
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
            str(event)
            == f"AccountState(account_id=SIM-000, account_type=MARGIN, base_currency=USD, is_reported=True, balances=[AccountBalance(total=1_525_000.00 USD, locked=0.00 USD, free=1_525_000.00 USD)], event_id={uuid})"  # noqa
        )
        assert (
            repr(event)
            == f"AccountState(account_id=SIM-000, account_type=MARGIN, base_currency=USD, is_reported=True, balances=[AccountBalance(total=1_525_000.00 USD, locked=0.00 USD, free=1_525_000.00 USD)], event_id={uuid})"  # noqa
        )

    def test_order_initialized_event_to_from_dict_and_str_repr(self):
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
            str(event)
            == f"OrderInitialized(client_order_id=O-2020872378423, strategy_id=SCALPER-001, event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderInitialized(client_order_id=O-2020872378423, strategy_id=SCALPER-001, event_id={uuid})"
        )

    def test_order_denied_event_to_from_dict_and_str_repr(self):
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
        assert (
            str(event)
            == f"OrderDenied(client_order_id=O-2020872378423, reason=Exceeded MAX_ORDER_RATE, event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderDenied(client_order_id=O-2020872378423, reason=Exceeded MAX_ORDER_RATE, event_id={uuid})"
        )

    def test_order_submitted_event_to_from_dict_and_str_repr(self):
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
            str(event)
            == f"OrderSubmitted(account_id=SIM-000, client_order_id=O-2020872378423, event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderSubmitted(account_id=SIM-000, client_order_id=O-2020872378423, event_id={uuid})"
        )

    def test_order_accepted_event_to_from_dict_and_str_repr(self, venue_order_id=None):
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
        assert (
            str(event)
            == f"OrderAccepted(account_id=SIM-000, client_order_id=O-2020872378423, venue_order_id={123456}, event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderAccepted(account_id=SIM-000, client_order_id=O-2020872378423, venue_order_id={123456}, event_id={uuid})"
        )

    def test_order_rejected_event_to_from_dict_and_str_repr(self):
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
        assert (
            str(event)
            == f"OrderRejected(account_id=SIM-000, client_order_id=O-2020872378423, reason='INSUFFICIENT_MARGIN', event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderRejected(account_id=SIM-000, client_order_id=O-2020872378423, reason='INSUFFICIENT_MARGIN', event_id={uuid})"
        )

    def test_order_canceled_event_to_from_dict_and_str_repr(self):
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
            str(event) == f"OrderCanceled(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, event_id={uuid})"
        )
        assert (
            repr(event) == f"OrderCanceled(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, event_id={uuid})"
        )

    def test_order_expired_event_to_from_dict_and_str_repr(self):
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
            str(event)
            == f"OrderExpired(account_id=SIM-000, client_order_id=O-2020872378423, venue_order_id=123456, event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderExpired(account_id=SIM-000, client_order_id=O-2020872378423, venue_order_id=123456, event_id={uuid})"
        )

    def test_order_triggered_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = uuid4()
        event = OrderTriggered(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            ts_triggered_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderTriggered.from_dict(OrderTriggered.to_dict(event)) == event
        assert (
            str(event) == f"OrderTriggered(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, event_id={uuid})"
        )
        assert (
            repr(event) == f"OrderTriggered(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, event_id={uuid})"
        )

    def test_order_pending_update_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = uuid4()
        event = OrderPendingUpdate(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            ts_pending_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderPendingUpdate.from_dict(OrderPendingUpdate.to_dict(event)) == event
        assert (
            str(event)
            == f"OrderPendingUpdate(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, ts_pending_ns=0, event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderPendingUpdate(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, ts_pending_ns=0, event_id={uuid})"
        )

    def test_order_pending_cancel_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = uuid4()
        event = OrderPendingCancel(
            account_id=AccountId("SIM", "000"),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            ts_pending_ns=0,
            event_id=uuid,
            timestamp_ns=0,
        )

        # Act, Assert
        assert OrderPendingCancel.from_dict(OrderPendingCancel.to_dict(event)) == event
        assert (
            str(event)
            == f"OrderPendingCancel(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, ts_pending_ns=0, event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderPendingCancel(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, ts_pending_ns=0, event_id={uuid})"
        )

    def test_order_update_rejected_event_to_from_dict_and_str_repr(self):
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
            str(event)
            == f"OrderUpdateRejected(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderUpdateRejected(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})"
        )

    def test_order_cancel_rejected_event_to_from_dict_and_str_repr(self):
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
            str(event)
            == f"OrderCancelRejected(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})"
        )
        assert (
            repr(event)
            == f"OrderCancelRejected(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, response_to=O-2020872378423, reason='ORDER_DOES_NOT_EXIST', "
            f"event_id={uuid})"
        )

    def test_order_updated_event_to_from_dict_and_str_repr(self):
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
            str(event) == f"OrderUpdated(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, qty=500_000, price=1.95000, trigger=None, event_id={uuid})"
        )
        assert (
            repr(event) == f"OrderUpdated(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, qty=500_000, price=1.95000, trigger=None, event_id={uuid})"
        )

    def test_order_filled_event_to_from_dict_and_str_repr(self):
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
        assert event.is_buy
        assert not event.is_sell
        assert OrderFilled.from_dict(OrderFilled.to_dict(event)) == event
        assert (
            str(event) == f"OrderFilled(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, position_id=2, strategy_id=SCALPER-001, "
            f"instrument_id=BTC/USDT.BINANCE, side=BUY-MAKER, last_qty=0.561000, "
            f"last_px=15600.12445 USDT, "
            f"commission=12.20000000 USDT, event_id={uuid})"
        )
        assert (
            repr(event) == f"OrderFilled(account_id=SIM-000, client_order_id=O-2020872378423, "
            f"venue_order_id=123456, position_id=2, strategy_id=SCALPER-001, "
            f"instrument_id=BTC/USDT.BINANCE, side=BUY-MAKER, last_qty=0.561000, "
            f"last_px=15600.12445 USDT, "
            f"commission=12.20000000 USDT, event_id={uuid})"
        )


class TestPositionEvents:
    def setup(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER-000"),
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_position_opened_event_to_from_dict_and_str_repr(self):
        # Arrange
        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        uuid = uuid4()
        event = PositionOpened.create(position, fill, uuid, 0)

        # Act, Assert
        assert PositionOpened.from_dict(PositionOpened.to_dict(event)) == event
        assert (
            str(event)
            == f"PositionOpened(position_id=P-123456, instrument_id=AUD/USD.SIM, account_id=SIM-000, from_order=O-19700101-000000-000-001-1, strategy_id=S-001, entry=BUY, side=LONG, net_qty=100000, quantity=100_000, peak_qty=100_000, currency=USD, avg_px_open=1.00001, realized_points=0, realized_return=0, realized_pnl=-2.00, ts_opened_ns=0, event_id={uuid})"  # noqa
        )
        assert (
            repr(event)
            == f"PositionOpened(position_id=P-123456, instrument_id=AUD/USD.SIM, account_id=SIM-000, from_order=O-19700101-000000-000-001-1, strategy_id=S-001, entry=BUY, side=LONG, net_qty=100000, quantity=100_000, peak_qty=100_000, currency=USD, avg_px_open=1.00001, realized_points=0, realized_return=0, realized_pnl=-2.00, ts_opened_ns=0, event_id={uuid})"  # noqa
        )

    def test_position_changed_event_to_from_dict_and_str_repr(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(50000),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00011"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        uuid = uuid4()
        event = PositionChanged.create(position, fill2, uuid, 0)

        # Act, Assert
        assert PositionChanged.from_dict(PositionChanged.to_dict(event)) == event
        assert (
            str(event)
            == f"PositionChanged(position_id=P-123456, instrument_id=AUD/USD.SIM, account_id=SIM-000, from_order=O-19700101-000000-000-001-1, strategy_id=S-001, entry=BUY, side=LONG, net_qty=50000, quantity=50_000, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=1.00011, realized_points=0.00010, realized_return=0.00010, realized_pnl=2.00, ts_opened_ns=0, event_id={uuid})"  # noqa
        )
        assert (
            repr(event)
            == f"PositionChanged(position_id=P-123456, instrument_id=AUD/USD.SIM, account_id=SIM-000, from_order=O-19700101-000000-000-001-1, strategy_id=S-001, entry=BUY, side=LONG, net_qty=50000, quantity=50_000, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=1.00011, realized_points=0.00010, realized_return=0.00010, realized_pnl=2.00, ts_opened_ns=0, event_id={uuid})"  # noqa
        )

    def test_position_closed_event_to_from_dict_and_str_repr(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100000),
        )

        fill1 = TestStubs.event_order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100000),
        )

        fill2 = TestStubs.event_order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00011"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        uuid = uuid4()
        event = PositionClosed.create(position, fill2, uuid, 0)

        # Act, Assert
        assert PositionClosed.from_dict(PositionClosed.to_dict(event)) == event
        assert (
            str(event)
            == f"PositionClosed(position_id=P-123456, instrument_id=AUD/USD.SIM, account_id=SIM-000, from_order=O-19700101-000000-000-001-1, strategy_id=S-001, entry=BUY, side=FLAT, net_qty=0, quantity=0, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=1.00011, realized_points=0.00010, realized_return=0.00010, realized_pnl=6.00, ts_opened_ns=0, ts_closed_ns=0, duration_ns=0, event_id={uuid})"  # noqa
        )
        assert (
            repr(event)
            == f"PositionClosed(position_id=P-123456, instrument_id=AUD/USD.SIM, account_id=SIM-000, from_order=O-19700101-000000-000-001-1, strategy_id=S-001, entry=BUY, side=FLAT, net_qty=0, quantity=0, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=1.00011, realized_points=0.00010, realized_return=0.00010, realized_pnl=6.00, ts_opened_ns=0, ts_closed_ns=0, duration_ns=0, event_id={uuid})"  # noqa
        )
