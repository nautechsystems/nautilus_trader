class SchwabError(Exception):
    """
    The base class for all Schwab specific errors.
    """

    def __init__(self, status, message, headers):
        super().__init__(message)
        self.status = status
        self.message = message
        self.headers = headers


def should_retry(error: BaseException) -> bool:
    return False
