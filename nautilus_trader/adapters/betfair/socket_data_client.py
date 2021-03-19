import asyncio
import json
import logging
from typing import List

from betfairlightweight.filters import streaming_order_filter


logger = logging.getLogger()


class SocketClient:
    def __init__(self, loop, host, port, crlf, encoding, ssl=True):
        self.loop = loop
        self.host = host
        self.port = port
        self.crlf = crlf
        self.encoding = encoding
        self.ssl = ssl

    def connection_messages(self) -> List:
        raise NotImplementedError

    async def connect(self):
        self.reader, self.writer = await asyncio.open_connection(
            host=self.host, port=self.port, loop=self.loop, ssl=self.ssl
        )

        for msg in self.connection_messages:
            if not isinstance(msg, str):
                msg = json.dumps(msg)
            logger.info("Sending connection message %s" % msg)
            byte_msg = msg.encode(encoding=self.encoding) + self.crlf
            self.writer.write(byte_msg)

    async def listen(self):
        while True:
            try:
                async for data in self.read_line():
                    if data is None:
                        return
                    await self.data_received(data)
            except ConnectionResetError:
                await self.connect()

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

    async def data_received(self, data):
        logger.info(data)
        await asyncio.ensure_future(self.publish(data))


class BetfairSocketClient(SocketClient):
    def __init__(
        self,
        app_key,
        session_token,
        host="stream-api.betfair.com",
        port=443,
        crlf=b"\r\n",
        encoding="utf-8",
    ):
        super().__init__(host=host, port=port, crlf=crlf, encoding=encoding)
        self.app_key = app_key
        self.session_token = session_token

    def connection_messages(self):
        order_filter = streaming_order_filter(include_overall_position=True)
        return [
            {
                "op": "authentication",
                "id": 2,
                "appKey": self.app_key,
                "session": self.session_token,
            },
            {
                "op": "orderSubscription",
                "id": 2,
                "orderFilter": order_filter,
                "initialClk": None,
                "clk": None,
            },
        ]
