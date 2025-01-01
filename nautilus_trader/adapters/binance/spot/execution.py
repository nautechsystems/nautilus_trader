# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.execution import BinanceCommonExecutionClient
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEnumParser
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEventType
from nautilus_trader.adapters.binance.spot.http.account import BinanceSpotAccountHttpAPI
from nautilus_trader.adapters.binance.spot.http.market import BinanceSpotMarketHttpAPI
from nautilus_trader.adapters.binance.spot.http.user import BinanceSpotUserDataHttpAPI
from nautilus_trader.adapters.binance.spot.providers import BinanceSpotInstrumentProvider
from nautilus_trader.adapters.binance.spot.schemas.account import BinanceSpotAccountInfo
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotAccountUpdateWrapper
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotOrderUpdateWrapper
from nautilus_trader.adapters.binance.spot.schemas.user import BinanceSpotUserMsgWrapper
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import order_type_to_str
from nautilus_trader.model.enums import time_in_force_to_str
from nautilus_trader.model.orders import Order


class BinanceSpotExecutionClient(BinanceCommonExecutionClient):
    """
    Provides an execution client for the Binance Spot/Margin exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BinanceHttpClient
        The binance HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BinanceSpotInstrumentProvider
        The instrument provider.
    base_url_ws : str
        The base URL for the WebSocket client.
    config : BinanceExecClientConfig
        The configuration for the client.
    account_type : BinanceAccountType, default 'SPOT'
        The account type for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BinanceSpotInstrumentProvider,
        base_url_ws: str,
        config: BinanceExecClientConfig,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
        name: str | None = None,
    ) -> None:
        PyCondition.is_true(
            account_type.is_spot_or_margin,
            "account_type was not SPOT, MARGIN or ISOLATED_MARGIN",
        )

        # Spot HTTP API
        self._spot_http_account = BinanceSpotAccountHttpAPI(client, clock, account_type)
        self._spot_http_market = BinanceSpotMarketHttpAPI(client, account_type)
        self._spot_http_user = BinanceSpotUserDataHttpAPI(client, account_type)

        # Spot enum parser
        self._spot_enum_parser = BinanceSpotEnumParser()

        # Instantiate common base class
        super().__init__(
            loop=loop,
            client=client,
            account=self._spot_http_account,
            market=self._spot_http_market,
            user=self._spot_http_user,
            enum_parser=self._spot_enum_parser,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_url_ws=base_url_ws,
            name=name,
            config=config,
        )

        # Register spot websocket user data event handlers
        self._spot_user_ws_handlers = {
            BinanceSpotEventType.outboundAccountPosition: self._handle_account_update,
            BinanceSpotEventType.executionReport: self._handle_execution_report,
            BinanceSpotEventType.listStatus: self._handle_list_status,
            BinanceSpotEventType.balanceUpdate: self._handle_balance_update,
        }

        # Websocket spot schema decoders
        self._decoder_spot_user_msg_wrapper = msgspec.json.Decoder(BinanceSpotUserMsgWrapper)
        self._decoder_spot_order_update_wrapper = msgspec.json.Decoder(
            BinanceSpotOrderUpdateWrapper,
        )
        self._decoder_spot_account_update_wrapper = msgspec.json.Decoder(
            BinanceSpotAccountUpdateWrapper,
        )

    async def _update_account_state(self) -> None:
        account_info: BinanceSpotAccountInfo = (
            await self._spot_http_account.query_spot_account_info(
                recv_window=str(5000),
            )
        )
        if account_info.canTrade:
            self._log.info("Binance API key authenticated.", LogColor.GREEN)
            self._log.info(f"API key {self._http_client.api_key} has trading permissions")
        else:
            self._log.error("Binance API key does not have trading permissions")
        self.generate_account_state(
            balances=account_info.parse_to_account_balances(),
            margins=[],
            reported=True,
            ts_event=millis_to_nanos(account_info.updateTime),
        )
        while self.get_account() is None:
            await asyncio.sleep(0.1)

    async def _init_dual_side_position(self) -> None:
        self._is_dual_side_position = False
        self._log.info(f"Dual side position: {self._is_dual_side_position}", LogColor.BLUE)

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def _get_binance_position_status_reports(
        self,
        symbol: str | None = None,
    ) -> list[PositionStatusReport]:
        # Never cash positions
        return []

    async def _get_binance_active_position_symbols(
        self,
        symbol: str | None = None,
    ) -> set[str]:
        # Never cash positions
        return set()

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def _check_order_validity(self, order: Order) -> None:
        # Check order type valid
        if order.order_type not in self._spot_enum_parser.spot_valid_order_types:
            self._log.error(
                f"Cannot submit order: {order_type_to_str(order.order_type)} "
                f"orders not supported by the Binance Spot/Margin exchange. "
                f"Use any of {[order_type_to_str(t) for t in self._spot_enum_parser.spot_valid_order_types]}",
            )
            return
        # Check time in force valid
        if order.time_in_force not in self._spot_enum_parser.spot_valid_time_in_force:
            self._log.error(
                f"Cannot submit order: "
                f"{time_in_force_to_str(order.time_in_force)} "
                f"not supported by the Binance Spot/Margin exchange. "
                f"Use any of {[time_in_force_to_str(t) for t in self._spot_enum_parser.spot_valid_time_in_force]}",
            )
            return
        # Check post-only
        if order.order_type == OrderType.STOP_LIMIT and order.is_post_only:
            self._log.error(
                "Cannot submit order: "
                "STOP_LIMIT `post_only` orders not supported by the Binance Spot/Margin exchange. "
                "This order may become a liquidity TAKER",
            )
            return

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        self._log.error(
            "Cannot batch cancel orders: not supported by the Binance Spot/Margin exchange",
        )

    # -- WEBSOCKET EVENT HANDLERS --------------------------------------------------------------------

    def _handle_user_ws_message(self, raw: bytes) -> None:
        try:
            wrapper = self._decoder_spot_user_msg_wrapper.decode(raw)
            self._spot_user_ws_handlers[wrapper.data.e](raw)
        except Exception as e:
            self._log.exception(f"Error on handling {raw!r}", e)

    def _handle_account_update(self, raw: bytes) -> None:
        account_msg = self._decoder_spot_account_update_wrapper.decode(raw)
        account_msg.data.handle_account_update(self)

    def _handle_execution_report(self, raw: bytes) -> None:
        order_msg = self._decoder_spot_order_update_wrapper.decode(raw)
        order_msg.data.handle_execution_report(self)

    def _handle_list_status(self, raw: bytes) -> None:
        self._log.warning("List status (OCO) received")  # Implement

    def _handle_balance_update(self, raw: bytes) -> None:
        self.create_task(self._update_account_state())
