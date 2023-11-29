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

# fmt: off
from ibapi.commission_report import CommissionReport
from ibapi.execution import Execution
from ibapi.order import Order as IBOrder
from ibapi.order_state import OrderState as IBOrderState
from ibapi.utils import current_fn_name

from nautilus_trader.adapters.interactive_brokers.client.common import AccountOrderRef
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.common.enums import LogColor


class InteractiveBrokersOrderManager:
    """
    For the InteractiveBrokersClient.
    """

    def __init__(self, client):
        self._client = client
        self._eclient = client._eclient
        self._log = client._log
        self.order_id_to_order_ref: dict[int, AccountOrderRef] = {}
        self.next_valid_order_id: int = -1
        # Temporary cache
        self._exec_id_details: dict[
            str,
            dict[str, Execution | (CommissionReport | str)],
        ] = {}

    def place_order(self, order: IBOrder) -> None:
        """
        Place an order through the EClient.

        Parameters
        ----------
        order : IBOrder
            The order object containing details such as the order ID, contract
            details, and order specifics.

        Returns
        -------
        None

        """
        self.order_id_to_order_ref[order.orderId] = AccountOrderRef(
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

        Returns
        -------
        None

        """
        for order in orders:
            order.orderRef = f"{order.orderRef}:{order.orderId}"
            self._eclient.placeOrder(order.orderId, order.contract, order)

    def cancel_order(self, order_id: int, manual_cancel_order_time: str = "") -> None:
        """
        Cancel an order through the EClient.

        Parameters
        ----------
        order_id : int
            The unique identifier for the order to be canceled.
        manual_cancel_order_time : str, optional
            The timestamp indicating when the order was canceled manually.

        Returns
        -------
        None

        """
        self._eclient.cancelOrder(order_id, manual_cancel_order_time)

    def cancel_all_orders(self) -> None:
        """
        Request to cancel all open orders through the EClient.

        Returns
        -------
        None

        """
        self._log.warning(
            "Canceling all open orders, regardless of how they were originally placed.",
        )
        self._eclient.reqGlobalCancel()

    async def get_open_orders(self, account_id: str) -> list[IBOrder]:
        """
        Retrieve a list of open orders for a specific account.

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
        if not (request := self._client.requests.get(name=name)):
            request = self._client.requests.add(
                req_id=self._client.next_req_id(),
                name=name,
                handle=self._eclient.reqOpenOrders,
            )
            request.handle()
            all_orders: list[IBOrder] = await self._client.await_request(request, 30)
        else:
            all_orders = await self._client.await_request(request, 30)
        orders = []
        for order in all_orders:
            if order.account_id == account_id:
                orders.append(order)
        return orders

    def next_order_id(self):
        """
        Retrieve the next valid order ID to be used for a new order.

        Returns
        -------
        int

        """
        order_id: int = self.next_valid_order_id
        self.next_valid_order_id += 1
        self._eclient.reqIds(-1)
        return order_id

    # -- EWrapper overrides -----------------------------------------------------------------------
    def nextValidId(self, order_id: int) -> None:
        """
        Receive the next valid order id.

        Will be invoked automatically upon successful API client connection,
        or after call to EClient::reqIds
        Important: the next valid order ID is only valid at the time it is received.

        """
        self._client.logAnswer(current_fn_name(), vars())
        self.next_valid_order_id = max(self.next_valid_order_id, order_id, 101)
        if self._client.account_manager.accounts() and not self._client.is_ib_ready.is_set():
            self._log.info("`is_ib_ready` set by nextValidId", LogColor.BLUE)
            self._client.is_ib_ready.set()

    def openOrder(
        self,
        order_id: int,
        contract: IBContract,
        order: IBOrder,
        order_state: IBOrderState,
    ) -> None:
        """
        Feed in currently open orders.
        """
        self._client.logAnswer(current_fn_name(), vars())
        # Handle response to on-demand request
        if request := self._client.requests.get(name="OpenOrders"):
            order.contract = IBContract(**contract.__dict__)
            order.order_state = order_state
            order.orderRef = order.orderRef.rsplit(":", 1)[0]
            request.result.append(order)
            # Validate and add reverse mapping, if not exists
            if order_ref := self.order_id_to_order_ref.get(order.orderId):
                if not (
                    order_ref.account_id == order.account_id and order_ref.order_id == order.orderRef
                ):
                    self._log.warning(
                        f"Discrepancy found in order, expected {order_ref}, "
                        f"was (account={order.account}, order_id={order.orderRef}",
                    )
            else:
                self.order_id_to_order_ref[order.orderId] = AccountOrderRef(
                    account_id=order.account_id,
                    order_id=order.orderRef,
                )
            return

        # Handle event based response
        name = f"openOrder-{order.account}"
        if handler := self._client.event_subscriptions.get(name, None):
            handler(
                order_ref=order.orderRef.rsplit(":", 1)[0],
                order=order,
                order_state=order_state,
            )

    def openOrderEnd(self) -> None:
        """
        Notifies the end of the open orders' reception.
        """
        self._client.logAnswer(current_fn_name(), vars())
        if request := self._client.requests.get(name="OpenOrders"):
            self._client.end_request(request.req_id)

    def orderStatus(
        self,
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
        self._client.logAnswer(current_fn_name(), vars())
        order_ref = self.order_id_to_order_ref.get(order_id, None)
        if order_ref:
            name = f"orderStatus-{order_ref.account_id}"
            if handler := self._client.event_subscriptions.get(name, None):
                handler(
                    order_ref=self.order_id_to_order_ref[order_id].order_id,
                    order_status=status,
                )

    def execDetails(
        self,
        req_id: int,
        contract: IBContract,
        execution: Execution,
    ) -> None:
        """
        Provide the executions that happened in the prior 24 hours.
        """
        self._client.logAnswer(current_fn_name(), vars())
        if not (cache := self._exec_id_details.get(execution.execId, None)):
            self._exec_id_details[execution.execId] = {}
            cache = self._exec_id_details[execution.execId]
        cache["execution"] = execution
        cache["order_ref"] = execution.orderRef.rsplit(":", 1)[0]

        name = f"execDetails-{execution.acctNumber}"
        if (handler := self._client.event_subscriptions.get(name, None)) and cache.get(
            "commission_report",
        ):
            handler(
                order_ref=cache["order_ref"],
                execution=cache["execution"],
                commission_report=cache["commission_report"],
            )
            cache.pop(execution.execId, None)

    def commissionReport(
        self,
        commission_report: CommissionReport,
    ) -> None:
        """
        Provide the CommissionReport of an Execution.
        """
        self._client.logAnswer(current_fn_name(), vars())
        if not (cache := self._exec_id_details.get(commission_report.execId, None)):
            self._exec_id_details[commission_report.execId] = {}
            cache = self._exec_id_details[commission_report.execId]
        cache["commission_report"] = commission_report

        if cache.get("execution") and (account := getattr(cache["execution"], "acctNumber", None)):
            name = f"execDetails-{account}"
            if handler := self._client.event_subscriptions.get(name, None):
                handler(
                    order_ref=cache["order_ref"],
                    execution=cache["execution"],
                    commission_report=cache["commission_report"],
                )
                cache.pop(commission_report.execId, None)
