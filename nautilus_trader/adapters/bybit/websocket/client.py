from typing import Callable

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.nautilus_pyo3.network import WebSocketClient


class BybitWebsocketClient:
    def __init__(
        self,
        clock: LiveClock,
        logger: Logger,
        base_url: str,
        handler: Callable[[bytes], None],
    ) -> None:
        self._clock = clock
        self._logger = logger
        self._log: LoggerAdapter = LoggerAdapter(type(self).__name__, logger=logger)
        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler

        self._streams_connecting: set[str] = set()
        self._streams: dict[str, WebSocketClient] = {}

    @property
    def url(self) -> str:
        """
        Return the server URL being used by the client.

        Returns
        -------
        str

        """
        return self._base_url

    @property
    def subscriptions(self) -> list[str]:
        """
        Return the current active subscriptions for the client.

        Returns
        -------
        str

        """
        return list(self._streams.keys())

    @property
    def has_subscriptions(self) -> bool:
        """
        Return whether the client has subscriptions.

        Returns
        -------
        bool

        """
        return bool(self._streams)

    async def subscribe_trades(self, symbol: str) -> None:
        self._connect(f"publicTrace.{symbol}")

    async def _connect(self, stream: str) -> None:
        if stream not in self._streams and stream not in self._streams_connecting:
            self._streams_connecting.add(stream)
            await self.connect(stream)

    async def connect(self, stream: str) -> None:
        pass
