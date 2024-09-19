from typing import Any


class OKXHttpError(Exception):
    """
    Define the class for all `OKX` http specific errors.
    """

    def __init__(self, status: int, message: str, headers: dict[str, Any]) -> None:
        """
        Define the base class for all `OKX` http specific errors.
        """
        super().__init__(message)
        self.status = status
        self.message = message
        self.headers = headers
