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

from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import OrderList
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestCommandStubs:
    @staticmethod
    def submit_order_command(order: Order) -> SubmitOrder:
        return SubmitOrder(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            position_id=TestIdStubs.position_id(),
            order=order,
            command_id=TestIdStubs.uuid(),
            ts_init=0,
        )

    @staticmethod
    def submit_order_list_command(order_list: OrderList) -> SubmitOrderList:
        return SubmitOrderList(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            order_list=order_list,
            position_id=TestIdStubs.position_id(),
            command_id=TestIdStubs.uuid(),
            ts_init=0,
        )

    @staticmethod
    def modify_order_command(
        price: Price | None = None,
        quantity: Quantity | None = None,
        instrument_id: InstrumentId | None = None,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
        order: Order | None = None,
    ) -> ModifyOrder:
        assert price or quantity
        if order is not None:
            return ModifyOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                quantity=quantity,
                price=price,
                trigger_price=None,
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            )
        else:
            return ModifyOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                quantity=quantity,
                price=price,
                trigger_price=None,
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            )

    @staticmethod
    def cancel_order_command(
        instrument_id: InstrumentId | None = None,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
        order: Order | None = None,
    ) -> CancelOrder:
        if order is not None:
            return CancelOrder(
                trader_id=order.trader_id,
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            )
        else:
            return CancelOrder(
                trader_id=TestIdStubs.trader_id(),
                strategy_id=TestIdStubs.strategy_id(),
                instrument_id=instrument_id or TestIdStubs.audusd_id(),
                client_order_id=client_order_id or TestIdStubs.client_order_id(),
                venue_order_id=venue_order_id or TestIdStubs.venue_order_id(),
                command_id=TestIdStubs.uuid(),
                ts_init=0,
            )
