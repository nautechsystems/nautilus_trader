# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import ContingencyType
from nautilus_trader.core.nautilus_pyo3 import Currency
from nautilus_trader.core.nautilus_pyo3 import LiquiditySide
from nautilus_trader.core.nautilus_pyo3 import Money
from nautilus_trader.core.nautilus_pyo3 import OrderAccepted
from nautilus_trader.core.nautilus_pyo3 import OrderCanceled
from nautilus_trader.core.nautilus_pyo3 import OrderCancelRejected
from nautilus_trader.core.nautilus_pyo3 import OrderDenied
from nautilus_trader.core.nautilus_pyo3 import OrderEmulated
from nautilus_trader.core.nautilus_pyo3 import OrderExpired
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderInitialized
from nautilus_trader.core.nautilus_pyo3 import OrderListId
from nautilus_trader.core.nautilus_pyo3 import OrderModifyRejected
from nautilus_trader.core.nautilus_pyo3 import OrderPendingCancel
from nautilus_trader.core.nautilus_pyo3 import OrderPendingUpdate
from nautilus_trader.core.nautilus_pyo3 import OrderRejected
from nautilus_trader.core.nautilus_pyo3 import OrderReleased
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import OrderSubmitted
from nautilus_trader.core.nautilus_pyo3 import OrderTriggered
from nautilus_trader.core.nautilus_pyo3 import OrderType
from nautilus_trader.core.nautilus_pyo3 import OrderUpdated
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TradeId
from nautilus_trader.core.nautilus_pyo3 import TriggerType
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


uuid = "91762096-b188-49ea-8562-8d8a4cc22ff2"


class TestEventsProviderPyo3:
    @staticmethod
    def order_denied_max_submit_rate() -> OrderDenied:
        return OrderDenied(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.audusd_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            reason="Exceeded MAX_ORDER_SUBMIT_RATE",
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_rejected_insufficient_margin() -> OrderRejected:
        return OrderRejected(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.audusd_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            reason="INSUFFICIENT_MARGIN",
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_filled_buy_limit() -> OrderFilled:
        return OrderFilled(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            trade_id=TradeId("1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            last_qty=Quantity.from_str("0.561000"),
            last_px=Price.from_str("15600.12445"),
            currency=Currency.from_str("USDT"),
            liquidity_side=LiquiditySide.MAKER,
            position_id=PositionId("2"),
            commission=Money.from_str("12.2 USDT"),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_initialized() -> OrderInitialized:
        return OrderInitialized(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            order_side=OrderSide.BUY,
            order_type=OrderType.LIMIT,
            quantity=Quantity.from_str("0.561000"),
            time_in_force=TimeInForce.DAY,
            post_only=True,
            reduce_only=True,
            quote_quantity=False,
            reconciliation=False,
            event_id=UUID4(uuid),
            emulation_trigger=TriggerType.BID_ASK,
            trigger_instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            price=Price.from_str("1520.10"),
            contingency_type=ContingencyType.OTO,
            linked_order_ids=[ClientOrderId("O-2020872378424")],
            order_list_id=OrderListId("1"),
            parent_order_id=None,
            exec_algorithm_id=None,
            exec_algorithm_params=None,
            exec_spawn_id=None,
            tags="ENTRY",
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_triggered() -> OrderTriggered:
        return OrderTriggered(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            reconciliation=False,
        )

    @staticmethod
    def order_submitted() -> OrderSubmitted:
        return OrderSubmitted(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_emulated() -> OrderEmulated:
        return OrderEmulated(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_released() -> OrderReleased:
        return OrderReleased(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            released_price=Price.from_str("22000.0"),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
        )

    @staticmethod
    def order_updated() -> OrderUpdated:
        return OrderUpdated(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            quantity=Quantity.from_str("1.5"),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            price=Price.from_str("1500.0"),
            trigger_price=None,
        )

    @staticmethod
    def order_pending_update() -> OrderPendingUpdate:
        return OrderPendingUpdate(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
        )

    @staticmethod
    def order_pending_cancel() -> OrderPendingCancel:
        return OrderPendingCancel(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
        )

    @staticmethod
    def order_modified_rejected():
        return OrderModifyRejected(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            event_id=UUID4(uuid),
            reason="ORDER_DOES_NOT_EXIST",
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_accepted() -> OrderAccepted:
        return OrderAccepted(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_cancel_rejected() -> OrderCancelRejected:
        return OrderCancelRejected(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            reason="ORDER_DOES_NOT_EXIST",
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_canceled() -> OrderCanceled:
        return OrderCanceled(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )

    @staticmethod
    def order_expired() -> OrderExpired:
        return OrderExpired(
            trader_id=TestIdProviderPyo3.trader_id(),
            strategy_id=TestIdProviderPyo3.strategy_id(),
            instrument_id=TestIdProviderPyo3.ethusdt_binance_id(),
            client_order_id=TestIdProviderPyo3.client_order_id(),
            account_id=TestIdProviderPyo3.account_id(),
            venue_order_id=TestIdProviderPyo3.venue_order_id(),
            event_id=UUID4(uuid),
            ts_init=0,
            ts_event=0,
            reconciliation=False,
        )
