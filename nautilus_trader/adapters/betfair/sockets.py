# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Callable, Optional

import msgspec

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.core.nautilus_pyo3.network import SocketClient
from nautilus_trader.core.nautilus_pyo3.network import SocketConfig


HOST = "stream-api.betfair.com"
# HOST = "stream-api-integration.betfair.com"
PORT = 443
CRLF = b"\r\n"
ENCODING = "utf-8"
UNIQUE_ID = itertools.count()


class BetfairStreamClient:
    """
    Provides a streaming client for `Betfair`.
    """

    def __init__(
        self,
        http_client: BetfairHttpClient,
        logger_adapter: LoggerAdapter,
        message_handler: Callable[[bytes], None],
        host: Optional[str] = HOST,
        port: Optional[int] = None,
        crlf: Optional[bytes] = None,
        encoding: Optional[str] = None,
    ) -> None:
        self._http_client = http_client
        self._log = logger_adapter
        self.handler = message_handler
        self.host = host or HOST
        self.port = port or PORT
        self.crlf = crlf or CRLF
        self.encoding = encoding or ENCODING
        self._client: Optional[SocketClient] = None
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

        self._client = await SocketClient.connect(
            SocketConfig(
                url=f"{self.host}:{self.port}",
                handler=self.handler,
                ssl=True,
                suffix=self.crlf,
            ),
        )

        self._log.debug("Running post connect")
        await self.post_connection()

        self.is_connected = True
        self._log.info("Connected.")

    async def post_connection(self):
        """
        Actions to be performed post connection.
        """

    async def disconnect(self):
        self._log.info("Disconnecting .. ")
        self.disconnecting = True
        self._client.disconnect()
        await self.post_disconnection()
        self.is_connected = False
        self._log.info("Disconnected.")

    async def post_disconnection(self) -> None:
        """
        Actions to be performed post disconnection.
        """

    async def send(self, message: bytes):
        self._log.debug(f"[SEND] {message.decode()}")
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
        logger: Logger,
        message_handler,
        partition_matched_by_strategy_ref: bool = True,
        include_overall_position: Optional[str] = None,
        customer_strategy_refs: Optional[str] = None,
        **kwargs,
    ):
        super().__init__(
            http_client=http_client,
            logger_adapter=LoggerAdapter("BetfairOrderStreamClient", logger),
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


class BetfairMarketStreamClient(BetfairStreamClient):
    """
    Provides a `Betfair` market stream client.
    """

    def __init__(
        self,
        http_client: BetfairHttpClient,
        logger: Logger,
        message_handler: Callable,
        **kwargs,
    ):
        super().__init__(
            http_client=http_client,
            logger_adapter=LoggerAdapter("BetfairMarketStreamClient", logger),
            message_handler=message_handler,
            **kwargs,
        )

    # TODO - Add support for initial_clk/clk reconnection
    async def send_subscription_message(
        self,
        market_ids: Optional[list] = None,
        betting_types: Optional[list] = None,
        event_type_ids: Optional[list] = None,
        event_ids: Optional[list] = None,
        turn_in_play_enabled: Optional[bool] = None,
        market_types: Optional[list] = None,
        venues: Optional[list] = None,
        country_codes: Optional[list] = None,
        race_types: Optional[list] = None,
        initial_clk: Optional[str] = None,
        clk: Optional[str] = None,
        conflate_ms: Optional[int] = None,
        heartbeat_ms: Optional[int] = None,
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
