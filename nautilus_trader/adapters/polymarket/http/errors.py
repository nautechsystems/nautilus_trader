from typing import Any

from py_clob_client.exceptions import PolyApiException


class PolymarketError(Exception):
    """
    Represents a Polymarket specific error.
    """

    def __init__(
        self,
        code: int | None,
        message: str | None,
    ) -> None:
        super().__init__(message)
        self.code = code
        self.message = message

    def __repr__(self) -> str:
        return f"{type(self).__name__}(code={self.code}, message='{self.message}')"


class PolymarketAPIError(PolymarketError):
    """
    Represents an error response from the Polymarket CLOB API.

    Raised when the API returns an error string instead of expected data.

    """

    def __init__(self, message: str) -> None:
        super().__init__(code=None, message=message)


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
    if isinstance(error, PolyApiException):
        # https://github.com/Polymarket/py-clob-client/blob/main/py_clob_client/exceptions.py
        status_code = getattr(error, "status_code", None)

        # Retry on rate limits and server errors
        if status_code == 429 or (status_code is not None and status_code >= 500):
            return True

    return False


def check_clob_response(response: dict[str, Any] | str) -> dict[str, Any]:
    """
    Check CLOB API response and raise exception if error string returned.

    Parameters
    ----------
    response : dict[str, Any] | str
        The response from the CLOB API.

    Returns
    -------
    dict[str, Any]
        The validated response dictionary.

    Raises
    ------
    PolymarketAPIError
        If response is an error string.

    """
    if isinstance(response, str):
        raise PolymarketAPIError(response)
    return response
