class DYDXOrderBroadcastError(Exception):
    """
    Define the class for all dYdX specific errors.
    """

    def __init__(self, message: str) -> None:
        super().__init__(message)
        self.message = message
