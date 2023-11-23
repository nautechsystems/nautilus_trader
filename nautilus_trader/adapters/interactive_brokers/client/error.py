from inspect import iscoroutinefunction


class InteractiveBrokersErrorHandler:
    """
    A dedicated error handling class for the InteractiveBrokersClient.

    This class handles various error and warning scenarios that might arise during the
    operation of the InteractiveBrokersClient.

    https://ibkrcampus.com/ibkr-api-page/tws-api-error-codes/#understanding-error-codes

    """

    WARNING_CODES = {1101, 1102, 110, 165, 202, 399, 404, 434, 492, 10167}
    CLIENT_ERRORS = {502, 503, 504, 10038, 10182, 1100, 2110}
    CONNECTIVITY_LOST_CODES = {1100, 2110}
    CONNECTIVITY_RESTORED_CODES = {1101, 1102}
    ORDER_REJECTION_CODES = {201, 203, 321, 10289, 10293}

    def __init__(self, client):
        self._client = client

    def _log_message(self, error_code: int, req_id: int, error_string: str, is_warning: bool):
        msg_type = "Warning" if is_warning else "Error"
        msg = f"{msg_type} {error_code} {req_id=}: {error_string}"
        if is_warning:
            self._client._log.info(msg)
        else:
            self._client._log.error(msg)

    def process_error(self, req_id: int, error_code: int, error_string: str):
        is_warning = error_code in self.WARNING_CODES or 2100 <= error_code < 2200
        self._log_message(error_code, req_id, error_string, is_warning)

        if req_id != -1:
            if self._client.subscriptions.get(req_id=req_id):
                self._handle_subscription_error(req_id, error_code, error_string)
            elif self._client.requests.get(req_id=req_id):
                self._handle_request_error(req_id, error_code, error_string)
            elif req_id in self._client._order_id_to_order_ref:
                self._handle_order_error(req_id, error_code, error_string)
            else:
                self._client._log.warning(f"Unhandled error: {error_code} for req_id {req_id}")
        elif error_code in self.CLIENT_ERRORS or error_code in self.CONNECTIVITY_LOST_CODES:
            self._client._log.warning(f"Client or Connectivity Lost Error: {error_string}")
            if self._client.is_ib_ready.is_set():
                self._client.is_ib_ready.clear()
        elif error_code in self.CONNECTIVITY_RESTORED_CODES:
            if not self._client.is_ib_ready.is_set():
                self._client.is_ib_ready.set()

    def _handle_subscription_error(self, req_id: int, error_code: int, error_string: str):
        subscription = self._client.subscriptions.get(req_id=req_id)
        if error_code in [10189, 366, 102]:
            # Handle specific subscription-related error codes
            self._client._log.warning(f"{error_code}: {error_string}")
            subscription.cancel()
            if iscoroutinefunction(subscription.handle):
                self._client.create_task(subscription.handle())
            else:
                subscription.handle()
        elif error_code == 10182:
            # Handle disconnection error
            self._client._log.warning(f"{error_code}: {error_string}")
            if self._client.is_ib_ready.is_set():
                self._client._log.info(f"`is_ib_ready` cleared by {subscription.name}")
                self._client.is_ib_ready.clear()
        else:
            # Log unknown subscription errors
            self._client._log.warning(
                f"Unknown subscription error: {error_code} for req_id {req_id}",
            )

    def _handle_request_error(self, req_id: int, error_code: int, error_string: str):
        request = self._client.requests.get(req_id=req_id)
        self._client._log.warning(f"{error_code}: {error_string}, {request}")
        self._client._end_request(req_id, success=False)

    def _handle_order_error(self, req_id: int, error_code: int, error_string: str):
        order_ref = self._client._order_id_to_order_ref.get(req_id, None)
        if not order_ref:
            self._client._log.warning(f"Order reference not found for req_id {req_id}")
            return

        name = f"orderStatus-{order_ref.account}"
        handler = self._client._event_subscriptions.get(name, None)

        if error_code in self.ORDER_REJECTION_CODES:
            # Handle various order rejections
            if handler:
                handler(order_ref=order_ref.order_id, order_status="Rejected", reason=error_string)
        elif error_code == 202:
            # Handle order cancellation warning
            if handler:
                handler(order_ref=order_ref.order_id, order_status="Cancelled", reason=error_string)
        else:
            # Log unknown order errors
            self._client._log.warning(f"Unknown order error: {error_code} for req_id {req_id}")
