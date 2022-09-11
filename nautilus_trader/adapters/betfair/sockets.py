# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Callable, Optional

import msgspec

from nautilus_trader.adapters.betfair.client.core import BetfairClient
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.network.socket import SocketClient


HOST = "stream-api.betfair.com"
# HOST = "stream-api-integration.betfair.com"
PORT = 443
CRLF = b"\r\n"
ENCODING = "utf-8"
_UNIQUE_ID = 0


class BetfairStreamClient(SocketClient):
    """
    Provides a streaming client for `Betfair`.
    """

    def __init__(
        self,
        client: BetfairClient,
        logger_adapter: LoggerAdapter,
        message_handler,
        loop: Optional[asyncio.AbstractEventLoop] = None,
        host: Optional[str] = None,
        port: Optional[int] = None,
        crlf: Optional[bytes] = None,
        encoding: Optional[str] = None,
    ):
        super().__init__(
            loop=loop or asyncio.get_event_loop(),
            logger=logger_adapter.get_logger(),
            host=host or HOST,
            port=port or PORT,
            handler=message_handler,
            crlf=crlf or CRLF,
            encoding=encoding or ENCODING,
        )
        self.client = client
        self.unique_id = self.new_unique_id()

    def connect(self):
        err = f"Must login to BetfairClient before calling connect on {self.__class__}"
        assert self.client.session_token, err
        return super().connect()

    def new_unique_id(self) -> int:
        global _UNIQUE_ID
        _UNIQUE_ID += 1
        return _UNIQUE_ID

    def auth_message(self):
        return {
            "op": "authentication",
            "id": self.unique_id,
            "appKey": self.client.app_key,
            "session": self.client.session_token,
        }

    async def send_dict(self, data):
        raw = msgspec.json.encode(data)
        await self.send(raw)


class BetfairOrderStreamClient(BetfairStreamClient):
    """
    Provides an order stream client for `Betfair`.
    """

    def __init__(
        self,
        client: BetfairClient,
        logger: Logger,
        message_handler,
        partition_matched_by_strategy_ref: bool = True,
        include_overall_position: Optional[str] = None,
        customer_strategy_refs: Optional[str] = None,
        **kwargs,
    ):
        super().__init__(
            client=client,
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
        subscribe_msg = {
            "op": "orderSubscription",
            "id": self.unique_id,
            "orderFilter": self.order_filter,
            "initialClk": None,
            "clk": None,
        }
        await self.send_dict(data=self.auth_message())
        await self.send_dict(data=subscribe_msg)


class BetfairMarketStreamClient(BetfairStreamClient):
    """
    Provides a `Betfair` market stream client.
    """

    def __init__(self, client: BetfairClient, logger: Logger, message_handler: Callable, **kwargs):
        self.subscription_message = None
        super().__init__(
            client=client,
            logger_adapter=LoggerAdapter("BetfairMarketStreamClient", logger),
            message_handler=message_handler,
            **kwargs,
        )

    # TODO - Add support for initial_clk/clk reconnection
    async def send_subscription_message(
        self,
        market_ids: list = None,
        betting_types: list = None,
        event_type_ids: list = None,
        event_ids: list = None,
        turn_in_play_enabled: bool = None,
        market_types: list = None,
        venues: list = None,
        country_codes: list = None,
        race_types: list = None,
        initial_clk: str = None,
        clk: str = None,
        conflate_ms: int = None,
        heartbeat_ms: int = None,
        segmentation_enabled: bool = True,
        subscribe_book_updates=True,
        subscribe_trade_updates=True,
        subscribe_market_definitions=True,
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
            (subscribe_book_updates, subscribe_trade_updates)
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
        if subscribe_market_definitions:
            data_fields.append("EX_MARKET_DEF")

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
        await self.send_dict(data=message)

    async def post_connection(self):
        await self.send_dict(data=self.auth_message())
