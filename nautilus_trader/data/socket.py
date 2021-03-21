import asyncio
import json

from nautilus_trader.data.client import DataClient


DEFAULT_CRLF = b"\r\n"


class SocketClient(DataClient):
    def __init__(
        self,
        host,
        port,
        message_handler,
        loop=None,
        connection_messages=None,
        crlf=None,
        encoding="utf-8",
        ssl=True,
    ):
        """

        :param loop: Event loop
        :param host: host to connect to
        :param port: Port to connect on
        :param message_handler: A callable to process the raw bytes read from the socket
        :param connection_messages: A list of messages to send on connection
        :param crlf: Carriage Return, Line Feed; Delimiter on which to split messages
        :param encoding: Encoding to use when sending messages
        :param ssl: Use SSL for socket connection
        """
        self.host = host
        self.port = port
        self.message_handler = message_handler
        self.loop = loop or asyncio.get_event_loop()
        self.connection_messages = connection_messages
        self.crlf = crlf or DEFAULT_CRLF
        self.encoding = encoding
        self.ssl = ssl
        self.reader = None
        self.write = None
        self.connected = False
        self.stop = False

    async def connect(self):
        if not self.connected:
            self.reader, self.writer = await asyncio.open_connection(
                host=self.host, port=self.port, loop=self.loop, ssl=self.ssl
            )
            await self.post_connect()
            self.connected = True

    async def post_connect(self):
        """
        Called straight after connection, sends `connection_messages` one by one.

        Can be overriden for more custom workflows.

        :return:
        """
        for msg in self.connection_messages:
            if not isinstance(msg, str):
                msg = json.dumps(msg)
            print(f"Sending connection message {msg}")
            byte_msg = msg.encode(encoding=self.encoding) + self.crlf
            self.writer.write(byte_msg)

    async def start(self):
        if not self.connected:
            await self.connect()
        while not self.stop:
            try:
                async for raw in self.read_line():
                    if raw is None:
                        break
                    self.message_handler(raw)
                    if self.stop:
                        break
            except ConnectionResetError:
                await self.connect()
        await self.shutdown()

    async def read_line(self):
        data, part = b"", b""
        while True:
            part = await self.reader.read(1024)

            if not part and self.reader.at_eof:
                yield

            if part:
                data += part

            if self.crlf in data:
                lines = data.split(self.crlf)
                data, part = lines[-1], b""

                for line in lines[:-1]:
                    yield line

    async def shutdown(self):
        self.writer.close()
        await self.writer.wait_closed()
