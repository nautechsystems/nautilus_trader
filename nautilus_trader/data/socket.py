import asyncio
from asyncio import IncompleteReadError
from typing import Callable, Optional

import orjson

from nautilus_trader.common.logging import LoggerAdapter


DEFAULT_CRLF = b"\r\n"


# TODO - Need to add DataClient subclass back
class SocketClient:
    def __init__(
        self,
        host,
        port,
        logger_adapter: LoggerAdapter,
        message_handler: Callable,
        loop=None,
        crlf=None,
        encoding="utf-8",
        ssl=True,
    ):
        """

        :param loop: Event loop
        :param host: host to connect to
        :param port: Port to connect on
        :param message_handler: A callable to process the raw bytes read from the socket
        :param crlf: Carriage Return, Line Feed; Delimiter on which to split messages
        :param encoding: Encoding to use when sending messages
        :param ssl: Use SSL for socket connection
        """
        super().__init__()
        self.host = host
        self.port = port
        self.logger = logger_adapter
        self.message_handler = message_handler
        self.loop = loop or asyncio.get_event_loop()
        self.crlf = crlf or DEFAULT_CRLF
        self.encoding = encoding
        self.ssl = ssl
        self.reader: Optional[asyncio.StreamReader] = None
        self.writer: Optional[asyncio.StreamWriter] = None
        self.connected = False
        self._stop = False
        self._stopped = False

    async def connect(self):
        if not self.connected:
            self.reader, self.writer = await asyncio.open_connection(
                host=self.host, port=self.port, loop=self.loop, ssl=self.ssl
            )
            await self.post_connection()
            self.connected = True

    async def disconnect(self):
        self.stop()
        while not self._stopped:
            await asyncio.sleep(0.01)
        self.writer.close()
        await self.writer.wait_closed()
        self.reader = None
        self.writer = None
        self.connected = False

    def stop(self):
        self._stop = True

    async def reconnect(self):
        await self.disconnect()
        await self.connect()

    async def post_connection(self):
        """
        Overridable hook for any post-connection duties, i.e. sending further connection messages
        """
        await asyncio.sleep(0)

    async def send(self, raw):
        if not isinstance(raw, (bytes, str)):
            raw = orjson.dumps(raw)
        if not isinstance(raw, bytes):
            raw = raw.encode(self.encoding)
        self.logger.debug(f"SEND: {raw.decode()}")
        self.writer.write(raw + self.crlf)
        await self.writer.drain()

    async def start(self):
        partial = b""
        if not self.connected:
            await self.connect()
        while not self._stop:
            try:
                raw = await self.reader.readuntil(separator=self.crlf)
                if partial:
                    raw = partial + raw
                    partial = b""
                self.logger.debug(f"RECV: {raw.decode()}")
                self.message_handler(raw.rstrip(self.crlf))
                await asyncio.sleep(0)
            except IncompleteReadError as e:
                partial = e.partial
                self.logger.warning(str(e))
                continue
            except ConnectionResetError:
                await self.connect()
        self._stopped = True
