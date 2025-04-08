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

from nautilus_trader.accounting.accounts.base import Account
from nautilus_trader.common.enums import ComponentState
from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TradingState
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
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
from nautilus_trader.model.identifiers import ComponentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestEventStubs:
    @staticmethod
    def component_state_changed() -> ComponentStateChanged:
        return ComponentStateChanged(
            trader_id=TestIdStubs.trader_id(),
            component_id=ComponentId("MyActor-001"),
            component_type="MyActor",
            state=ComponentState.RUNNING,
            config={"do_something": True, "trade_size": Decimal("10")},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def trading_state_changed() -> TradingStateChanged:
        return TradingStateChanged(
            trader_id=TestIdStubs.trader_id(),
            state=TradingState.HALTED,
            config={"max_order_submit_rate": "100/00:00:01"},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def cash_account_state(
        account_id: AccountId | None = None,
        base_currency: Currency | None = USD,
    ) -> AccountState:
        balances = [
            AccountBalance(
                Money(1_000_000, USD),
                Money(0, USD),
                Money(1_000_000, USD),
            ),
        ]

        if base_currency is None:
            balances.append(
                AccountBalance(
                    Money(10_000, AUD),
                    Money(0, AUD),
                    Money(10_000, AUD),
                ),
            )

        return AccountState(
            account_id=account_id or TestIdStubs.account_id(),
            account_type=AccountType.CASH,
            base_currency=base_currency,
            reported=True,  # reported
            balances=balances,
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def margin_account_state(account_id: AccountId | None = None) -> AccountState:
        return AccountState(
            account_id=account_id or TestIdStubs.account_id(),
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,  # reported
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[
                MarginBalance(
                    Money(10_000, USD),
                    Money(50_000, USD),
                    TestIdStubs.audusd_id(),
                ),
            ],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def betting_account_state(
        balance: float = 10_000,
        currency: Currency = GBP,
        account_id: AccountId | None = None,
    ) -> AccountState:
        return AccountState(
            account_id=account_id or TestIdStubs.account_id(),
            account_type=AccountType.BETTING,
            base_currency=GBP,
            reported=False,  # reported
            balances=[
                AccountBalance(
                    Money(balance, currency),
                    Money(0, currency),
                    Money(balance, currency),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def order_released(
        order: Order,
        released_price: Price | None = None,
    ) -> OrderReleased:
        return OrderReleased(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            released_price=released_price or Price.from_str("1.00000"),
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_submitted(
        order: Order,
        account_id: AccountId | None = None,
        ts_event: int = 0,
    ) -> OrderSubmitted:
        return OrderSubmitted(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            account_id=account_id or TestIdStubs.account_id(),
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_accepted(
        order: Order,
        account_id: AccountId | None = None,
        venue_order_id: VenueOrderId | None = None,
        ts_event: int = 0,
    ) -> OrderAccepted:
        return OrderAccepted(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id or TestIdStubs.venue_order_id(),
            account_id=account_id or TestIdStubs.account_id(),
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_rejected(
        order: Order,
        account_id: AccountId | None = None,
        ts_event: int = 0,
    ) -> OrderRejected:
        return OrderRejected(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            account_id=account_id or TestIdStubs.account_id(),
            reason="ORDER_REJECTED",
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_pending_update(
        order: Order,
        ts_event: int = 0,
    ) -> OrderPendingUpdate:
        return OrderPendingUpdate(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_updated(
        order: Order,
        quantity: Quantity | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
        ts_event: int = 0,
    ) -> OrderUpdated:
        return OrderUpdated(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            event_id=UUID4(),
            quantity=quantity or order.quantity,
            price=price,
            trigger_price=trigger_price,
            ts_event=ts_event,
            ts_init=0,
        )

    @staticmethod
    def order_pending_cancel(
        order: Order,
        ts_event: int = 0,
    ) -> OrderPendingCancel:
        return OrderPendingCancel(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=order.account_id,
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_filled(
        order: Order,
        instrument: Instrument,
        strategy_id: StrategyId | None = None,
        account_id: AccountId | None = None,
        venue_order_id: VenueOrderId | None = None,
        trade_id: TradeId | None = None,
        position_id: PositionId | None = None,
        last_qty: Quantity | None = None,
        last_px: Price | None = None,
        side: OrderSide | None = None,  # For linearizing: flip side & use 1/price for prob space
        liquidity_side: LiquiditySide = LiquiditySide.TAKER,
        account: Account | None = None,
        ts_event: int = 0,
    ) -> OrderFilled:
        strategy_id = strategy_id or order.strategy_id
        account_id = account_id or order.account_id or TestIdStubs.account_id()
        venue_order_id = venue_order_id or order.venue_order_id or VenueOrderId("1")
        trade_id = trade_id or TradeId(order.client_order_id.value.replace("O", "E"))
        position_id = position_id or order.position_id
        last_qty = last_qty or order.quantity
        last_px = last_px or Price.from_str(f"{1:.{instrument.price_precision}f}")
        order_side = side or order.side

        if account is None:
            # Causes circular import if moved to the top
            from nautilus_trader.test_kit.stubs.execution import TestExecStubs

            account = TestExecStubs.cash_account()
        assert account is not None  # Type checking

        commission = account.calculate_commission(
            instrument=instrument,
            last_qty=order.quantity,
            last_px=last_px,
            liquidity_side=liquidity_side,
        )

        return OrderFilled(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=order.client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            trade_id=trade_id,
            position_id=position_id,
            order_side=order_side,
            order_type=order.order_type,
            last_qty=last_qty,
            last_px=last_px or order.price,
            currency=instrument.quote_currency,
            commission=commission,
            liquidity_side=liquidity_side,
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_canceled(
        order: Order,
        ts_event: int = 0,
    ) -> OrderCanceled:
        return OrderCanceled(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=TestIdStubs.account_id(),
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_expired(
        order: Order,
        ts_event: int = 0,
    ) -> OrderExpired:
        return OrderExpired(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=TestIdStubs.account_id(),
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def order_triggered(
        order: Order,
        ts_event: int = 0,
    ) -> OrderTriggered:
        return OrderTriggered(
            trader_id=order.trader_id,
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            account_id=TestIdStubs.account_id(),
            ts_event=ts_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def position_opened(position: Position) -> PositionOpened:
        return PositionOpened.create(
            position=position,
            fill=position.last_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def position_changed(position: Position) -> PositionChanged:
        return PositionChanged.create(
            position=position,
            fill=position.last_event,
            event_id=UUID4(),
            ts_init=0,
        )

    @staticmethod
    def position_closed(position: Position) -> PositionClosed:
        return PositionClosed.create(
            position=position,
            fill=position.last_event,
            event_id=UUID4(),
            ts_init=0,
        )
