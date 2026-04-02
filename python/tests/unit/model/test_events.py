# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountBalance
from nautilus_trader.model import AccountState
from nautilus_trader.model import AccountType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import Currency
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import Money
from nautilus_trader.model import OrderAccepted
from nautilus_trader.model import OrderCanceled
from nautilus_trader.model import OrderCancelRejected
from nautilus_trader.model import OrderDenied
from nautilus_trader.model import OrderEmulated
from nautilus_trader.model import OrderExpired
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderModifyRejected
from nautilus_trader.model import OrderPendingCancel
from nautilus_trader.model import OrderPendingUpdate
from nautilus_trader.model import OrderRejected
from nautilus_trader.model import OrderReleased
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderSubmitted
from nautilus_trader.model import OrderTriggered
from nautilus_trader.model import OrderType
from nautilus_trader.model import OrderUpdated
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import TradeId
from nautilus_trader.model import VenueOrderId


@pytest.fixture
def uuid():
    return UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2")


@pytest.fixture
def client_order_id():
    return ClientOrderId("O-20210410-022422-001-001-1")


@pytest.fixture
def venue_order_id():
    return VenueOrderId("123456")


def test_account_state_construction(account_id, uuid):
    balance = AccountBalance(
        total=Money.from_str("1_000_000 USD"),
        locked=Money.from_str("0 USD"),
        free=Money.from_str("1_000_000 USD"),
    )

    state = AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        balances=[balance],
        margins=[],
        is_reported=True,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    assert state.account_id == account_id
    assert state.account_type == AccountType.CASH
    assert len(state.balances) == 1


def test_account_state_to_dict_and_from_dict_roundtrip(account_id, uuid):
    balance = AccountBalance(
        total=Money.from_str("1_000_000 USD"),
        locked=Money.from_str("0 USD"),
        free=Money.from_str("1_000_000 USD"),
    )

    state = AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        balances=[balance],
        margins=[],
        is_reported=True,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        base_currency=Currency.from_str("USD"),
    )

    d = state.to_dict()
    restored = AccountState.from_dict(d)

    assert restored == state


def test_order_denied(trader_id, strategy_id, audusd_id, client_order_id, uuid):
    event = OrderDenied(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        reason="Exceeded MAX_ORDER_SUBMIT_RATE",
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    assert event.trader_id == trader_id
    assert event.strategy_id == strategy_id
    assert event.instrument_id == audusd_id
    assert event.client_order_id == client_order_id
    assert event.reason == "Exceeded MAX_ORDER_SUBMIT_RATE"
    assert "OrderDenied" in repr(event)
    assert "AUD/USD.SIM" in str(event)


def test_order_denied_to_dict_roundtrip(trader_id, strategy_id, audusd_id, client_order_id, uuid):
    event = OrderDenied(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        reason="Exceeded MAX_ORDER_SUBMIT_RATE",
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    restored = OrderDenied.from_dict(event.to_dict())

    assert restored == event


def test_order_submitted(trader_id, strategy_id, audusd_id, account_id, client_order_id, uuid):
    event = OrderSubmitted(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    assert event.client_order_id == client_order_id
    assert event.account_id == account_id
    assert "OrderSubmitted" in repr(event)


def test_order_submitted_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    uuid,
):
    event = OrderSubmitted(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    restored = OrderSubmitted.from_dict(event.to_dict())

    assert restored == event


def test_order_accepted(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderAccepted(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        venue_order_id=venue_order_id,
        account_id=account_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
    )

    assert event.venue_order_id == venue_order_id
    assert event.reconciliation is False
    assert "OrderAccepted" in repr(event)


def test_order_accepted_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderAccepted(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        venue_order_id=venue_order_id,
        account_id=account_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
    )

    restored = OrderAccepted.from_dict(event.to_dict())

    assert restored == event


def test_order_rejected(trader_id, strategy_id, audusd_id, account_id, client_order_id, uuid):
    event = OrderRejected(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        reason="Insufficient margin",
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
    )

    assert event.reason == "Insufficient margin"
    assert "OrderRejected" in repr(event)


def test_order_rejected_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    uuid,
):
    event = OrderRejected(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        reason="Insufficient margin",
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
    )

    restored = OrderRejected.from_dict(event.to_dict())

    assert restored == event


def test_order_canceled(trader_id, strategy_id, audusd_id, client_order_id, venue_order_id, uuid):
    event = OrderCanceled(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
    )

    assert event.venue_order_id == venue_order_id
    assert "OrderCanceled" in repr(event)


def test_order_canceled_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderCanceled(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
    )

    restored = OrderCanceled.from_dict(event.to_dict())

    assert restored == event


def test_order_expired(trader_id, strategy_id, audusd_id, client_order_id, venue_order_id, uuid):
    event = OrderExpired(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
    )

    assert "OrderExpired" in repr(event)


def test_order_expired_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderExpired(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
    )

    restored = OrderExpired.from_dict(event.to_dict())

    assert restored == event


def test_order_triggered(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderTriggered(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
        account_id=account_id,
    )

    assert "OrderTriggered" in repr(event)


def test_order_triggered_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderTriggered(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
        account_id=account_id,
    )

    restored = OrderTriggered.from_dict(event.to_dict())

    assert restored == event


def test_order_emulated(trader_id, strategy_id, audusd_id, client_order_id, uuid):
    event = OrderEmulated(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    assert "OrderEmulated" in repr(event)


def test_order_emulated_to_dict_roundtrip(trader_id, strategy_id, audusd_id, client_order_id, uuid):
    event = OrderEmulated(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    restored = OrderEmulated.from_dict(event.to_dict())

    assert restored == event


def test_order_released(trader_id, strategy_id, audusd_id, client_order_id, uuid):
    event = OrderReleased(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        released_price=Price.from_str("1.00000"),
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    assert event.released_price == Price.from_str("1.00000")
    assert "OrderReleased" in repr(event)


def test_order_released_to_dict_roundtrip(trader_id, strategy_id, audusd_id, client_order_id, uuid):
    event = OrderReleased(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        released_price=Price.from_str("1.00000"),
        event_id=uuid,
        ts_event=0,
        ts_init=0,
    )

    restored = OrderReleased.from_dict(event.to_dict())

    assert restored == event


def test_order_updated(trader_id, strategy_id, audusd_id, client_order_id, venue_order_id, uuid):
    event = OrderUpdated(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        quantity=Quantity.from_int(500_000),
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
        price=Price.from_str("1.00010"),
        trigger_price=Price.from_str("1.00005"),
    )

    assert event.quantity == Quantity.from_int(500_000)
    assert event.price == Price.from_str("1.00010")
    assert event.trigger_price == Price.from_str("1.00005")
    assert "OrderUpdated" in repr(event)


def test_order_updated_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderUpdated(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        quantity=Quantity.from_int(500_000),
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
        price=Price.from_str("1.00010"),
    )

    restored = OrderUpdated.from_dict(event.to_dict())

    assert restored == event


def test_order_pending_update(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderPendingUpdate(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
    )

    assert "OrderPendingUpdate" in repr(event)


def test_order_pending_update_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderPendingUpdate(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
    )

    restored = OrderPendingUpdate.from_dict(event.to_dict())

    assert restored == event


def test_order_pending_cancel(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderPendingCancel(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
    )

    assert "OrderPendingCancel" in repr(event)


def test_order_pending_cancel_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderPendingCancel(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
    )

    restored = OrderPendingCancel.from_dict(event.to_dict())

    assert restored == event


def test_order_modify_rejected(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderModifyRejected(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        reason="ORDER_DOES_NOT_EXIST",
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
        account_id=account_id,
    )

    assert event.reason == "ORDER_DOES_NOT_EXIST"
    assert "OrderModifyRejected" in repr(event)


def test_order_modify_rejected_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderModifyRejected(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        reason="ORDER_DOES_NOT_EXIST",
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
        account_id=account_id,
    )

    restored = OrderModifyRejected.from_dict(event.to_dict())

    assert restored == event


def test_order_cancel_rejected(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderCancelRejected(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        reason="ORDER_DOES_NOT_EXIST",
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
        account_id=account_id,
    )

    assert event.reason == "ORDER_DOES_NOT_EXIST"
    assert "OrderCancelRejected" in repr(event)


def test_order_cancel_rejected_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderCancelRejected(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        reason="ORDER_DOES_NOT_EXIST",
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        venue_order_id=venue_order_id,
        account_id=account_id,
    )

    restored = OrderCancelRejected.from_dict(event.to_dict())

    assert restored == event


def test_order_filled(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderFilled(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        venue_order_id=venue_order_id,
        account_id=account_id,
        trade_id=TradeId("1"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("1.00000"),
        currency=Currency.from_str("USD"),
        liquidity_side=LiquiditySide.MAKER,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        commission=Money.from_str("2.00 USD"),
    )

    assert event.is_buy
    assert not event.is_sell
    assert event.order_side == OrderSide.BUY
    assert event.order_type == OrderType.LIMIT
    assert event.last_qty == Quantity.from_int(100_000)
    assert event.last_px == Price.from_str("1.00000")
    assert event.commission == Money.from_str("2.00 USD")
    assert event.liquidity_side == LiquiditySide.MAKER
    assert "OrderFilled" in repr(event)


def test_order_filled_to_dict_roundtrip(
    trader_id,
    strategy_id,
    audusd_id,
    account_id,
    client_order_id,
    venue_order_id,
    uuid,
):
    event = OrderFilled(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        venue_order_id=venue_order_id,
        account_id=account_id,
        trade_id=TradeId("1"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("1.00000"),
        currency=Currency.from_str("USD"),
        liquidity_side=LiquiditySide.MAKER,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
        commission=Money.from_str("2.00 USD"),
    )

    restored = OrderFilled.from_dict(event.to_dict())

    assert restored == event
