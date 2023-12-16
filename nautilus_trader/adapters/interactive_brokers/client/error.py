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

from inspect import iscoroutinefunction
from typing import TYPE_CHECKING


if TYPE_CHECKING:
    from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient


class InteractiveBrokersErrorHandler:
    """
    Handles errors and warnings for the InteractiveBrokersClient.

    This class is designed to process and log various types of error messages and
    warnings encountered during the operation of the InteractiveBrokersClient. It
    categorizes different error codes and manages appropriate responses, including
    logging and state updates.

    https://ibkrcampus.com/ibkr-api-page/tws-api-error-codes/#understanding-error-codes

    """

    WARNING_CODES = {1101, 1102, 110, 165, 202, 399, 404, 434, 492, 10167}
    CLIENT_ERRORS = {502, 503, 504, 10038, 10182, 1100, 2110}
    CONNECTIVITY_LOST_CODES = {1100, 1300, 2110}
    CONNECTIVITY_RESTORED_CODES = {1101, 1102}
    ORDER_REJECTION_CODES = {201, 203, 321, 10289, 10293}

    def __init__(self, client: "InteractiveBrokersClient"):
        self._client = client
        self._eclient = client._eclient
        self._log = client._log

        self._eclient.error = self.error

    def _log_message(
        self,
        error_code: int,
        req_id: int,
        error_string: str,
        is_warning: bool,
    ) -> None:
        """
        Log the provided error or warning message.

        Parameters
        ----------
        error_code : int
            The error code associated with the message.
        req_id : int
            The request ID associated with the error or warning.
        error_string : str
            The error or warning message string.
        is_warning : bool
            Indicates whether the message is a warning or an error.

        Returns
        -------
        None

        """
        msg_type = "Warning" if is_warning else "Error"
        msg = f"{msg_type} {error_code} {req_id=}: {error_string}"
        if is_warning:
            self._log.info(msg)
        else:
            self._log.error(msg)

    def _process_error(self, req_id: int, error_code: int, error_string: str) -> None:
        """
        Process an error based on its code, request ID, and message. Depending on the
        error code, this method delegates to specific error handlers or performs general
        error handling.

        Parameters
        ----------
        req_id : int
            The request ID associated with the error.
        error_code : int
            The error code.
        error_string : str
            The error message string.

        Returns
        -------
        None

        """
        is_warning = error_code in self.WARNING_CODES or 2100 <= error_code < 2200
        self._log_message(error_code, req_id, error_string, is_warning)

        if req_id != -1:
            if self._client.subscriptions.get(req_id=req_id):
                self._handle_subscription_error(req_id, error_code, error_string)
            elif self._client.requests.get(req_id=req_id):
                self._handle_request_error(req_id, error_code, error_string)
            elif req_id in self._client.order_manager.order_id_to_order_ref:
                self._handle_order_error(req_id, error_code, error_string)
            else:
                self._log.warning(f"Unhandled error: {error_code} for req_id {req_id}")
        elif error_code in self.CLIENT_ERRORS or error_code in self.CONNECTIVITY_LOST_CODES:
            self._log.warning(f"Client or Connectivity Lost Error: {error_string}")
            if self._client.is_ib_ready.is_set():
                self._client.is_ib_ready.clear()
        elif error_code in self.CONNECTIVITY_RESTORED_CODES:
            if not self._client.is_ib_ready.is_set():
                self._client.is_ib_ready.set()

    def _handle_subscription_error(self, req_id: int, error_code: int, error_string: str) -> None:
        """
        Handle errors specific to data subscriptions. Processes subscription-related
        errors and takes appropriate actions, such as cancelling the subscription or
        clearing flags.

        Parameters
        ----------
        req_id : int
            The request ID associated with the subscription error.
        error_code : int
            The error code.
        error_string : str
            The error message string.

        Returns
        -------
        None

        """
        subscription = self._client.subscriptions.get(req_id=req_id)
        if not subscription:
            return
        if error_code in [10189, 366, 102]:
            # Handle specific subscription-related error codes
            self._log.warning(f"{error_code}: {error_string}")
            subscription.cancel()
            if iscoroutinefunction(subscription.handle):
                self._client.create_task(subscription.handle())
            else:
                subscription.handle()
        elif error_code == 10182:
            # Handle disconnection error
            self._log.warning(f"{error_code}: {error_string}")
            if self._client.is_ib_ready.is_set():
                self._log.info(f"`is_ib_ready` cleared by {subscription.name}")
                self._client.is_ib_ready.clear()
        else:
            # Log unknown subscription errors
            self._log.warning(
                f"Unknown subscription error: {error_code} for req_id {req_id}",
            )

    def _handle_request_error(self, req_id: int, error_code: int, error_string: str) -> None:
        """
        Handle errors related to general requests. Logs the error and ends the request
        associated with the given request ID.

        Parameters
        ----------
        req_id : int
            The request ID associated with the error.
        error_code : int
            The error code.
        error_string : str
            The error message string.

        Returns
        -------
        None

        """
        request = self._client.requests.get(req_id=req_id)
        self._log.warning(f"{error_code}: {error_string}, {request}")
        self._client.end_request(req_id, success=False)

    def _handle_order_error(self, req_id: int, error_code: int, error_string: str) -> None:
        """
        Handle errors related to orders. Manages various order-related errors, including
        rejections and cancellations, and logs or forwards them as appropriate.

        Parameters
        ----------
        req_id : int
            The request ID associated with the order error.
        error_code : int
            The error code.
        error_string : str
            The error message string.

        Returns
        -------
        None

        """
        order_ref = self._client.order_manager.order_id_to_order_ref.get(req_id, None)
        if not order_ref:
            self._log.warning(f"Order reference not found for req_id {req_id}")
            return

        name = f"orderStatus-{order_ref.account_id}"
        handler = self._client.event_subscriptions.get(name, None)

        if error_code in self.ORDER_REJECTION_CODES:
            # Handle various order rejections
            if handler:
                handler(order_ref=order_ref.order_id, order_status="Rejected", reason=error_string)
        elif error_code == 202:
            # Handle order cancellation warning
            if handler:
                handler(order_ref=order_ref.order_id, order_status="Cancelled", reason=error_string)
        else:
            # Log unknown order warnings / errors
            self._log.warning(
                f"Unhandled order warning or error code {error_code} for req_id {req_id} - "
                f"{error_string}",
            )

    # -- EWrapper overrides -----------------------------------------------------------------------
    def error(
        self,
        req_id: int,
        error_code: int,
        error_string: str,
        advanced_order_reject_json: str = "",
    ) -> None:
        """
        Errors sent by TWS API are received here.
        """
        self._process_error(req_id, error_code, error_string)
