import hashlib
import hmac
import json
from typing import Callable, Optional

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogColor
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
        api_key: Optional[str] = None,
        api_secret: Optional[str] = None,
        is_private: Optional[bool] = False
    ) -> None:
        self._clock = clock
        self._logger = logger
        self._log: LoggerAdapter = LoggerAdapter(type(self).__name__, logger=logger)
        self._base_url: str = base_url
        self._handler: Callable[[bytes], None] = handler
        self._client: WebSocketClient = None
        self._is_private = is_private
        self._api_key = api_key
        self._api_secret = api_secret

        self._streams_connecting: set[str] = set()
        self._subscriptions: list[str] = []

    @property
    def url(self) -> str:
        return self._base_url

    @property
    def subscriptions(self) -> list[str]:
        return self._subscriptions

    def has_subscriptions(self,item: str) -> bool:
        return item in self._subscriptions

    ################################################################################
    # Public
    ################################################################################


    async def subscribe_trades(self, symbol: str) -> None:
        subscription = f"publicTrade.{symbol}"
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))
        self._subscriptions.append(subscription)

    async def subscribe_tickers(self,symbol: str)-> None:
        subscription = f"tickers.{symbol}"
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))
        self._subscriptions.append(subscription)


    ################################################################################
    # Private
    ################################################################################
    async def subscribe_account_position_update(self) -> None:
        sub = {"op": "subscribe", "args": ["position"]}

    async def subscribe_orders_update(self) -> None:
        subscription = "order"
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))
        self._subscriptions.append(subscription)

    async def subscribe_executions_update(self) -> None:
        subscription = "execution"
        sub = {"op": "subscribe", "args": [subscription]}
        await self._client.send_text(json.dumps(sub))
        self._subscriptions.append(subscription)

    async def connect(self) -> None:
        self._log.debug(f"Connecting to {self.url} websocket stream")
        client = await WebSocketClient.connect(
            url=self.url,
            handler=self._handler,
            heartbeat=15,
        )
        self._client = client
        self._log.info(f"Connected to {self.url}.", LogColor.BLUE)
        ## authenticate
        if self._is_private:
            signature = self._get_signature()
            self._client.send_text(json.dumps(signature))


    def _get_signature(self):
        timestamp = self._clock.timestamp_ms()+1000
        sign = f"GET/realtime{timestamp}"
        signature = hmac.new(self._api_secret.encode("utf-8"), sign.encode("utf-8"), hashlib.sha256).hexdigest()
        return {
            "op": "auth",
            "args": [self._api_key, timestamp, signature],
        }

    async def disconnect(self) -> None:
        await self._client.send_text(json.dumps({"op": "unsubscribe", "args": self._subscriptions}))
        await self._client.disconnect()
        self._log.info(f"Disconnected from {self.url}.", LogColor.BLUE)