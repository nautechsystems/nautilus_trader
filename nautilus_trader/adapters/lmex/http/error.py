# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter
#  Licensed under the GNU Lesser General Public License Version 3.0
# -------------------------------------------------------------------------------------------------

from typing import Any


class LmexError(Exception):
    """
    Base class for all LMEX-specific HTTP errors.

    Parameters
    ----------
    status : int
        The HTTP status code returned by the server.
    message : Any
        The error payload (may be a dict, string, or None).
    headers : dict
        The HTTP response headers.

    """

    def __init__(self, status: int, message: Any, headers: dict[str, str]) -> None:
        super().__init__(message)
        self.status = status
        self.message = message
        self.headers = headers

    def __repr__(self) -> str:
        return f"{type(self).__name__}(status={self.status!r}, message={self.message!r})"


class LmexClientError(LmexError):
    """
    Represents an LMEX 4xx (client-side) HTTP error.

    These errors indicate a problem with the request itself — invalid parameters,
    insufficient funds, unknown symbol, etc.

    Parameters
    ----------
    status : int
        The HTTP status code (4xx).
    message : Any
        The error payload from the exchange.
    headers : dict
        The HTTP response headers.

    """

    def __init__(self, status: int, message: Any, headers: dict[str, str]) -> None:
        super().__init__(status, message, headers)


class LmexServerError(LmexError):
    """
    Represents an LMEX 5xx (server-side) HTTP error.

    These errors indicate a transient issue on the exchange side and are
    candidates for automatic retry.

    Parameters
    ----------
    status : int
        The HTTP status code (5xx).
    message : Any
        The error payload from the exchange.
    headers : dict
        The HTTP response headers.

    """

    def __init__(self, status: int, message: Any, headers: dict[str, str]) -> None:
        super().__init__(status, message, headers)


def should_retry(error: BaseException) -> bool:
    """
    Return whether the given error is a candidate for an automatic retry.

    Only 5xx server errors are retried; 4xx client errors are not.

    Parameters
    ----------
    error : BaseException
        The exception to evaluate.

    Returns
    -------
    bool

    """
    return isinstance(error, LmexServerError)
