# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import asyncio
import itertools
from collections.abc import Callable

import msgspec

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.common.component import Logger
from nautilus_trader.core.nautilus_pyo3 import SocketClient
from nautilus_trader.core.nautilus_pyo3 import SocketConfig


HOST = "stream-api.betfair.com"
# HOST = "stream-api-integration.betfair.com"
PORT = 443
CRLF = b"\r\n"
ENCODING = "utf-8"
UNIQUE_ID = itertools.count()
USE_SSL = True


class BetfairStreamClient:
    """
    Provides a streaming client for `Betfair`.
    """

    def __init__(
        self,
        http_client: BetfairHttpClient,
        message_handler: Callable[[bytes], None],
        host: str | None = HOST,
        port: int | None = None,
        crlf: bytes | None = None,
        encoding: str | None = None,
    ) -> None:
        self._http_client = http_client
        self._log = Logger(type(self).__name__)
        self.handler = message_handler
        self.host = host or HOST
        self.port = port or PORT
        self.crlf = crlf or CRLF
        self.use_ssl = USE_SSL
        self.encoding = encoding or ENCODING
        self._client: SocketClient | None = None
        self.unique_id = next(UNIQUE_ID)
        self.is_connected: bool = False
        self.disconnecting: bool = False
        self._loop = asyncio.get_event_loop()

    async def connect(self):
        if not self._http_client.session_token:
            await self._http_client.connect()

        if self.is_connected:
            self._log.info("Socket already connected.")
            return

        self._log.info("Connecting betfair socket client..")
        config = SocketConfig(
            url=f"{self.host}:{self.port}",
            handler=self.handler,
            ssl=self.use_ssl,
            suffix=self.crlf,
        )
        self._client = await SocketClient.connect(
            config,
            None,
            self.post_reconnection,
            None,
            # TODO - waiting for async handling
            # self.post_connection,
            # self.post_reconnection,
            # self.post_disconnection,
        )
        self._log.debug("Running post connect")
        await self.post_connection()

        self.is_connected = True
        self._log.info("Connected.")

    async def disconnect(self):
        self._log.info("Disconnecting .. ")
        self.disconnecting = True
        self._client.disconnect()

        self._log.debug("Running post disconnect")
        await self.post_disconnection()

        self.is_connected = False
        self._log.info("Disconnected.")

    async def post_connection(self) -> None:
        """
        Actions to be performed post connection.
        """

    def post_reconnection(self) -> None:
        """
        Actions to be performed post connection.
        """

    async def post_disconnection(self) -> None:
        """
        Actions to be performed post disconnection.
        """

    async def send(self, message: bytes):
        self._log.debug(f"[SEND] {message.decode()}")
        if self._client is None:
            raise RuntimeError("Cannot send message: no client.")
        await self._client.send(message)
        self._log.debug("[SENT]")

    def auth_message(self):
        return {
            "op": "authentication",
            "id": self.unique_id,
            "appKey": self._http_client.app_key,
            "session": self._http_client.session_token,
        }


class BetfairOrderStreamClient(BetfairStreamClient):
    """
    Provides an order stream client for `Betfair`.
    """

    def __init__(
        self,
        http_client: BetfairHttpClient,
        message_handler: Callable[[bytes], None],
        partition_matched_by_strategy_ref: bool = True,
        include_overall_position: str | None = None,
        customer_strategy_refs: str | None = None,
        **kwargs,
    ) -> None:
        super().__init__(
            http_client=http_client,
            message_handler=message_handler,
            **kwargs,
        )
        self.order_filter = {
            "includeOverallPosition": include_overall_position,
            "customerStrategyRefs": customer_strategy_refs,
            "partitionMatchedByStrategyRef": partition_matched_by_strategy_ref,
        }

    async def post_connection(self):
        await super().post_connection()
        subscribe_msg = {
            "op": "orderSubscription",
            "id": self.unique_id,
            "orderFilter": self.order_filter,
            "initialClk": None,
            "clk": None,
        }
        await self.send(msgspec.json.encode(self.auth_message()))
        await self.send(msgspec.json.encode(subscribe_msg))

    def post_reconnection(self):
        super().post_reconnection()
        self._loop.create_task(self.post_connection())


class BetfairMarketStreamClient(BetfairStreamClient):
    """
    Provides a `Betfair` market stream client.
    """

    def __init__(
        self,
        http_client: BetfairHttpClient,
        message_handler: Callable,
        **kwargs,
    ):
        super().__init__(
            http_client=http_client,
            message_handler=message_handler,
            **kwargs,
        )

    # TODO - Add support for initial_clk/clk reconnection
    async def send_subscription_message(
        self,
        market_ids: list | None = None,
        betting_types: list | None = None,
        event_type_ids: list | None = None,
        event_ids: list | None = None,
        turn_in_play_enabled: bool | None = None,
        market_types: list | None = None,
        venues: list | None = None,
        country_codes: list | None = None,
        race_types: list | None = None,
        initial_clk: str | None = None,
        clk: str | None = None,
        conflate_ms: int | None = None,
        heartbeat_ms: int | None = None,
        segmentation_enabled: bool = True,
        subscribe_book_updates=True,
        subscribe_trade_updates=True,
        subscribe_market_definitions=True,
        subscribe_ticker=True,
        subscribe_bsp_updates=True,
        subscribe_bsp_projected=True,
    ):
        filters = (
            market_ids,
            betting_types,
            event_type_ids,
            event_ids,
            turn_in_play_enabled,
            market_types,
            venues,
            country_codes,
            race_types,
        )
        assert any(filters), "Must pass at least one filter"
        assert any(
            (subscribe_book_updates, subscribe_trade_updates),
        ), "Must subscribe to either book updates or trades"
        if market_ids is not None:
            # TODO - Log a warning about inefficiencies of specific market ids - Won't receive any updates for new
            #  markets that fit criteria like when using event type / market type etc
            # logging.warning()
            pass
        market_filter = {
            "marketIds": market_ids,
            "bettingTypes": betting_types,
            "eventTypeIds": event_type_ids,
            "eventIds": event_ids,
            "turnInPlayEnabled": turn_in_play_enabled,
            "marketTypes": market_types,
            "venues": venues,
            "countryCodes": country_codes,
            "raceTypes": race_types,
        }
        data_fields = []
        if subscribe_book_updates:
            data_fields.append("EX_ALL_OFFERS")
        if subscribe_trade_updates:
            data_fields.append("EX_TRADED")
        if subscribe_ticker:
            data_fields.extend(["EX_TRADED_VOL", "EX_LTP"])
        if subscribe_market_definitions:
            data_fields.append("EX_MARKET_DEF")
        if subscribe_bsp_updates:
            data_fields.append("SP_TRADED")
        if subscribe_bsp_projected:
            data_fields.append("SP_PROJECTED")

        message = {
            "op": "marketSubscription",
            "id": self.unique_id,
            "marketFilter": market_filter,
            "marketDataFilter": {"fields": data_fields},
            "initialClk": initial_clk,
            "clk": clk,
            "conflateMs": conflate_ms,
            "heartbeatMs": heartbeat_ms,
            "segmentationEnabled": segmentation_enabled,
        }
        await self.send(msgspec.json.encode(message))

    async def post_connection(self):
        await super().post_connection()
        await self.send(msgspec.json.encode(self.auth_message()))

    def post_reconnection(self):
        super().post_reconnection()
        self._loop.create_task(self.post_connection())
