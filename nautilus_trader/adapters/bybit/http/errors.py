class BybitError(Exception):
    def __init__(self, status, message, headers):
        super().__init__(message)
        self.status = status
        self.message = message
        self.headers = headers
