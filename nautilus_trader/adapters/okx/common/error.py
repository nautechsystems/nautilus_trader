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


def raise_okx_error(error_code: int, status_code: int | None, message: str | None) -> None:
    if error_code in OKXGeneralError.error_code_messages:
        raise OKXGeneralError(error_code, status_code, message)
    raise OKXError(error_code, status_code, f"OKX error: {error_code=}, {status_code=}, {message=}")


class OKXError(Exception):
    """
    The base class for all OKX specific errors.

    References
    ----------
    https://www.okx.com/docs-v5/en/?python#error-code

    """

    def __init__(self, error_code: int, status_code: int | None, message: str | None):
        super().__init__(message)
        self.error_code = error_code
        self.status_code = status_code
        self.message = message


# TODO implement specific error classes from https://www.okx.com/docs-v5/en/?python#error-code
# e.g., API Class, etc


class OKXGeneralError(OKXError):
    """
    Provides error codes and messages for OKX General Class exceptions.
    """

    error_code_messages = {
        50000: "Body for POST request cannot be empty.",
        50001: "Service temporarily unavailable. Try again later",
        50002: "JSON syntax error",
        50004: "API endpoint request timeout (does not mean that the request was successful or "
        "failed, please check the request result).",
        50005: "API is offline or unavailable.",
        50006: "Invalid Content-Type. Please use 'application/JSON'.",
        50007: "Account blocked.",
        50008: "User does not exist.",
        50009: "Account is suspended due to ongoing liquidation.",
        50010: "User ID cannot be empty.",
        50011: {
            200: "Rate limit reached. Please refer to API documentation and throttle requests "
            "accordingly.",
            429: "Requests too frequent",
        },
        50012: "Account status invalid. Check account status",
        50013: "Systems are busy. Please try again later.",
        50014: "Parameter {param0} cannot be empty.",
        50015: "Either parameter {param0} or {param1} is required.",
        50016: "Parameter {param0} and {param1} is an invalid pair.",
        50017: "Position frozen and related operations restricted due to auto-deleveraging (ADL). "
        "Try again later",
        50018: "{param0} frozen and related operations restricted due to auto-deleveraging (ADL). "
        "Try again later",
        50019: "Account frozen and related operations restricted due to auto-deleveraging (ADL). "
        "Try again later",
        50020: "Position frozen and related operations are restricted due to liquidation. "
        "Try again later",
        50021: "{param0} frozen and related operations are restricted due to liquidation. "
        "Try again later",
        50022: "Account frozen and related operations are restricted due to liquidation. "
        "Try again later",
        50023: "Funding fees frozen and related operations are restricted. Try again later",
        50024: "Either parameter {param0} or {param1} should be submitted.",
        50025: "Parameter {param0} count exceeds the limit {param1}.",
        50026: "System error. Try again later",
        50027: "This account is restricted from trading. Please contact customer support for "
        "assistance.",
        50028: "Unable to take the order, please reach out to support center for details.",
        50029: "Your account has triggered OKX risk control and is temporarily restricted from "
        "conducting transactions. Please check your email registered with OKX for contact from our "
        "customer support team.",
        50030: "You don't have permission to use this API endpoint",
        50032: "Your account has been set to prohibit transactions in this currency. Please "
        "confirm and try again",
        50033: "Instrument blocked. Please verify trading this instrument is allowed under account "
        "settings and try again.",
        50035: "This endpoint requires that APIKey must be bound to IP",
        50036: "The expTime can't be earlier than the current system time. Please adjust the "
        "expTime and try again.",
        50037: "Order expired.",
        50038: "This feature is unavailable in demo trading",
        50039: "Parameter 'before' isn't supported for timestamp pagination",
        50040: "Too frequent operations, please try again later",
        50041: "Your user ID hasn't been allowlisted. Please contact customer service for "
        "assistance.",
        50044: "Must select one broker type",
        50047: "{param0} has already settled. To check the relevant candlestick data, please use "
        "{param1}",
        50048: "Switching risk unit may lead position risk increases and be forced liquidated. "
        "Please adjust position size, make sure margin is in a safe status.",
        50049: "No information on the position tier. The current instrument doesn't support margin "
        "trading.",
        50050: "You've already activated options trading. Please don't activate it again.",
        50051: "Due to compliance restrictions in your country or region, you cannot use this "
        "feature.",
        50052: "Due to local laws and regulations, you cannot trade with your chosen crypto.",
        50053: "This feature is only available in demo trading.",
        50055: "Reset unsuccessful. Assets can only be reset up to 5 times per day.",
        50056: "You have pending orders or open positions with this currency. Please reset after "
        "canceling all the pending orders/closing all the open positions.",
        50057: "Reset unsuccessful. Try again later.",
        50058: "This crypto is not supported in an asset reset.",
        50059: "Before you continue, you'll need to complete additional steps as required by your "
        "local regulators. Please visit the website or app for more details.",
        50060: "For security and compliance purposes, please complete the identity verification "
        "process to continue using our services.",
        50061: "You've reached the maximum order rate limit for this account.",
        50063: "You can't activate the credits as they might have expired or are already activated",
        50064: "The borrowing system is unavailable. Try again later.",
    }

    def __init__(self, error_code: int, status_code: int | None, message: str | None):
        assert error_code in self.error_code_messages, (
            f"Error code {error_code} is not an OKX General Class error, see "
            "https://www.okx.com/docs-v5/en/?python#error-code for details"
        )
        message: str | dict[int, str] = message or self.error_code_messages[error_code]  # type: ignore
        if isinstance(message, dict):
            if status_code is not None:
                message = message[status_code]
            else:
                message = (
                    "Error is one of: [" + ", ".join(f"{msg!r}" for msg in message.values()) + "]"
                )
        super().__init__(error_code, status_code, message)
