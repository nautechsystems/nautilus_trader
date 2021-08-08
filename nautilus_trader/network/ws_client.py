import asyncio
from asyncio import AbstractEventLoop
from asyncio import IncompleteReadError
from typing import Callable, Dict, List, Optional

import aiohttp
from aiohttp import ClientWebSocketResponse

from nautilus_trader.common.logging import LoggerAdapter


class WebsocketClient:
    def __init__(
        self,
        ws_url: str,
        handler: Callable,
        logger: LoggerAdapter,
        loop: AbstractEventLoop = None,
        ws_connect_kwargs: Optional[Dict] = None,
    ):
        """
        A low level Websocket base client

        :param ws_url: Websocket url to connect to
        :param handler: Callable to handle raw data received
        :param logger: Logger
        :param loop: EventLoop
        :param ws_connect_kwargs: (optional) Additional kwargs to pass to aiohttp.ClientSession._ws_connect()
        """
        self.ws_url = ws_url
        self.handler = handler
        self.ws_connect_kwargs = ws_connect_kwargs or {}
        self._loop = loop or asyncio.get_event_loop()
        self._log = logger
        self._session = None
        self._ws: Optional[ClientWebSocketResponse] = None
        self._tasks: List[asyncio.Task] = []
        self._stop = False
        self._stopped = False

    async def connect(self, start=True):
        self._session = aiohttp.ClientSession(loop=self._loop)
        self._log.debug(f"Connecting to websocket: {self.ws_url}")
        self._ws = await self._session._ws_connect(url=self.ws_url, **self.ws_connect_kwargs)
        if start:
            task = self._loop.create_task(self.start())
            self._tasks.append(task)

    async def disconnect(self):
        self._trigger_stop = True
        while not self._stopped:
            await asyncio.sleep(0.01)
        await self._ws.close()
        self._log.debug("Websocket closed")

    async def send(self, raw: bytes):
        self._log.debug("SEND:" + str(raw))
        await self._ws.send_bytes(raw)

    async def recv(self):
        try:
            resp = await self._ws.receive()
            return resp.data
        except IncompleteReadError as e:
            self._log.exception(e)
            await self.connect(start=False)

    async def start(self):
        self._log.debug("Starting recv loop")
        while not self._stop:
            try:
                raw = await self.recv()
                self._log.debug("[RECV] {raw}")
                if raw is not None:
                    self.handler(raw)
            except Exception as e:
                # TODO - Handle disconnect? Should we reconnect or throw?
                self._log.exception(e)
                self._stop = True
        self._log.debug("stopped")
        self._stopped = True

    async def close(self):
        tasks = [task for task in asyncio.all_tasks() if task is not asyncio.current_task()]
        list(map(lambda task: task.cancel(), tasks))
        return await asyncio.gather(*tasks, return_exceptions=True)
