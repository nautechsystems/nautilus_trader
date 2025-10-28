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

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderDenied
from nautilus_trader.model.events import OrderEmulated
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderPendingCancel
from nautilus_trader.model.events import OrderPendingUpdate
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderReleased
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.events import OrderTriggered
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestModelEvents:
    def test_account_state_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        balance = AccountBalance(
            total=Money(1525000, USD),
            locked=Money(0, USD),
            free=Money(1525000, USD),
        )
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[balance],
            margins=[],
            info={},
            event_id=uuid,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert AccountState.from_dict(AccountState.to_dict(event)) == event
        assert (
            str(event)
            == f"AccountState(account_id=SIM-000, account_type=MARGIN, base_currency=USD, is_reported=True, balances=[AccountBalance(total=1_525_000.00 USD, locked=0.00 USD, free=1_525_000.00 USD)], margins=[], event_id={uuid})"
        )
        assert (
            repr(event)
            == f"AccountState(account_id=SIM-000, account_type=MARGIN, base_currency=USD, is_reported=True, balances=[AccountBalance(total=1_525_000.00 USD, locked=0.00 USD, free=1_525_000.00 USD)], margins=[], event_id={uuid})"
        )

    def test_account_state_with_margin_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        balance = AccountBalance(
            total=Money(1_525_000, USD),
            locked=Money(25_000, USD),
            free=Money(1_500_000, USD),
        )
        margin = MarginBalance(
            initial=Money(5_000, USD),
            maintenance=Money(20_000, USD),
            instrument_id=AUDUSD_SIM.id,
        )
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[balance],
            margins=[margin],
            info={},
            event_id=uuid,
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert AccountState.from_dict(AccountState.to_dict(event)) == event
        assert (
            str(event)
            == f"AccountState(account_id=SIM-000, account_type=MARGIN, base_currency=USD, is_reported=True, balances=[AccountBalance(total=1_525_000.00 USD, locked=25_000.00 USD, free=1_500_000.00 USD)], margins=[MarginBalance(initial=5_000.00 USD, maintenance=20_000.00 USD, instrument_id=AUD/USD.SIM)], event_id={uuid})"
        )
        assert (
            repr(event)
            == f"AccountState(account_id=SIM-000, account_type=MARGIN, base_currency=USD, is_reported=True, balances=[AccountBalance(total=1_525_000.00 USD, locked=25_000.00 USD, free=1_500_000.00 USD)], margins=[MarginBalance(initial=5_000.00 USD, maintenance=20_000.00 USD, instrument_id=AUD/USD.SIM)], event_id={uuid})"
        )

    def test_order_initialized_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderInitialized(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_str("0.561000"),
            time_in_force=TimeInForce.DAY,
            post_only=True,
            reduce_only=True,
            quote_quantity=False,
            options={"price": "15200.10"},
            emulation_trigger=TriggerType.BID_ASK,
            trigger_instrument_id=TestIdStubs.usdjpy_id(),
            contingency_type=ContingencyType.OTO,
            order_list_id=OrderListId("1"),
            linked_order_ids=[ClientOrderId("O-2020872378424")],
            parent_order_id=None,
            exec_algorithm_id=None,
            exec_algorithm_params=None,
            exec_spawn_id=None,
            tags=["tag-01", "tag-02", "tag-03"],
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderInitialized.from_dict(OrderInitialized.to_dict(event)) == event
        assert (
            str(event)
            == f"OrderInitialized(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, side=BUY, type=LIMIT, quantity=0.561000, time_in_force=DAY, post_only=True, reduce_only=True, quote_quantity=False, options={{'price': '15200.10'}}, emulation_trigger=BID_ASK, trigger_instrument_id=USD/JPY.SIM, contingency_type=OTO, order_list_id=1, linked_order_ids=['O-2020872378424'], parent_order_id=None, exec_algorithm_id=None, exec_algorithm_params=None, exec_spawn_id=None, tags=['tag-01', 'tag-02', 'tag-03'])"  # noqa
        )
        assert (
            repr(event)
            == f"OrderInitialized(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, side=BUY, type=LIMIT, quantity=0.561000, time_in_force=DAY, post_only=True, reduce_only=True, quote_quantity=False, options={{'price': '15200.10'}}, emulation_trigger=BID_ASK, trigger_instrument_id=USD/JPY.SIM, contingency_type=OTO, order_list_id=1, linked_order_ids=['O-2020872378424'], parent_order_id=None, exec_algorithm_id=None, exec_algorithm_params=None, exec_spawn_id=None, tags=['tag-01', 'tag-02', 'tag-03'], event_id={uuid}, ts_init=0)"
        )

    def test_order_denied_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        reason = "Exceeded MAX_ORDER_SUBMIT_RATE"
        event = OrderDenied(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            reason=reason,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert event.reason == reason
        assert OrderDenied.from_dict(OrderDenied.to_dict(event)) == event
        assert (
            str(event)
            == "OrderDenied(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, reason='Exceeded MAX_ORDER_SUBMIT_RATE')"
        )
        assert (
            repr(event)
            == f"OrderDenied(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, reason='Exceeded MAX_ORDER_SUBMIT_RATE', event_id={uuid}, ts_init=0)"
        )

    def test_order_emulated_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderEmulated(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderEmulated.from_dict(OrderEmulated.to_dict(event)) == event
        assert (
            str(event)
            == "OrderEmulated(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423)"
        )
        assert (
            repr(event)
            == f"OrderEmulated(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, event_id={uuid}, ts_init=0)"
        )

    def test_order_released_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderReleased(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            released_price=Price.from_str("50200.10"),
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderReleased.from_dict(OrderReleased.to_dict(event)) == event
        assert (
            str(event)
            == "OrderReleased(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, released_price=50200.10)"
        )
        assert (
            repr(event)
            == f"OrderReleased(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, released_price=50200.10, event_id={uuid}, ts_init=0)"
        )

    def test_order_submitted_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderSubmitted(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderSubmitted.from_dict(OrderSubmitted.to_dict(event)) == event
        assert (
            str(event)
            == "OrderSubmitted(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderSubmitted(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_accepted_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderAccepted(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderAccepted.from_dict(OrderAccepted.to_dict(event)) == event
        assert (
            str(event)
            == "OrderAccepted(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderAccepted(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_rejected_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderRejected(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            account_id=AccountId("SIM-000"),
            reason="INSUFFICIENT_MARGIN",
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderRejected.from_dict(OrderRejected.to_dict(event)) == event
        assert (
            str(event)
            == "OrderRejected(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, account_id=SIM-000, reason='INSUFFICIENT_MARGIN', due_post_only=False, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderRejected(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, account_id=SIM-000, reason='INSUFFICIENT_MARGIN', due_post_only=False, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_rejected_with_due_post_only(self):
        # Arrange
        uuid = UUID4()
        event = OrderRejected(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            account_id=AccountId("SIM-000"),
            reason="POST_ONLY_WOULD_EXECUTE",
            ts_event=0,
            event_id=uuid,
            ts_init=0,
            due_post_only=True,
        )

        # Act
        assert event.due_post_only is True

        # Test serialization
        event_dict = OrderRejected.to_dict(event)
        assert event_dict["due_post_only"] is True

        # Test deserialization
        event_from_dict = OrderRejected.from_dict(event_dict)
        assert event_from_dict.due_post_only is True
        assert event_from_dict == event

    def test_order_rejected_default_due_post_only(self):
        # Arrange
        uuid = UUID4()
        event = OrderRejected(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            account_id=AccountId("SIM-000"),
            reason="INSUFFICIENT_MARGIN",
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert event.due_post_only is False

        # Test serialization
        event_dict = OrderRejected.to_dict(event)
        assert event_dict["due_post_only"] is False

    def test_order_canceled_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderCanceled(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderCanceled.from_dict(OrderCanceled.to_dict(event)) == event
        assert (
            str(event)
            == "OrderCanceled(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderCanceled(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_expired_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderExpired(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderExpired.from_dict(OrderExpired.to_dict(event)) == event
        assert (
            str(event)
            == "OrderExpired(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderExpired(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_triggered_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderTriggered(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderTriggered.from_dict(OrderTriggered.to_dict(event)) == event
        assert (
            str(event)
            == "OrderTriggered(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderTriggered(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_pending_update_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderPendingUpdate(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderPendingUpdate.from_dict(OrderPendingUpdate.to_dict(event)) == event
        assert (
            str(event)
            == "OrderPendingUpdate(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderPendingUpdate(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_pending_update_event_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderPendingUpdate(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=None,
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderPendingUpdate.from_dict(OrderPendingUpdate.to_dict(event)) == event
        assert (
            str(event)
            == "OrderPendingUpdate(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=None, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderPendingUpdate(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=None, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_pending_cancel_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderPendingCancel(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderPendingCancel.from_dict(OrderPendingCancel.to_dict(event)) == event
        assert (
            str(event)
            == "OrderPendingCancel(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderPendingCancel(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_pending_cancel_event_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderPendingCancel(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=None,
            account_id=AccountId("SIM-000"),
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderPendingCancel.from_dict(OrderPendingCancel.to_dict(event)) == event
        assert (
            str(event)
            == "OrderPendingCancel(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=None, account_id=SIM-000, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderPendingCancel(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=None, account_id=SIM-000, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_modify_rejected_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderModifyRejected(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            reason="ORDER_DOES_NOT_EXIST",
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderModifyRejected.from_dict(OrderModifyRejected.to_dict(event)) == event
        assert (
            str(event)
            == "OrderModifyRejected(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderModifyRejected(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_modify_rejected_event_with_none_venue_order_id_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderModifyRejected(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=None,
            account_id=AccountId("SIM-000"),
            reason="ORDER_DOES_NOT_EXIST",
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderModifyRejected.from_dict(OrderModifyRejected.to_dict(event)) == event
        assert (
            str(event)
            == "OrderModifyRejected(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=None, account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderModifyRejected(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=None, account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_cancel_rejected_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderCancelRejected(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            reason="ORDER_DOES_NOT_EXIST",
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderCancelRejected.from_dict(OrderCancelRejected.to_dict(event)) == event
        assert (
            str(event)
            == "OrderCancelRejected(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderCancelRejected(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_cancel_rejected_with_none_venue_order_id_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderCancelRejected(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=None,
            account_id=AccountId("SIM-000"),
            reason="ORDER_DOES_NOT_EXIST",
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderCancelRejected.from_dict(OrderCancelRejected.to_dict(event)) == event
        assert (
            str(event)
            == "OrderCancelRejected(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=None, account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderCancelRejected(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=None, account_id=SIM-000, reason='ORDER_DOES_NOT_EXIST', event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_updated_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderUpdated(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            quantity=Quantity.from_int(500_000),
            price=Price.from_str("1.95000"),
            trigger_price=None,
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert OrderUpdated.from_dict(OrderUpdated.to_dict(event)) == event
        assert (
            str(event)
            == "OrderUpdated(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, quantity=500_000, price=1.95000, trigger_price=None, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderUpdated(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, quantity=500_000, price=1.95000, trigger_price=None, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_order_filled_event_to_from_dict_and_str_repr(self):
        # Arrange
        uuid = UUID4()
        event = OrderFilled(
            trader_id=TraderId("TRADER-001"),
            strategy_id=StrategyId("SCALPER-001"),
            instrument_id=InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            client_order_id=ClientOrderId("O-2020872378423"),
            venue_order_id=VenueOrderId("123456"),
            account_id=AccountId("SIM-000"),
            trade_id=TradeId("1"),
            position_id=PositionId("2"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            last_qty=Quantity.from_str("0.561000"),
            last_px=Price.from_str("15600.12445"),
            currency=USDT,
            commission=Money(12.20000000, USDT),
            liquidity_side=LiquiditySide.MAKER,
            ts_event=0,
            event_id=uuid,
            ts_init=0,
        )

        # Act, Assert
        assert event.is_buy
        assert not event.is_sell
        assert OrderFilled.from_dict(OrderFilled.to_dict(event)) == event
        assert (
            str(event)
            == "OrderFilled(instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, trade_id=1, position_id=2, order_side=BUY, order_type=LIMIT, last_qty=0.561000, last_px=15_600.12445 USDT, commission=12.20000000 USDT, liquidity_side=MAKER, ts_event=0)"
        )
        assert (
            repr(event)
            == f"OrderFilled(trader_id=TRADER-001, strategy_id=SCALPER-001, instrument_id=BTCUSDT.BINANCE, client_order_id=O-2020872378423, venue_order_id=123456, account_id=SIM-000, trade_id=1, position_id=2, order_side=BUY, order_type=LIMIT, last_qty=0.561000, last_px=15_600.12445 USDT, commission=12.20000000 USDT, liquidity_side=MAKER, event_id={uuid}, ts_event=0, ts_init=0)"
        )

    def test_account_state_copies_balance_objects(self):
        """
        Test that AccountState properly copies AccountBalance objects to prevent
        mutations from affecting previously stored events.

        This addresses issue #2701 where mutable AccountBalance references caused
        inconsistent account reporting.

        """
        # Arrange
        original_balance = AccountBalance(
            total=Money(1000, USD),
            locked=Money(100, USD),
            free=Money(900, USD),
        )

        # Create AccountState which should copy the balance
        account_state = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[original_balance],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act - Modify the original balance after AccountState creation
        # This simulates what happens when the account balance gets updated
        modified_balance = AccountBalance(
            total=Money(2000, USD),  # Changed values
            locked=Money(200, USD),
            free=Money(1800, USD),
        )

        # Assert - The balance stored in AccountState should be independent
        stored_balance = account_state.balances[0]

        # The stored balance should NOT be the same object
        assert stored_balance is not original_balance

        # The stored balance should still have the original values
        assert stored_balance.total == Money(1000, USD)
        assert stored_balance.locked == Money(100, USD)
        assert stored_balance.free == Money(900, USD)

        # The stored balance should NOT have the modified values
        assert stored_balance.total != modified_balance.total
        assert stored_balance.locked != modified_balance.locked
        assert stored_balance.free != modified_balance.free


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
            Quantity.from_int(100_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill)

        uuid = UUID4()
        event = PositionOpened.create(position, fill, uuid, 0)

        # Act, Assert
        assert PositionOpened.from_dict(PositionOpened.to_dict(event)) == event
        assert (
            str(event)
            == "PositionOpened(instrument_id=AUD/USD.SIM, position_id=P-123456, account_id=SIM-000, opening_order_id=O-19700101-000000-000-001-1, closing_order_id=None, entry=BUY, side=LONG, signed_qty=100000.0, quantity=100_000, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=0.0, realized_return=0.00000, realized_pnl=-2.00 USD, unrealized_pnl=0.00 USD, ts_opened=0, ts_last=0, ts_closed=0, duration_ns=0)"
        )
        assert (
            repr(event)
            == f"PositionOpened(trader_id=TESTER-000, strategy_id=S-001, instrument_id=AUD/USD.SIM, position_id=P-123456, account_id=SIM-000, opening_order_id=O-19700101-000000-000-001-1, closing_order_id=None, entry=BUY, side=LONG, signed_qty=100000.0, quantity=100_000, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=0.0, realized_return=0.00000, realized_pnl=-2.00 USD, unrealized_pnl=0.00 USD, ts_opened=0, ts_last=0, ts_closed=0, duration_ns=0, event_id={uuid})"
        )

    def test_position_changed_event_to_from_dict_and_str_repr(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(50_000),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00011"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        uuid = UUID4()
        event = PositionChanged.create(position, fill2, uuid, 0)

        # Act, Assert
        assert PositionChanged.from_dict(PositionChanged.to_dict(event)) == event
        assert (
            str(event)
            == "PositionChanged(instrument_id=AUD/USD.SIM, position_id=P-123456, account_id=SIM-000, opening_order_id=O-19700101-000000-000-001-1, closing_order_id=None, entry=BUY, side=LONG, signed_qty=50000.0, quantity=50_000, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=1.00011, realized_return=0.00010, realized_pnl=2.00 USD, unrealized_pnl=5.00 USD, ts_opened=0, ts_last=0, ts_closed=0, duration_ns=0)"
        )
        assert (
            repr(event)
            == f"PositionChanged(trader_id=TESTER-000, strategy_id=S-001, instrument_id=AUD/USD.SIM, position_id=P-123456, account_id=SIM-000, opening_order_id=O-19700101-000000-000-001-1, closing_order_id=None, entry=BUY, side=LONG, signed_qty=50000.0, quantity=50_000, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=1.00011, realized_return=0.00010, realized_pnl=2.00 USD, unrealized_pnl=5.00 USD, ts_opened=0, ts_last=0, ts_closed=0, duration_ns=0, event_id={uuid})"
        )

    def test_position_closed_event_to_from_dict_and_str_repr(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00001"),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("1.00011"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)
        position.apply(fill2)

        uuid = UUID4()
        event = PositionClosed.create(position, fill2, uuid, 0)

        # Act, Assert
        assert PositionClosed.from_dict(PositionClosed.to_dict(event)) == event
        assert (
            str(event)
            == "PositionClosed(instrument_id=AUD/USD.SIM, position_id=P-123456, account_id=SIM-000, opening_order_id=O-19700101-000000-000-001-1, closing_order_id=O-19700101-000000-000-001-2, entry=BUY, side=FLAT, signed_qty=0.0, quantity=0, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=1.00011, realized_return=0.00010, realized_pnl=6.00 USD, unrealized_pnl=0.00 USD, ts_opened=0, ts_last=0, ts_closed=0, duration_ns=0)"
        )
        assert (
            repr(event)
            == f"PositionClosed(trader_id=TESTER-000, strategy_id=S-001, instrument_id=AUD/USD.SIM, position_id=P-123456, account_id=SIM-000, opening_order_id=O-19700101-000000-000-001-1, closing_order_id=O-19700101-000000-000-001-2, entry=BUY, side=FLAT, signed_qty=0.0, quantity=0, peak_qty=100_000, currency=USD, avg_px_open=1.00001, avg_px_close=1.00011, realized_return=0.00010, realized_pnl=6.00 USD, unrealized_pnl=0.00 USD, ts_opened=0, ts_last=0, ts_closed=0, duration_ns=0, event_id={uuid})"
        )
