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

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.limit import LimitOrder
from tests.test_kit.stubs.component import TestComponentStubs
from tests.test_kit.stubs.events import TestEventStubs
from tests.test_kit.stubs.identities import TestIdStubs


class TestExecStubs:
    @staticmethod
    def cash_account():
        return AccountFactory.create(
            TestEventStubs.cash_account_state(account_id=TestIdStubs.account_id())
        )

    @staticmethod
    def margin_account():
        return AccountFactory.create(
            TestEventStubs.margin_account_state(account_id=TestIdStubs.account_id())
        )

    @staticmethod
    def betting_account(account_id=None):
        return AccountFactory.create(
            TestEventStubs.betting_account_state(account_id=account_id or TestIdStubs.account_id())
        )

    @staticmethod
    def limit_order(
        instrument_id=None, side=None, price=None, quantity=None, time_in_force=None
    ) -> LimitOrder:
        strategy = TestComponentStubs.trading_strategy()
        order = strategy.order_factory.limit(
            instrument_id or TestIdStubs.audusd_id(),
            side or OrderSide.BUY,
            quantity or Quantity.from_int(10),
            price or Price.from_str("0.50"),
            time_in_force=time_in_force or TimeInForce.GTC,
        )
        return order
