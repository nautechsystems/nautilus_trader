from nautilus_trader.adapters.binance.common.constants import BINANCE_RETRY_ERRORS
from nautilus_trader.adapters.binance.common.enums import BinanceErrorCode
from nautilus_trader.core.nautilus_pyo3 import HttpTimeoutError


class BinanceError(Exception):
    """
    The base class for all Binance specific errors.
    """

    def __init__(self, status, message, headers):
        super().__init__(message)
        self.status = status
        self.message = message
        self.headers = headers


class BinanceServerError(BinanceError):
    """
    Represents an Binance specific 500 series HTTP error.
    """

    def __init__(self, status, message, headers):
        super().__init__(status, message, headers)


class BinanceClientError(BinanceError):
    """
    Represents an Binance specific 400 series HTTP error.
    """

    def __init__(self, status, message, headers):
        super().__init__(status, message, headers)


def is_transport_timeout_error(error: BaseException) -> bool:
    """
    Return whether the error is a transport timeout from the Python or pyo3 HTTP layer.
    """
    return isinstance(error, (TimeoutError, HttpTimeoutError))


def classify_transport_error_type(error: BaseException) -> str | None:
    """
    Normalize transport errors to stable type names for logs and health payloads.
    """
    if is_transport_timeout_error(error):
        return "TimeoutError"
    return None


def get_binance_error_code(error: BaseException) -> BinanceErrorCode | None:
    """
    Extract the Binance error code from an exception.

    Parameters
    ----------
    error : BaseException
        The error to extract the code from.

    Returns
    -------
    BinanceErrorCode | None
        The error code if it can be extracted, otherwise None.

    """
    if isinstance(error, BinanceError):
        try:
            # Handle case where message might be a dict, string, or missing 'code' key
            if isinstance(error.message, dict) and "code" in error.message:
                return BinanceErrorCode(int(error.message["code"]))
            elif isinstance(error.message, str):
                # Try to parse error code from string format like '{"code":-1021,"msg":"..."}'
                import json

                try:
                    parsed_message = json.loads(error.message)
                    if isinstance(parsed_message, dict) and "code" in parsed_message:
                        return BinanceErrorCode(int(parsed_message["code"]))
                except (json.JSONDecodeError, ValueError, KeyError):
                    pass
        except (ValueError, KeyError, TypeError):
            pass  # If any parsing fails, return None

    return None


def should_retry(error: BaseException) -> bool:
    """
    Determine if a retry should be attempted based on the error code.

    Parameters
    ----------
    error : BaseException
        The error to check.

    Returns
    -------
    bool
        True if should retry, otherwise False.

    """
    error_code = get_binance_error_code(error)
    return error_code in BINANCE_RETRY_ERRORS if error_code else False
