# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.accounting.accounts.betting import BettingAccount
from nautilus_trader.accounting.accounts.cash import CashAccount
from nautilus_trader.accounting.accounts.margin import MarginAccount
from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import OrderList
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestExecStubs:
    @staticmethod
    def cash_account(account_id: AccountId | None = None) -> CashAccount:
        return AccountFactory.create(
            TestEventStubs.cash_account_state(account_id=account_id or TestIdStubs.account_id()),
        )

    @staticmethod
    def margin_account(account_id: AccountId | None = None) -> MarginAccount:
        return AccountFactory.create(
            TestEventStubs.margin_account_state(account_id=account_id or TestIdStubs.account_id()),
        )

    @staticmethod
    def betting_account(account_id: AccountId | None = None) -> BettingAccount:
        return AccountFactory.create(
            TestEventStubs.betting_account_state(account_id=account_id or TestIdStubs.account_id()),
        )

    @staticmethod
    def limit_order(
        instrument_id=None,
        order_side=None,
        price=None,
        quantity=None,
        time_in_force=None,
        trader_id: TradeId | None = None,
        strategy_id: StrategyId | None = None,
        client_order_id: ClientOrderId | None = None,
        expire_time=None,
        tags=None,
    ) -> LimitOrder:
        return LimitOrder(
            trader_id=trader_id or TestIdStubs.trader_id(),
            strategy_id=strategy_id or TestIdStubs.strategy_id(),
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            client_order_id=client_order_id or TestIdStubs.client_order_id(),
            order_side=order_side or OrderSide.BUY,
            quantity=quantity or Quantity.from_str("100"),
            price=price or Price.from_str("55.0"),
            time_in_force=time_in_force or TimeInForce.GTC,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
            post_only=False,
            reduce_only=False,
            display_qty=None,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
            linked_order_ids=None,
            parent_order_id=None,
            tags=tags,
        )

    @staticmethod
    def limit_with_stop_market(
        instrument_id=None,
        order_side=None,
        price=None,
        quantity=None,
        time_in_force=None,
        trader_id: TradeId | None = None,
        strategy_id: StrategyId | None = None,
        order_list_id: OrderListId | None = None,
        entry_client_order_id: ClientOrderId | None = None,
        sl_client_order_id: ClientOrderId | None = None,
        sl_trigger_price=None,
        expire_time=None,
        tags=None,
    ):
        entry_order = LimitOrder(
            trader_id=trader_id or TestIdStubs.trader_id(),
            strategy_id=strategy_id or TestIdStubs.strategy_id(),
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            client_order_id=entry_client_order_id or TestIdStubs.client_order_id(1),
            order_side=order_side or OrderSide.BUY,
            quantity=quantity or Quantity.from_str("100"),
            price=price or Price.from_str("55.0"),
            time_in_force=time_in_force or TimeInForce.GTC,
            expire_time_ns=0 if expire_time is None else dt_to_unix_nanos(expire_time),
            init_id=TestIdStubs.uuid(),
            ts_init=0,
            post_only=False,
            reduce_only=False,
            display_qty=None,
            contingency_type=ContingencyType.OTO,
            order_list_id=order_list_id or TestIdStubs.order_list_id(),
            linked_order_ids=[sl_client_order_id or TestIdStubs.client_order_id(2)],
            parent_order_id=None,
            tags=tags,
        )
        sl_order = StopMarketOrder(
            trader_id=trader_id or TestIdStubs.trader_id(),
            strategy_id=strategy_id or TestIdStubs.strategy_id(),
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            client_order_id=sl_client_order_id or TestIdStubs.client_order_id(2),
            order_side=Order.opposite_side(entry_order.side),
            quantity=entry_order.quantity,
            trigger_price=sl_trigger_price or Price.from_str("50.0"),
            trigger_type=TriggerType.MID_POINT,
            init_id=UUID4(),
            ts_init=0,
            time_in_force=TimeInForce.GTC,
            order_list_id=order_list_id or TestIdStubs.order_list_id(),
            parent_order_id=entry_order.client_order_id,
            tags=None,
        )
        return OrderList(order_list_id or TestIdStubs.order_list_id(), [entry_order, sl_order])

    @staticmethod
    def market_order(
        instrument_id=None,
        order_side=None,
        quantity=None,
        trader_id: TradeId | None = None,
        strategy_id: StrategyId | None = None,
        client_order_id: ClientOrderId | None = None,
        time_in_force=None,
    ) -> MarketOrder:
        return MarketOrder(
            trader_id=trader_id or TestIdStubs.trader_id(),
            strategy_id=strategy_id or TestIdStubs.strategy_id(),
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            client_order_id=client_order_id or TestIdStubs.client_order_id(),
            order_side=order_side or OrderSide.BUY,
            quantity=quantity or Quantity.from_str("100"),
            time_in_force=time_in_force or TimeInForce.GTC,
            init_id=TestIdStubs.uuid(),
            ts_init=0,
            reduce_only=False,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_list_id=None,
            linked_order_ids=None,
            parent_order_id=None,
            tags=None,
        )

    @staticmethod
    def make_submitted_order(
        order: Order | None = None,
        instrument_id=None,
        **order_kwargs,
    ) -> Order:
        order = order or TestExecStubs.limit_order(instrument_id=instrument_id, **order_kwargs)
        submitted = TestEventStubs.order_submitted(order=order)
        assert order
        order.apply(submitted)
        return order

    @staticmethod
    def make_accepted_order(
        order: Order | None = None,
        instrument_id: InstrumentId | None = None,
        account_id: AccountId | None = None,
        venue_order_id: VenueOrderId | None = None,
        **order_kwargs,
    ) -> Order:
        order = order or TestExecStubs.limit_order(instrument_id=instrument_id, **order_kwargs)
        submitted = TestExecStubs.make_submitted_order(order)
        accepted = TestEventStubs.order_accepted(
            order=submitted,
            account_id=account_id,
            venue_order_id=venue_order_id,
        )
        assert order
        order.apply(accepted)
        return order

    @staticmethod
    def make_filled_order(instrument: Instrument, **kwargs) -> Order:
        order = TestExecStubs.make_accepted_order(instrument_id=instrument.id, **kwargs)
        fill = TestEventStubs.order_filled(order=order, instrument=instrument)
        order.apply(fill)
        return order
