# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from ibapi.commission_report import CommissionReport
from ibapi.contract import Contract
from ibapi.execution import Execution
from ibapi.order import Order as IBOrder
from ibapi.order_cancel import OrderCancel as IBOrderCancel
from ibapi.order_state import OrderState as IBOrderState

from nautilus_trader.adapters.interactive_brokers.client.common import AccountOrderRef
from nautilus_trader.adapters.interactive_brokers.client.common import BaseMixin
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.common.enums import LogColor


class InteractiveBrokersClientOrderMixin(BaseMixin):
    """
    Manages orders for the InteractiveBrokersClient.

    This class enables the execution and management of trades. It maintains an internal
    state that tracks the relationship between Nautilus orders and IB API orders,
    ensuring that actions such as placing, modifying, and canceling orders are correctly
    reflected in both systems.

    """

    def place_order(self, order: IBOrder) -> None:
        """
        Place an order through the EClient.

        Parameters
        ----------
        order : IBOrder
            The order object containing details such as the order ID, contract
            details, and order specifics.

        """
        self._order_id_to_order_ref[order.orderId] = AccountOrderRef(
            account_id=order.account,
            order_id=order.orderRef.rsplit(":", 1)[0],
        )
        order.orderRef = f"{order.orderRef}:{order.orderId}"
        self._eclient.placeOrder(order.orderId, order.contract, order)

    def place_order_list(self, orders: list[IBOrder]) -> None:
        """
        Place a list of orders through the EClient.

        Parameters
        ----------
        orders : list[IBOrder]
            A list of order objects to be placed.

        """
        for order in orders:
            order.orderRef = f"{order.orderRef}:{order.orderId}"
            self._eclient.placeOrder(order.orderId, order.contract, order)

    def cancel_order(self, order_id: int, order_cancel: IBOrderCancel = None) -> None:
        """
        Cancel an order through the EClient.

        Parameters
        ----------
        order_id : int
            The unique identifier for the order to be canceled.
        order_cancel : OrderCancel object, optional.
            The Order cancellation parameters when cancelling an order, when subject to CME Rule 576.

        """
        if order_cancel is None:
            order_cancel = IBOrderCancel()
        self._eclient.cancelOrder(order_id, order_cancel)

    def cancel_all_orders(self) -> None:
        """
        Request to cancel all open orders through the EClient.
        """
        self._log.warning(
            "Canceling all open orders, regardless of how they were originally placed.",
        )
        self._eclient.reqGlobalCancel()

    async def get_open_orders(self, account_id: str) -> list[IBOrder]:
        """
        Retrieve a list of open orders for a specific account. Once the request is
        completed, openOrderEnd() will be called.

        Parameters
        ----------
        account_id : str
            The account identifier for which to retrieve open orders.

        Returns
        -------
        list[IBOrder]

        """
        self._log.debug(f"Requesting open orders for {account_id}")
        name = "OpenOrders"
        if not (request := self._requests.get(name=name)):
            request = self._requests.add(
                req_id=self._next_req_id(),
                name=name,
                handle=self._eclient.reqOpenOrders,
            )
            if not request:
                return []
            request.handle()

        all_orders: list[IBOrder] | None = await self._await_request(request, 30)
        if all_orders:
            orders: list[IBOrder] = [order for order in all_orders if order.account == account_id]
        else:
            orders = []

        return orders

    def next_order_id(self) -> int:
        """
        Retrieve the next valid order ID to be used for a new order.

        Returns
        -------
        int

        """
        order_id: int = self._next_valid_order_id
        self._next_valid_order_id += 1
        self._eclient.reqIds(-1)
        return order_id

    async def process_next_valid_id(self, *, order_id: int) -> None:
        """
        Receive the next valid order id.

        Will be invoked automatically upon successful API client connection,
        or after call to EClient::reqIds
        Important: the next valid order ID is only valid at the time it is received.

        """
        self._next_valid_order_id = max(self._next_valid_order_id, order_id, 101)
        if self.accounts() and not self._is_ib_connected.is_set():
            self._log.debug("`_is_ib_connected` set by `nextValidId`.", LogColor.BLUE)
            self._is_ib_connected.set()

    async def process_open_order(
        self,
        *,
        order_id: int,
        contract: Contract,
        order: IBOrder,
        order_state: IBOrderState,
    ) -> None:
        """
        Feed in currently open orders.
        """
        order.contract = IBContract(**contract.__dict__)
        order.order_state = order_state
        order.orderRef = order.orderRef.rsplit(":", 1)[0]

        # Handle response to on-demand request
        if request := self._requests.get(name="OpenOrders"):
            request.result.append(order)
            # Validate and add reverse mapping, if not exists
            if order_ref := self._order_id_to_order_ref.get(order.orderId):
                if not (
                    order_ref.account_id == order.account and order_ref.order_id == order.orderRef
                ):
                    self._log.warning(
                        f"Discrepancy found in order, expected {order_ref}, "
                        f"was (account={order.account}, order_id={order.orderRef}",
                    )
            else:
                self._order_id_to_order_ref[order.orderId] = AccountOrderRef(
                    account_id=order.account,
                    order_id=order.orderRef,
                )
            return

        # Handle event based response
        name = f"openOrder-{order.account}"
        if handler := self._event_subscriptions.get(name, None):
            handler(
                order_ref=order.orderRef.rsplit(":", 1)[0],
                order=order,
                order_state=order_state,
            )

    async def process_open_order_end(self) -> None:
        """
        Notifies the end of the open orders' reception.
        """
        if request := self._requests.get(name="OpenOrders"):
            self._end_request(request.req_id)

    async def process_order_status(
        self,
        *,
        order_id: int,
        status: str,
        filled: Decimal,
        remaining: Decimal,
        avg_fill_price: float,
        perm_id: int,
        parent_id: int,
        last_fill_price: float,
        client_id: int,
        why_held: str,
        mkt_cap_price: float,
    ) -> None:
        """
        Get the up-to-date information of an order every time it changes.

        Note: Often there are duplicate orderStatus messages.

        """
        order_ref = self._order_id_to_order_ref.get(order_id, None)
        if order_ref:
            name = f"orderStatus-{order_ref.account_id}"
            if handler := self._event_subscriptions.get(name, None):
                handler(
                    order_ref=self._order_id_to_order_ref[order_id].order_id,
                    order_status=status,
                )

    async def process_exec_details(
        self,
        *,
        req_id: int,
        contract: Contract,
        execution: Execution,
    ) -> None:
        """
        Provide the executions that happened in the prior 24 hours.
        """
        if not (cache := self._exec_id_details.get(execution.execId, None)):
            self._exec_id_details[execution.execId] = {}
            cache = self._exec_id_details[execution.execId]
        cache["execution"] = execution
        cache["order_ref"] = execution.orderRef.rsplit(":", 1)[0]

        name = f"execDetails-{execution.acctNumber}"
        if (handler := self._event_subscriptions.get(name, None)) and cache.get(
            "commission_report",
        ):
            handler(
                order_ref=cache["order_ref"],
                execution=cache["execution"],
                commission_report=cache["commission_report"],
            )
            cache.pop(execution.execId, None)

    async def process_commission_report(
        self,
        *,
        commission_report: CommissionReport,
    ) -> None:
        """
        Provide the CommissionReport of an Execution.
        """
        if not (cache := self._exec_id_details.get(commission_report.execId, None)):
            self._exec_id_details[commission_report.execId] = {}
            cache = self._exec_id_details[commission_report.execId]
        cache["commission_report"] = commission_report

        if cache.get("execution") and (account := getattr(cache["execution"], "acctNumber", None)):
            name = f"execDetails-{account}"
            if handler := self._event_subscriptions.get(name, None):
                handler(
                    order_ref=cache["order_ref"],
                    execution=cache["execution"],
                    commission_report=cache["commission_report"],
                )
                cache.pop(commission_report.execId, None)
