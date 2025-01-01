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

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.common.config import NautilusConfig

from nautilus_trader.backtest.exchange cimport SimulatedExchange
from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.common.component cimport TestClock
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.execution.client cimport ExecutionClient
from nautilus_trader.execution.messages cimport BatchCancelOrders
from nautilus_trader.execution.messages cimport CancelAllOrders
from nautilus_trader.execution.messages cimport CancelOrder
from nautilus_trader.execution.messages cimport ModifyOrder
from nautilus_trader.execution.messages cimport SubmitOrder
from nautilus_trader.execution.messages cimport SubmitOrderList
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.orders.base cimport Order


cdef class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the `BacktestEngine`.

    Parameters
    ----------
    exchange : SimulatedExchange
        The simulated exchange for the backtest.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : TestClock
        The clock for the client.
    routing : bool
        If multi-venue routing is enabled for the client.
    frozen_account : bool
        If the backtest run account is frozen.
    """

    def __init__(
        self,
        SimulatedExchange exchange not None,
        MessageBus msgbus not None,
        Cache cache not None,
        TestClock clock not None,
        bint routing=False,
        bint frozen_account=False,
    ) -> None:
        super().__init__(
            client_id=ClientId(exchange.id.value),
            venue=Venue(exchange.id.value),
            oms_type=exchange.oms_type,
            account_type=exchange.account_type,
            base_currency=exchange.base_currency,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._set_account_id(AccountId(f"{exchange.id.value}-001"))
        if not frozen_account:
            AccountFactory.register_calculated_account(exchange.id.value)

        self._exchange = exchange
        self.is_connected = False

    cpdef void _start(self):
        self._log.info(f"Connecting...")
        self.is_connected = True
        self._log.info(f"Connected")

    cpdef void _stop(self):
        self._log.info(f"Disconnecting...")
        self.is_connected = False
        self._log.info(f"Disconnected")

# -- COMMAND HANDLERS -----------------------------------------------------------------------------

    cpdef void submit_order(self, SubmitOrder command):
        Condition.is_true(self.is_connected, "not connected")

        self.generate_order_submitted(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        self._exchange.send(command)

    cpdef void submit_order_list(self, SubmitOrderList command):
        Condition.is_true(self.is_connected, "not connected")

        cdef Order order
        for order in command.order_list.orders:
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

        self._exchange.send(command)

    cpdef void modify_order(self, ModifyOrder command):
        Condition.is_true(self.is_connected, "not connected")

        self._exchange.send(command)

    cpdef void cancel_order(self, CancelOrder command):
        Condition.is_true(self.is_connected, "not connected")

        self._exchange.send(command)

    cpdef void cancel_all_orders(self, CancelAllOrders command):
        Condition.is_true(self.is_connected, "not connected")

        self._exchange.send(command)

    cpdef void batch_cancel_orders(self, BatchCancelOrders command):
        Condition.is_true(self.is_connected, "not connected")

        self._exchange.send(command)
