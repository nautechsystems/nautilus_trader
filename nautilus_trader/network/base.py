# TODO - There's a lot of overlap between tcp and web socket - should they share a base class like this?


class StreamingBase:
    def __init__(self, logger, recv_handler: callable):  # type: ignore
        """
        Base class for WebsocketClient and TCPClient handling some shared read/write/connection functionality

        :param logger:
        :param recv_handler: Called with data received from recv()
        """
        self.logger = logger
        self.recv_handler = recv_handler
        self._stop = False
        self._stopped = False

    async def connect(self):
        raise NotImplementedError

    async def disconnect(self):
        raise NotImplementedError

    async def _recv(self) -> bytes:
        raise NotImplementedError

    async def _send(self, data: bytes) -> None:
        raise NotImplementedError

    async def send(self, data: bytes):
        # TODO - Ensure connection is alive
        return self._send(data=data)

    async def recv(self) -> bytes:
        # TODO - Ensure connection is alive
        return await self._recv()

    async def start(self):
        while not self._stop:
            try:
                raw = await self.recv()
                self.logger.debug("[RECV] {raw}")
                if raw is not None:
                    self.recv_handler(raw)
            except Exception as e:
                # TODO - Handle disconnect? Should we reconnect or throw?
                self.logger.exception(e)
                self._stop = True
        self.logger.debug("stopped")
        self._stopped = True
