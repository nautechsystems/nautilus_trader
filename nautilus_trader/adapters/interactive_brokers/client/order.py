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

import functools
from decimal import Decimal

from ibapi.commission_report import CommissionReport
from ibapi.contract import Contract
from ibapi.execution import Execution
from ibapi.execution import ExecutionFilter
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

    _fetch_all_open_orders: bool

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
            The Order cancellation parameters when canceling an order, when subject to CME Rule 576.

        """
        if order_cancel is None:
            order_cancel = IBOrderCancel()

        self._eclient.cancelOrder(order_id, order_cancel)

    def cancel_all_orders(self) -> None:
        """
        Request to cancel all open orders through the EClient.
        """
        self._log.warning(
            "Canceling all open orders, regardless of how they were originally placed",
        )
        self._eclient.reqGlobalCancel()

    async def get_open_orders(self, account_id: str) -> list[IBOrder]:
        """
        Retrieve a list of open orders for a specific account. Once the request is
        completed, openOrderEnd() will be called.

        The behavior depends on the `fetch_all_open_orders` configuration:
        - If True: Uses reqAllOpenOrders() to fetch orders from all API clients,
          TWS/IB Gateway GUI, and other trading interfaces
        - If False: Uses reqOpenOrders() to fetch only orders from the current
          client ID session

        Parameters
        ----------
        account_id : str
            The account identifier for which to retrieve open orders.

        Returns
        -------
        list[IBOrder]
            List of open orders filtered by the specified account_id.

        """
        self._log.debug(f"Requesting open orders for {account_id}")
        name = "OpenOrders"

        if not (request := self._requests.get(name=name)):
            # Choose the appropriate handler based on configuration
            if self._fetch_all_open_orders:
                handle = self._eclient.reqAllOpenOrders
            else:
                handle = self._eclient.reqOpenOrders

            request = self._requests.add(
                req_id=self._next_req_id(),
                name=name,
                handle=handle,
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

    async def get_executions(
        self,
        account_id: str,
        execution_filter: ExecutionFilter | None = None,
    ) -> list[dict]:
        """
        Retrieve execution reports for a specific account.

        Parameters
        ----------
        account_id : str
            The account identifier for which to retrieve executions.
        execution_filter : ExecutionFilter, optional
            Filter criteria for executions. If None, a default filter for the account will be used.

        Returns
        -------
        list[dict]
            List of execution details with associated contracts and commission reports.
            Each dict contains 'execution', 'contract', and 'commission_report' keys.

        """
        self._log.debug(f"Requesting executions for {account_id}")
        name = f"Executions-{account_id}"

        if not (request := self._requests.get(name=name)):
            # Create execution filter if not provided
            if execution_filter is None:
                execution_filter = ExecutionFilter()
                execution_filter.acctCode = account_id

            req_id = self._next_req_id()
            request = self._requests.add(
                req_id=req_id,
                name=name,
                handle=functools.partial(
                    self._eclient.reqExecutions,
                    reqId=req_id,
                    execFilter=execution_filter,
                ),
                cancel=lambda: None,  # No cancel method for executions
            )

            if not request:
                return []

            request.handle()

        # Wait for execution details to be collected
        execution_details: list[dict] | None = await self._await_request(request, 30)

        if execution_details:
            # Filter by account if needed (in case filter didn't work perfectly)
            filtered_executions = [
                exec_detail
                for exec_detail in execution_details
                if exec_detail.get("execution")
                and exec_detail["execution"].acctNumber == account_id
            ]
        else:
            filtered_executions = []

        return filtered_executions

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
            self._log.debug("`_is_ib_connected` set by `nextValidId`", LogColor.BLUE)
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
                    avg_fill_price=avg_fill_price,
                    filled=filled,
                    remaining=remaining,
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
        cache["contract"] = IBContract(**contract.__dict__)
        cache["order_ref"] = execution.orderRef.rsplit(":", 1)[0]
        cache["req_id"] = req_id

        # Check if this is for a get_executions request
        execution_request_name = f"Executions-{execution.acctNumber}"

        if request := self._requests.get(name=execution_request_name):
            if request.req_id == req_id and cache.get("commission_report"):
                # Add complete execution detail to request result
                execution_detail = {
                    "execution": cache["execution"],
                    "contract": cache["contract"],
                    "commission_report": cache["commission_report"],
                }
                request.result.append(execution_detail)
                # Don't remove from cache yet, wait for execDetailsEnd

        # Handle event-based response for live executions
        name = f"execDetails-{execution.acctNumber}"
        if (handler := self._event_subscriptions.get(name, None)) and cache.get(
            "commission_report",
        ):
            handler(
                order_ref=cache["order_ref"],
                execution=cache["execution"],
                commission_report=cache["commission_report"],
                contract=cache["contract"],
            )

            # Only remove from cache if not part of a request
            if not self._requests.get(name=execution_request_name):
                self._exec_id_details.pop(execution.execId, None)

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
            # Check if this is for a get_executions request
            execution_request_name = f"Executions-{account}"
            if request := self._requests.get(name=execution_request_name):
                req_id = cache.get("req_id")
                if req_id == request.req_id:
                    # Add complete execution detail to request result
                    execution_detail = {
                        "execution": cache["execution"],
                        "contract": cache["contract"],
                        "commission_report": cache["commission_report"],
                    }
                    request.result.append(execution_detail)
                    # Don't remove from cache yet, wait for execDetailsEnd

            # Handle event-based response for live executions
            name = f"execDetails-{account}"
            if handler := self._event_subscriptions.get(name, None):
                handler(
                    order_ref=cache["order_ref"],
                    execution=cache["execution"],
                    commission_report=cache["commission_report"],
                    contract=cache.get("contract"),
                )

                # Only remove from cache if not part of a request
                if not self._requests.get(name=execution_request_name):
                    self._exec_id_details.pop(commission_report.execId, None)

    async def process_exec_details_end(self, req_id: int) -> None:
        """
        Process when all executions have been sent for a request.
        """
        # End the request if it exists
        if self._requests.get(req_id=req_id):
            self._end_request(req_id)
