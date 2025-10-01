import asyncio
from collections.abc import Awaitable
from collections.abc import Callable
from typing import Any
from weakref import WeakSet

from msgspec import json as msgspec_json
from schwab.streaming import StreamClient
from schwab.streaming import UnexpectedResponse

from nautilus_trader.adapters.schwab.http.client import SchwabHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import Logger
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.nautilus_pyo3 import WebSocketClient
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError
from nautilus_trader.core.nautilus_pyo3 import WebSocketConfig


class SchwabWebSocketClient:
    """
    Provides a Schwab streaming WebSocket client.

    Parameters
    ----------
    clock : LiveClock
        The clock instance.
    handler : Callable[[bytes], None]
        The callback handler for message events.
    handler_reconnect : Callable[..., Awaitable[None]], optional
        The callback handler to be called on reconnect.
    loop : asyncio.AbstractEventLoop
        The event loop for the client.

    """

    def __init__(
        self,
        clock: LiveClock,
        http_client: SchwabHttpClient,
        handler: Callable[[bytes], None],
        handler_reconnect: Callable[..., Awaitable[None]] | None,
        loop: asyncio.AbstractEventLoop,
    ) -> None:
        self._clock = clock
        self._log: Logger = Logger(type(self).__name__)
        self._http_client = http_client
        self._handler: Callable[[bytes], None] = handler
        self._handler_reconnect: Callable[..., Awaitable[None]] | None = handler_reconnect
        self._loop = loop
        self._tasks: WeakSet[asyncio.Task] = WeakSet()
        self._client: WebSocketClient | None = None
        self._auth_event = asyncio.Event()

        self._is_authenticated = False
        self._request_id = 0
        self._base_url = "wss://streamer-api.schwab.com/ws"
        self._stream_correl_id = None
        self._stream_customer_id = None
        self._stream_channel = None
        self._stream_function_id = None
        self._subscriptions: dict[str, list[str]] = {}
        self._order_book_deltas_services = {
            "XNAS": "NASDAQ_BOOK",
            "XNYS": "NYSE_BOOK",
            "SMART": "OPTIONS_BOOK",
        }
        self._ws_auth_timeout_secs = 5.0

    # @property
    # def subscriptions(self) -> list[str]:
    #     return self._subscriptions
    #
    # def has_subscription(self, item: str) -> bool:
    #     return item in self._subscriptions

    async def _init_from_preferences(self, prefs: dict[str, Any]) -> None:
        # Record streamer subscription keys
        stream_info = prefs["streamerInfo"][0]

        self._stream_correl_id = stream_info["schwabClientCorrelId"]
        self._stream_customer_id = stream_info["schwabClientCustomerId"]
        self._stream_channel = stream_info["schwabClientChannel"]
        self._stream_function_id = stream_info["schwabClientFunctionId"]

        # Initialize socket
        self._base_url = stream_info["streamerSocketUrl"]

        config = WebSocketConfig(
            url=self._base_url,
            handler=self._msg_handler,
            headers=[],
        )
        self._client = await WebSocketClient.connect(
            config=config,
        )

    async def connect(self) -> None:
        """
        Connect websocket clients to the server based on existing subscriptions.
        """
        # Recreate the http client in case the token is updated
        http_client = self._http_client.create_schwab_client()
        r = await http_client.get_user_preferences()
        assert r.status_code == 200, r.raise_for_status()
        r = r.json()
        await self._init_from_preferences(r)
        self._log.info(f"Connected to {self._base_url}", LogColor.BLUE)
        await self._authenticate(http_client.token_metadata.token["access_token"])

    async def reconnect(self) -> None:
        """
        Reconnect the client to the server and resubscribe to all streams.
        """
        await self.connect()
        await self._subscribe_all()
        if self._handler_reconnect:
            await self._handler_reconnect()

    async def _subscribe_all(self) -> None:
        if "CHART_EQUITY" in self._subscriptions:
            for symbol in self._subscriptions["CHART_EQUITY"]:
                await self.subscribe_klines(symbol, "1-MINUTE-LAST")
        if "LEVELONE_EQUITIES" in self._subscriptions:
            for symbol in self._subscriptions["LEVELONE_EQUITIES"]:
                await self.subscribe_quote_ticks(symbol)
        if "NASDAQ_BOOK" in self._subscriptions:
            for symbol in self._subscriptions["NASDAQ_BOOK"]:
                await self.subscribe_order_book_deltas(symbol, "XNAS")
        if "NYSE_BOOK" in self._subscriptions:
            for symbol in self._subscriptions["NYSE_BOOK"]:
                await self.subscribe_order_book_deltas(symbol, "XNYS")
        if "OPTIONS_BOOK" in self._subscriptions:
            for symbol in self._subscriptions["OPTIONS_BOOK"]:
                await self.subscribe_order_book_deltas(symbol, "SMART")

    async def disconnect(self) -> None:
        pass

    def _make_request(self, *, service, command, parameters):
        request_id = self._request_id
        self._request_id += 1
        request = {
            "service": service,
            "requestid": str(request_id),
            "command": command,
            "SchwabClientCustomerId": self._stream_customer_id,
            "SchwabClientCorrelId": self._stream_correl_id,
            "parameters": parameters,
        }

        return request, request_id

    async def _authenticate(self, token: str) -> None:
        self._is_authenticated = False
        self._auth_event.clear()
        request_parameters = {
            "Authorization": token,
            "SchwabClientChannel": self._stream_channel,
            "SchwabClientFunctionId": self._stream_function_id,
        }
        request, request_id = self._make_request(
            service="ADMIN",
            command="LOGIN",
            parameters=request_parameters,
        )
        try:
            await self._send({"requests": [request]})
            await asyncio.wait_for(
                self._auth_event.wait(),
                timeout=self._ws_auth_timeout_secs,
            )
        except TimeoutError:
            self._log.warning("Websocket client authentication timeout")
            raise
        except WebSocketClientError as e:
            self._log.exception(
                "Failed to send authentication request",
                {e},
            )
            raise
        except Exception as e:
            self._log.exception(
                "Unexpected error during websocket client authentication",
                {e},
            )
            raise

    def _msg_handler(self, raw: bytes) -> None:
        """
        Handle pushed websocket messages.

        Parameters
        ----------
        raw : bytes
            The received message in bytes.

        """
        msg = msgspec_json.decode(raw)

        if (
            not self._is_authenticated
            and "response" in msg
            and msg["response"][0]["service"] == "ADMIN"
            and msg["response"][0]["command"] == "LOGIN"
            and msg["response"][0]["content"]["code"] == 0
        ):
            # Login response
            self._is_authenticated = True
            self._auth_event.set()
            self._log.info("Websocket client authenticated", LogColor.BLUE)
            return

        if "response" in msg:
            # Sub response
            for d in msg["response"]:
                if d["content"]["code"] == 0:
                    continue
                else:
                    raise UnexpectedResponse(
                        msg,
                        f"Unexpected response code during message handling: "
                        f"{msg['response'][0]['content']['code']}, "
                        f"msg is '{msg['response'][0]['content']['msg']}'",
                    )
            return

        self._handler(raw)

    async def _send(self, msg: dict[str, Any]) -> None:
        await self._send_text(msgspec_json.encode(msg))

    async def _send_text(self, msg: bytes) -> None:
        if self._client is None:
            self._log.error(f"Cannot send message {msg!r}: not connected")
            return

        self._log.debug(f"SENDING: {msg!r}")

        try:
            await self._client.send_text(msg)
        except WebSocketClientError as e:
            self._log.error(str(e))

    def _convert_enum_iterable(self, iterable, required_enum_type):
        if iterable is None:
            return None

        if isinstance(iterable, required_enum_type):
            return [iterable.value]

        values = []
        for value in iterable:
            if isinstance(value, required_enum_type):
                values.append(value.value)
            else:
                values.append(value)
        return values

    async def _subscribe(self, symbol: str, service: str, command: str, field_type=None) -> None:
        self._log.debug(f"Subscribing to {service}.{command} for {symbol}")
        parameters = {
            # 'keys': ','.join(symbol)
            "keys": symbol,
        }

        if field_type is not None:
            fields = field_type.all_fields()

            fields = sorted(self._convert_enum_iterable(fields, field_type))
            parameters["fields"] = ",".join(str(f) for f in fields)

        request, request_id = self._make_request(
            service=service,
            command=command,
            parameters=parameters,
        )

        await self._send(request)

    ################################################################################
    # Public
    ################################################################################

    async def subscribe_klines(self, symbol: str, interval: str) -> None:
        if interval != "1-MINUTE-LAST":
            raise NotImplementedError
        service = "CHART_EQUITY"
        command = None
        if service not in self._subscriptions:
            self._subscriptions[service] = [symbol]
            command = "SUBS"
        else:
            if symbol in self._subscriptions[service]:
                self._log.warning(f"Cannot subscribe {service} for '{symbol}': already subscribed")
                return
            self._subscriptions[service].append(symbol)
            command = "ADD"
        await self._subscribe(symbol, service, command, field_type=StreamClient.ChartEquityFields)

    async def unsubscribe_klines(self, symbol: str, interval: str) -> None:
        if interval != "1-MINUTE-LAST":
            raise NotImplementedError
        service = "CHART_EQUITY"
        command = "UNSUBS"
        if service not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe {service} for '{symbol}': not subscribed")
            return
        else:
            if symbol not in self._subscriptions[service]:
                self._log.warning(f"Cannot unsubscribe {service} for '{symbol}': not subscribed")
                return
            self._subscriptions[service].remove(symbol)
        await self._subscribe(symbol, service, command)

    async def subscribe_quote_ticks(self, symbol: str) -> None:
        service = "LEVELONE_EQUITIES"
        command = None
        if service not in self._subscriptions:
            self._subscriptions[service] = [symbol]
            command = "SUBS"
        else:
            if symbol in self._subscriptions[service]:
                self._log.warning(f"Cannot subscribe {service} for '{symbol}': already subscribed")
                return
            self._subscriptions[service].append(symbol)
            command = "ADD"
        await self._subscribe(
            symbol,
            service,
            command,
            field_type=StreamClient.LevelOneEquityFields,
        )

    async def unsubscribe_quote_ticks(self, symbol: str) -> None:
        service = "LEVELONE_EQUITIES"
        command = "UNSUBS"
        if service not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe {service} for '{symbol}': not subscribed")
            return
        else:
            if symbol not in self._subscriptions[service]:
                self._log.warning(f"Cannot unsubscribe {service} for '{symbol}': not subscribed")
                return
            self._subscriptions[service].remove(symbol)
        await self._subscribe(symbol, service, command)

    async def subscribe_order_book_deltas(self, symbol: str, venue: str) -> None:
        service = self._order_book_deltas_services[venue]
        command = "SUBS"
        if service not in self._subscriptions:
            self._subscriptions[service] = [symbol]
            command = "SUBS"
        else:
            if symbol in self._subscriptions[service]:
                self._log.warning(f"Cannot subscribe {service} for '{symbol}': already subscribed")
                return
            self._subscriptions[service].append(symbol)
            command = "ADD"
        await self._subscribe(symbol, service, command, field_type=StreamClient.BookFields)

    async def unsubscribe_order_book_deltas(self, symbol: str, venue: str) -> None:
        service = self._order_book_deltas_services[venue]
        command = "UNSUBS"
        if service not in self._subscriptions:
            self._log.warning(f"Cannot unsubscribe {service} for '{symbol}': not subscribed")
            return
        else:
            if symbol not in self._subscriptions[service]:
                self._log.warning(f"Cannot unsubscribe {service} for '{symbol}': not subscribed")
                return
            self._subscriptions[service].remove(symbol)
        await self._subscribe(symbol, service, command)
