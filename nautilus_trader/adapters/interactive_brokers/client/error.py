# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.identifiers import VenueOrderId


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
    CLIENT_ERRORS: Final[set[int]] = {502, 503, 504, 10038, 1100, 2110}
    CONNECTIVITY_LOST_CODES: Final[set[int]] = {326, 1100, 1300, 2110}
    CONNECTIVITY_RESTORED_CODES: Final[set[int]] = {1101, 1102}
    # Transient data-farm status notifications. A "broken" code degrades only the affected data
    # feeds (market-data farm 2103, historical/HMDS farm 2105); the matching "OK" code resumes
    # them (market-data farm 2104, HMDS farm 2106). Per IB these are expected during the nightly
    # maintenance restart and must NOT be treated as full connectivity loss (which would tear
    # down the whole socket, including the order/execution channel).
    DATA_FARM_BROKEN_CODES: Final[set[int]] = {2103, 2105}
    DATA_FARM_OK_CODES: Final[set[int]] = {2104, 2106}
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
        msg = f"{error_string} (code: {error_code}, {req_id=})"

        if error_code in self.SUPPRESS_ERROR_LOGGING_CODES:
            self._log.debug(msg)
        else:
            self._log.warning(msg) if is_warning else self._log.error(msg)

    async def process_error(
        self,
        *,
        req_id: int,
        error_time: int,
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
        error_time : int
            The timestamp when the error occurred.
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
            elif VenueOrderId(str(req_id)) in self._order_id_to_order_ref:
                await self._handle_order_error(req_id, error_code, error_string)
            else:
                self._log.warning(f"Unhandled error: {error_code} for req_id {req_id}")
        else:
            self._handle_connectivity_message(error_code)

    def _handle_connectivity_message(self, error_code: int) -> None:
        """
        Handle a general (non request-specific) status or connectivity message.

        Transient data-farm notifications degrade only the data feeds (keeping the socket, and
        so the order/execution channel, alive); genuine connectivity-lost codes clear the
        connection so the watchdog can reconnect; connectivity-restored codes set it again.

        Parameters
        ----------
        error_code : int
            The status or error code (dispatched with req_id == -1).

        """
        if error_code in self.DATA_FARM_BROKEN_CODES:
            self._mark_data_farm_degraded(error_code)
        elif error_code in self.DATA_FARM_OK_CODES:
            self._handle_data_farm_ok(error_code)
        elif error_code in self.CLIENT_ERRORS or error_code in self.CONNECTIVITY_LOST_CODES:
            if error_code == 326:
                self._client_id_collision_count += 1
                self._log.warning(
                    f"IB error 326 (client id already in use), collision "
                    f"#{self._client_id_collision_count}: backing off and retrying with the same "
                    f"client id (falls back to a bounded id band after "
                    f"{self._client_id_reuse_limit} consecutive collisions)",
                )

            if self._is_ib_connected.is_set():
                self._log.debug(
                    f"`_is_ib_connected` unset by code {error_code} in `_process_error`",
                    LogColor.BLUE,
                )
                self._is_ib_connected.clear()
        elif error_code in self.CONNECTIVITY_RESTORED_CODES and not self._is_ib_connected.is_set():
            self._log.debug(
                f"`_is_ib_connected` set by code {error_code} in `_process_error`",
                LogColor.BLUE,
            )
            self._is_ib_connected.set()
            self._had_ib_connection = True

    def _mark_data_farm_degraded(self, error_code: int) -> None:
        """
        Flag the data feeds as degraded after a transient data-farm "broken"
        notification.

        Records the time of the first degradation (used to backfill bars missed during the
        outage) and keeps the socket, including the order/execution channel, alive. The
        connection watchdog is only tripped by genuine socket loss, never by a data-farm blip.

        Parameters
        ----------
        error_code : int
            The data-farm error code which triggered the degradation.

        """
        if self._data_farm_degraded_since_ns is None:
            self._data_farm_degraded_since_ns = self._clock.timestamp_ns()

        self._log.debug(
            f"Data farm degraded by code {error_code}; feeds will resubscribe on farm-OK",
            LogColor.BLUE,
        )

    def _handle_data_farm_ok(self, error_code: int) -> None:
        """
        Resume degraded data feeds after a data-farm "OK" notification.

        Resubscribes the affected feeds without a socket teardown. Bars that completed during
        the outage are recovered via the historical backfill (gated on the farm being OK, not
        merely on the socket being up). A no-op when no degradation is outstanding.

        Parameters
        ----------
        error_code : int
            The data-farm error code which signalled recovery.

        """
        if self._data_farm_degraded_since_ns is None:
            return

        if (
            self._data_farm_resubscription_task is not None
            and not self._data_farm_resubscription_task.done()
        ):
            return

        self._log.info(f"Data farm connection is OK (code {error_code}); resubscribing feeds")
        self._data_farm_resubscription_task = self._create_task(
            self._resubscribe_after_farm_recovery(),
        )

    async def _resubscribe_after_farm_recovery(self) -> None:
        try:
            await self._resubscribe_all()
        finally:
            self._data_farm_degraded_since_ns = None

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
            # "Failed to request live updates (disconnected)" - a data-farm level drop of this
            # subscription, typically during a farm outage. Do NOT treat it as full connectivity
            # loss (which would tear down the whole socket, including the order/execution
            # channel); keep the socket alive and mark the data feeds degraded so they
            # resubscribe once the farm recovers (2104/2106).
            self._log.warning(f"{error_code}: {error_string}")
            self._mark_data_farm_degraded(error_code)
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
        # Use VenueOrderId as dict key (for orders we placed, req_id is the valid orderId)
        order_ref = self._order_id_to_order_ref.get(VenueOrderId(str(req_id)), None)

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
