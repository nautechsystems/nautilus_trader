# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Optional

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestExecStubs:
    @staticmethod
    def cash_account(account_id: Optional[AccountId] = None):
        return AccountFactory.create(
            TestEventStubs.cash_account_state(account_id=account_id or TestIdStubs.account_id()),
        )

    @staticmethod
    def margin_account(account_id: Optional[AccountId] = None):
        return AccountFactory.create(
            TestEventStubs.margin_account_state(account_id=account_id or TestIdStubs.account_id()),
        )

    @staticmethod
    def betting_account(account_id=None):
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
        trader_id: Optional[TradeId] = None,
        strategy_id: Optional[StrategyId] = None,
        client_order_id: Optional[ClientOrderId] = None,
        expire_time=None,
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
            contingency_type=ContingencyType.NONE,
            order_list_id=None,
            linked_order_ids=None,
            parent_order_id=None,
            tags=None,
        )

    @staticmethod
    def market_order(
        instrument_id=None,
        order_side=None,
        quantity=None,
        trader_id: Optional[TradeId] = None,
        strategy_id: Optional[StrategyId] = None,
        client_order_id: Optional[ClientOrderId] = None,
        time_in_force=None,
    ) -> LimitOrder:
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
            contingency_type=ContingencyType.NONE,
            order_list_id=None,
            linked_order_ids=None,
            parent_order_id=None,
            tags=None,
        )

    @staticmethod
    def make_submitted_order(
        order: Optional[Order] = None,
        instrument_id=None,
        **order_kwargs,
    ):
        order = order or TestExecStubs.limit_order(instrument_id=instrument_id, **order_kwargs)
        submitted = TestEventStubs.order_submitted(order=order)
        order.apply(submitted)
        return order

    @staticmethod
    def make_accepted_order(
        order: Optional[Order] = None,
        instrument_id: Optional[InstrumentId] = None,
        account_id: Optional[AccountId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
        **order_kwargs,
    ) -> LimitOrder:
        order = order or TestExecStubs.limit_order(instrument_id=instrument_id, **order_kwargs)
        submitted = TestExecStubs.make_submitted_order(order)
        accepted = TestEventStubs.order_accepted(
            order=submitted,
            account_id=account_id,
            venue_order_id=venue_order_id,
        )
        order.apply(accepted)
        return order

    @staticmethod
    def make_filled_order(instrument, **kwargs) -> LimitOrder:
        order = TestExecStubs.make_accepted_order(instrument_id=instrument.id, **kwargs)
        fill = TestEventStubs.order_filled(order=order, instrument=instrument)
        order.apply(fill)
        return order
