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
from typing import Final

from nautilus_trader.adapters.interactive_brokers.client.common import BaseMixin
from nautilus_trader.common.enums import LogColor


class InteractiveBrokersClientErrorMixin(BaseMixin):
    """
    Handles errors and warnings for the InteractiveBrokersClient.

    This class is designed to process and log various types of error messages and
    warnings encountered during the operation of the InteractiveBrokersClient. It
    categorizes different error codes and manages appropriate responses, including
    logging and state updates.

    https://ibkrcampus.com/ibkr-api-page/tws-api-error-codes/#understanding-error-codes

    """

    WARNING_CODES: Final[set[int]] = {1101, 1102, 110, 165, 202, 399, 404, 434, 492, 10167}
    CLIENT_ERRORS: Final[set[int]] = {502, 503, 504, 10038, 10182, 1100, 2110}
    CONNECTIVITY_LOST_CODES: Final[set[int]] = {1100, 1300, 2110}
    CONNECTIVITY_RESTORED_CODES: Final[set[int]] = {1101, 1102}
    ORDER_REJECTION_CODES: Final[set[int]] = {201, 203, 321, 10289, 10293}
    SUPPRESS_ERROR_LOGGING_CODES: Final[set[int]] = {200}

    async def _log_message(
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

        """
        msg = f"{error_string} (code: {error_code}, {req_id=})."
        if error_code in self.SUPPRESS_ERROR_LOGGING_CODES:
            self._log.debug(msg)
        else:
            self._log.warning(msg) if is_warning else self._log.error(msg)

    async def process_error(
        self,
        *,
        req_id: int,
        error_code: int,
        error_string: str,
        advanced_order_reject_json: str = "",
    ) -> None:
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
        advanced_order_reject_json : str
            The JSON string for advanced order rejection.

        """
        is_warning = error_code in self.WARNING_CODES or 2100 <= error_code < 2200
        error_string = error_string.replace("\n", " ")
        await self._log_message(error_code, req_id, error_string, is_warning)

        if req_id != -1:
            if self._subscriptions.get(req_id=req_id):
                await self._handle_subscription_error(req_id, error_code, error_string)
            elif self._requests.get(req_id=req_id):
                await self._handle_request_error(req_id, error_code, error_string)
            elif req_id in self._order_id_to_order_ref:
                await self._handle_order_error(req_id, error_code, error_string)
            else:
                self._log.warning(f"Unhandled error: {error_code} for req_id {req_id}")
        elif error_code in self.CLIENT_ERRORS or error_code in self.CONNECTIVITY_LOST_CODES:
            if self._is_ib_connected.is_set():
                self._log.debug(
                    f"`_is_ib_connected` unset by code {error_code} in `_process_error`.",
                    LogColor.BLUE,
                )
                self._is_ib_connected.clear()
        elif error_code in self.CONNECTIVITY_RESTORED_CODES:
            if not self._is_ib_connected.is_set():
                self._log.debug(
                    f"`_is_ib_connected` set by code {error_code} in `_process_error`.",
                    LogColor.BLUE,
                )
                self._is_ib_connected.set()

    async def _handle_subscription_error(
        self,
        req_id: int,
        error_code: int,
        error_string: str,
    ) -> None:
        """
        Handle errors specific to data subscriptions. Processes subscription-related
        errors and takes appropriate actions, such as canceling the subscription or
        clearing flags.

        Parameters
        ----------
        req_id : int
            The request ID associated with the subscription error.
        error_code : int
            The error code.
        error_string : str
            The error message string.

        """
        subscription = self._subscriptions.get(req_id=req_id)
        if not subscription:
            return
        if error_code in [10189, 366, 102]:
            # Handle specific subscription-related error codes
            self._log.warning(f"{error_code}: {error_string}")
            subscription.cancel()
            if iscoroutinefunction(subscription.handle):
                self._create_task(subscription.handle())
            else:
                subscription.handle()
        elif error_code == 10182:
            # Handle disconnection error
            self._log.warning(f"{error_code}: {error_string}")
            if self._is_ib_connected.is_set():
                self._log.info(
                    f"`_is_ib_connected` unset by {subscription.name} in `_handle_subscription_error`.",
                )
                self._is_ib_connected.clear()
        else:
            # Log unknown subscription errors
            self._log.warning(
                f"Unknown subscription error: {error_code} for req_id {req_id}",
            )

    async def _handle_request_error(self, req_id: int, error_code: int, error_string: str) -> None:
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

        """
        request = self._requests.get(req_id=req_id)
        if error_code == 200:
            self._log.debug(f"{error_code}: {error_string}, {request}")
        else:
            self._log.warning(f"{error_code}: {error_string}, {request}")
        self._end_request(req_id, success=False)

    async def _handle_order_error(self, req_id: int, error_code: int, error_string: str) -> None:
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

        """
        order_ref = self._order_id_to_order_ref.get(req_id, None)
        if not order_ref:
            self._log.warning(f"Order reference not found for req_id {req_id}")
            return

        name = f"orderStatus-{order_ref.account_id}"
        handler = self._event_subscriptions.get(name, None)

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
                f"Unhandled order warning or error code: {error_code} (req_id {req_id}) - "
                f"{error_string}",
            )
