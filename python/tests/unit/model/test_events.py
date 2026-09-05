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
"""
Test events behavior.
"""

import pytest

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountBalance
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountState
from nautilus_trader.model import AccountType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import Money
from nautilus_trader.model import OrderAccepted
from nautilus_trader.model import OrderCanceled
from nautilus_trader.model import OrderCancelRejected
from nautilus_trader.model import OrderDenied
from nautilus_trader.model import OrderEmulated
from nautilus_trader.model import OrderExpired
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderFillVoided
from nautilus_trader.model import OrderInitialized
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
from nautilus_trader.model import PortfolioSnapshot
from nautilus_trader.model import PositionId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import VenueOrderId


@pytest.fixture
def uuid() -> object:
    """
    Uuid.
    """
    return UUID4.from_str("91762096-b188-49ea-8562-8d8a4cc22ff2")


@pytest.fixture
def client_order_id() -> object:
    """
    Client order id.
    """
    return ClientOrderId("O-20210410-022422-001-001-1")


@pytest.fixture
def venue_order_id() -> object:
    """
    Venue order id.
    """
    return VenueOrderId("123456")


@pytest.fixture
def order_fill_voided(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> object:
    """
    Order fill voided.
    """
    return OrderFillVoided(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        venue_order_id=venue_order_id,
        account_id=account_id,
        correction_id="CORRECTION-001",
        trade_id=TradeId("1"),
        voided_qty=Quantity.from_int(100_000),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        last_px=Price.from_str("1.00000"),
        currency=Currency.from_str("USD"),
        liquidity_side=LiquiditySide.MAKER,
        event_id=uuid,
        ts_event=1,
        ts_init=2,
        reconciliation=False,
        is_reopened=True,
        commission_voided=Money.from_str("2.00 USD"),
        position_id=PositionId("P-001"),
        reason="VENUE_VOID",
        info={"source": "test"},
    )


def test_account_state_construction(account_id: AccountId, uuid: UUID4) -> None:
    """
    Test account state construction.
    """
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
    assert state.is_reported is True
    assert state.event_id == uuid
    assert state.ts_event == 0
    assert state.ts_init == 0


def test_account_state_to_dict_and_from_dict_roundtrip(account_id: AccountId, uuid: UUID4) -> None:
    """
    Test account state to dict and from dict roundtrip.
    """
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


def test_account_state_info_roundtrip(account_id: AccountId, uuid: UUID4) -> None:
    """
    Test account state info roundtrip.
    """
    balance = AccountBalance(
        total=Money.from_str("1_000_000 USD"),
        locked=Money.from_str("0 USD"),
        free=Money.from_str("1_000_000 USD"),
    )
    info = {"total_wallet_balance": 1525.0, "available_balance": 1500.0}

    state = AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        balances=[balance],
        margins=[],
        is_reported=True,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        base_currency=Currency.from_str("USD"),
        info=info,
    )

    assert state.info == info

    restored = AccountState.from_dict(state.to_dict())
    assert restored.info == info


def test_account_state_info_defaults_empty(account_id: AccountId, uuid: UUID4) -> None:
    """
    Test account state info defaults empty.
    """
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

    assert state.info == {}


def test_portfolio_snapshot_valuation_metadata(
    account_id: AccountId,
    audusd_id: InstrumentId,
    uuid: UUID4,
) -> None:
    """
    Test portfolio snapshot valuation metadata.
    """
    usd = Currency.from_str("USD")
    snapshot = PortfolioSnapshot(
        account_id=account_id,
        account_type=AccountType.CASH,
        balances=[],
        margins=[],
        unrealized_pnls=[],
        realized_pnls=[],
        total_equity=[Money.from_str("1_100 USD")],
        event_id=uuid,
        ts_event=1,
        ts_init=2,
        base_currency=usd,
        base_currency_equity=Money.from_str("1_100 USD"),
        is_stale=True,
        stale_instruments=[audusd_id],
        stale_currencies=[Currency.from_str("AUD")],
        unpriced_instruments=[],
    )

    assert snapshot.base_currency_equity == Money.from_str("1_100 USD")
    assert snapshot.is_stale is True
    assert snapshot.stale_instruments == [audusd_id]
    assert snapshot.stale_currencies == [Currency.from_str("AUD")]
    assert snapshot.unpriced_instruments == []


def test_order_event_dicts_preserve_causation_id(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order event dicts preserve causation id.
    """
    events = _make_order_events(
        trader_id,
        strategy_id,
        audusd_id,
        account_id,
        client_order_id,
        venue_order_id,
        uuid,
    )
    causation_id = "38e6a9e5-59a4-4e92-bc1f-2ed5790f9a4b"

    for event in events:
        values = event.to_dict()
        assert values["causation_id"] is None

        values["causation_id"] = causation_id
        restored = type(event).from_dict(values)

        assert restored.to_dict()["causation_id"] == causation_id


def test_order_denied(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order denied.
    """
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


def _make_order_events(
    trader_id: TraderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    event_id: object,
) -> object:
    common = {
        "trader_id": trader_id,
        "strategy_id": strategy_id,
        "instrument_id": instrument_id,
        "client_order_id": client_order_id,
        "event_id": event_id,
        "ts_event": 1,
        "ts_init": 2,
    }
    reconciled = {
        **common,
        "venue_order_id": venue_order_id,
        "account_id": account_id,
        "reconciliation": False,
    }

    return [
        OrderDenied(**common, reason="DENIED"),
        OrderFilled(
            **reconciled,
            trade_id=TradeId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            last_qty=Quantity.from_int(10),
            last_px=Price.from_str("1.00000"),
            currency=Currency.from_str("USD"),
            liquidity_side=LiquiditySide.MAKER,
        ),
        OrderInitialized(
            **common,
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_int(10),
            time_in_force=TimeInForce.GTC,
            post_only=True,
            reduce_only=False,
            quote_quantity=False,
            reconciliation=False,
            price=Price.from_str("1.00000"),
        ),
        OrderRejected(**common, account_id=account_id, reason="REJECTED", reconciliation=False),
        OrderTriggered(**reconciled),
        OrderSubmitted(**common, account_id=account_id),
        OrderEmulated(**common),
        OrderReleased(**common, released_price=Price.from_str("1.00000")),
        OrderUpdated(
            **reconciled,
            quantity=Quantity.from_int(11),
            price=Price.from_str("1.00001"),
        ),
        OrderPendingUpdate(**reconciled),
        OrderPendingCancel(**reconciled),
        OrderModifyRejected(**reconciled, reason="MODIFY_REJECTED"),
        OrderAccepted(**reconciled),
        OrderCancelRejected(**reconciled, reason="CANCEL_REJECTED"),
        OrderCanceled(**reconciled),
        OrderExpired(**reconciled),
    ]


def test_order_denied_to_dict_roundtrip(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order denied to dict roundtrip.
    """
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


def test_order_submitted(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order submitted.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order submitted to dict roundtrip.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order accepted.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order accepted to dict roundtrip.
    """
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


def test_order_rejected(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order rejected.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order rejected to dict roundtrip.
    """
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


def test_order_canceled(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order canceled.
    """
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
        reason="not-enough-liquidity",
    )

    assert event.venue_order_id == venue_order_id
    assert event.reason == "not-enough-liquidity"
    assert "OrderCanceled" in repr(event)


def test_order_canceled_to_dict_roundtrip(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order canceled to dict roundtrip.
    """
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
        reason="not-enough-liquidity",
    )

    restored = OrderCanceled.from_dict(event.to_dict())

    assert restored == event
    assert restored.reason == "not-enough-liquidity"


def test_order_canceled_default_reason_to_dict_roundtrip(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order canceled default reason to dict roundtrip.
    """
    event = OrderCanceled(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        event_id=uuid,
        ts_event=0,
        ts_init=0,
        reconciliation=False,
    )

    restored = OrderCanceled.from_dict(event.to_dict())

    assert restored.reason is None


def test_order_expired(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order expired.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order expired to dict roundtrip.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order triggered.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order triggered to dict roundtrip.
    """
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


def test_order_emulated(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order emulated.
    """
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


def test_order_emulated_to_dict_roundtrip(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order emulated to dict roundtrip.
    """
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


def test_order_released(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order released.
    """
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


def test_order_released_to_dict_roundtrip(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order released to dict roundtrip.
    """
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


def test_order_updated(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order updated.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order updated to dict roundtrip.
    """
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


def test_order_updated_to_dict_roundtrip_preserves_protection_price(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    uuid: UUID4,
) -> None:
    """
    Test order updated to dict roundtrip preserves protection price.
    """
    event = OrderUpdated(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        quantity=Quantity.from_int(500_000),
        event_id=uuid,
        ts_event=1,
        ts_init=2,
        reconciliation=True,
        venue_order_id=venue_order_id,
        account_id=account_id,
        price=Price.from_str("1.00010"),
        trigger_price=Price.from_str("1.00005"),
        protection_price=Price.from_str("0.99900"),
        is_quote_quantity=True,
    )

    restored = OrderUpdated.from_dict(event.to_dict())

    assert event.protection_price == Price.from_str("0.99900")
    assert restored.protection_price == event.protection_price
    assert restored.price == event.price
    assert restored.trigger_price == event.trigger_price
    assert restored.quantity == event.quantity
    assert restored.is_quote_quantity == event.is_quote_quantity
    assert restored.reconciliation == event.reconciliation
    assert restored == event


def test_order_rejected_to_dict_roundtrip_preserves_due_post_only(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    client_order_id: ClientOrderId,
    account_id: AccountId,
    uuid: UUID4,
) -> None:
    """
    Test order rejected to dict roundtrip preserves due post only.
    """
    event = OrderRejected(
        trader_id=trader_id,
        strategy_id=strategy_id,
        instrument_id=audusd_id,
        client_order_id=client_order_id,
        account_id=account_id,
        reason="POST_ONLY_WOULD_TAKE",
        event_id=uuid,
        ts_event=1,
        ts_init=2,
        reconciliation=True,
        due_post_only=True,
    )

    restored = OrderRejected.from_dict(event.to_dict())

    assert event.due_post_only is True
    assert restored.due_post_only is True
    assert restored.reason == event.reason
    assert restored.reconciliation == event.reconciliation
    assert restored == event


def test_order_pending_update(
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order pending update.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order pending update to dict roundtrip.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order pending cancel.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order pending cancel to dict roundtrip.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order modify rejected.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order modify rejected to dict roundtrip.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order cancel rejected.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order cancel rejected to dict roundtrip.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order filled.
    """
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
    trader_id: TraderId,
    strategy_id: StrategyId,
    audusd_id: InstrumentId,
    account_id: AccountId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    uuid: UUID4,
) -> None:
    """
    Test order filled to dict roundtrip.
    """
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


def test_order_fill_voided_to_dict_roundtrip(order_fill_voided: object) -> None:
    """
    Test order fill voided to dict roundtrip.
    """
    restored = OrderFillVoided.from_dict(order_fill_voided.to_dict())

    assert restored == order_fill_voided
    assert order_fill_voided.correction_id == "CORRECTION-001"
    assert order_fill_voided.trade_id == TradeId("1")
    assert order_fill_voided.voided_qty == Quantity.from_int(100_000)
    assert order_fill_voided.commission_voided == Money.from_str("2.00 USD")
    assert order_fill_voided.order_side == OrderSide.BUY
    assert order_fill_voided.order_type == OrderType.LIMIT
    assert order_fill_voided.last_px == Price.from_str("1.00000")
    assert order_fill_voided.currency == Currency.from_str("USD")
    assert order_fill_voided.liquidity_side == LiquiditySide.MAKER
    assert order_fill_voided.position_id == PositionId("P-001")
    assert order_fill_voided.reason == "VENUE_VOID"
    assert order_fill_voided.ts_event == 1
    assert order_fill_voided.ts_init == 2
    assert order_fill_voided.reconciliation is False
    assert order_fill_voided.is_reopened is True
    assert order_fill_voided.info == {"source": "test"}


def test_order_fill_voided_from_dict_rejects_malformed_order_side(
    order_fill_voided: object,
) -> None:
    """
    Test order fill voided from dict rejects malformed order side.
    """
    values = order_fill_voided.to_dict()
    values["order_side"] = "NOT_A_SIDE"

    with pytest.raises(ValueError, match="Matching variant not found"):
        OrderFillVoided.from_dict(values)


def test_order_fill_voided_ordering_raises(order_fill_voided: object) -> None:
    """
    Test order fill voided ordering raises.
    """
    with pytest.raises(TypeError):
        _ = order_fill_voided < order_fill_voided
