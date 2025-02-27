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
from asyncio import TaskGroup
from decimal import Decimal

import msgspec

from nautilus_trader.accounting.accounts.margin import MarginAccount
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceErrorCode
from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.execution import BinanceCommonExecutionClient
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEnumParser
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEventType
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.adapters.binance.futures.http.market import BinanceFuturesMarketHttpAPI
from nautilus_trader.adapters.binance.futures.http.user import BinanceFuturesUserDataHttpAPI
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountInfo
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesDualSidePosition
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesLeverage
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesPositionRisk
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesAccountUpdateWrapper
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesOrderUpdateWrapper
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesTradeLiteWrapper
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesUserMsgWrapper
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceError
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import order_type_to_str
from nautilus_trader.model.enums import time_in_force_to_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import Order


class BinanceFuturesExecutionClient(BinanceCommonExecutionClient):
    """
    Provides an execution client for the Binance Futures exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BinanceHttpClient
        The Binance HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BinanceFuturesInstrumentProvider
        The instrument provider.
    base_url_ws : str
        The base URL for the WebSocket client.
    config : BinanceExecClientConfig
        The configuration for the client.
    account_type : BinanceAccountType, default 'USDT_FUTURE'
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
        instrument_provider: BinanceFuturesInstrumentProvider,
        base_url_ws: str,
        config: BinanceExecClientConfig,
        account_type: BinanceAccountType = BinanceAccountType.USDT_FUTURE,
        name: str | None = None,
    ) -> None:
        PyCondition.is_true(
            account_type.is_futures,
            "account_type was not USDT_FUTURE or COIN_FUTURE",
        )

        # Futures HTTP API
        self._futures_http_account = BinanceFuturesAccountHttpAPI(client, clock, account_type)
        self._futures_http_market = BinanceFuturesMarketHttpAPI(client, account_type)
        self._futures_http_user = BinanceFuturesUserDataHttpAPI(client, account_type)

        # Futures enum parser
        self._futures_enum_parser = BinanceFuturesEnumParser()

        # Instantiate common base class
        super().__init__(
            loop=loop,
            client=client,
            account=self._futures_http_account,
            market=self._futures_http_market,
            user=self._futures_http_user,
            enum_parser=self._futures_enum_parser,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_url_ws=base_url_ws,
            name=name,
            config=config,
        )

        # Register additional futures websocket user data event handlers
        self._futures_user_ws_handlers = {
            BinanceFuturesEventType.ACCOUNT_UPDATE: self._handle_account_update,
            BinanceFuturesEventType.ORDER_TRADE_UPDATE: self._handle_order_trade_update,
            BinanceFuturesEventType.MARGIN_CALL: self._handle_margin_call,
            BinanceFuturesEventType.ACCOUNT_CONFIG_UPDATE: self._handle_account_config_update,
            BinanceFuturesEventType.LISTEN_KEY_EXPIRED: self._handle_listen_key_expired,
            BinanceFuturesEventType.TRADE_LITE: self._handle_trade_lite,
        }

        self._use_trade_lite = config.use_trade_lite
        if self._use_trade_lite:
            self._log.info("TRADE_LITE events will be used", LogColor.BLUE)

        self._leverages = config.futures_leverages

        # WebSocket futures schema decoders
        self._decoder_futures_user_msg_wrapper = msgspec.json.Decoder(BinanceFuturesUserMsgWrapper)
        self._decoder_futures_order_update_wrapper = msgspec.json.Decoder(
            BinanceFuturesOrderUpdateWrapper,
        )
        self._decoder_futures_account_update_wrapper = msgspec.json.Decoder(
            BinanceFuturesAccountUpdateWrapper,
        )
        self._decoder_futures_trade_lite_wrapper = msgspec.json.Decoder(
            BinanceFuturesTradeLiteWrapper,
        )

    async def _update_account_state(self) -> None:
        account_info: BinanceFuturesAccountInfo = (
            await self._futures_http_account.query_futures_account_info(recv_window=str(5000))
        )
        if account_info.canTrade:
            self._log.info("Binance API key authenticated", LogColor.GREEN)
            self._log.info(f"API key {self._http_client.api_key} has trading permissions")
        else:
            self._log.error("Binance API key does not have trading permissions")
        self.generate_account_state(
            balances=account_info.parse_to_account_balances(),
            margins=account_info.parse_to_margin_balances(),
            reported=True,
            ts_event=millis_to_nanos(account_info.updateTime),
        )
        while self.get_account() is None:
            await asyncio.sleep(0.1)

        if self._leverages:
            async with TaskGroup() as tg:
                tasks = [
                    tg.create_task(self._futures_http_account.set_leverage(symbol, leverage))
                    for symbol, leverage in self._leverages.items()
                ]
            for task in tasks:
                res: BinanceFuturesLeverage = task.result()
                self._log.info(f"Set default leverage {res.symbol} {res.leverage}X")

        account: MarginAccount = self.get_account()
        position_risks = await self._futures_http_account.query_futures_position_risk()
        for position in position_risks:
            instrument_id: InstrumentId = self._get_cached_instrument_id(position.symbol)
            leverage = Decimal(position.leverage)
            account.set_leverage(instrument_id, leverage)
            self._log.debug(f"Set leverage {position.symbol} {leverage}X")

    async def _init_dual_side_position(self) -> None:
        binance_futures_dual_side_position: BinanceFuturesDualSidePosition = (
            await self._futures_http_account.query_futures_hedge_mode()
        )
        # "true": Hedge Mode; "false": One-way Mode
        self._is_dual_side_position = binance_futures_dual_side_position.dualSidePosition
        if self._is_dual_side_position:
            PyCondition.is_false(
                self._use_reduce_only,
                "Cannot use `reduce_only` with Binance Hedge Mode",
            )
        self._log.info(f"Dual side position: {self._is_dual_side_position}", LogColor.BLUE)

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def _get_binance_position_status_reports(
        self,
        symbol: str | None = None,
    ) -> list[PositionStatusReport]:
        reports: list[PositionStatusReport] = []
        # Check Binance for all active positions
        binance_positions: list[BinanceFuturesPositionRisk]
        binance_positions = await self._futures_http_account.query_futures_position_risk(symbol)
        for position in binance_positions:
            if Decimal(position.positionAmt) == 0:
                continue  # Flat position
            report = position.parse_to_position_status_report(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(position.symbol),
                report_id=UUID4(),
                enum_parser=self._futures_enum_parser,
                ts_init=self._clock.timestamp_ns(),
            )
            self._log.debug(f"Received {report}")
            reports.append(report)
        return reports

    async def _get_binance_active_position_symbols(
        self,
        symbol: str | None = None,
    ) -> set[str]:
        # Check Binance for all active positions
        active_symbols: set[str] = set()
        binance_positions: list[BinanceFuturesPositionRisk]
        binance_positions = await self._futures_http_account.query_futures_position_risk(symbol)
        for position in binance_positions:
            if Decimal(position.positionAmt) == 0:
                continue  # Flat position
            # Add active symbol
            active_symbols.add(position.symbol)
        return active_symbols

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def _check_order_validity(self, order: Order) -> None:
        # Check order type valid
        if order.order_type not in self._futures_enum_parser.futures_valid_order_types:
            self._log.error(
                f"Cannot submit order: {order_type_to_str(order.order_type)} "
                f"orders not supported by the Binance exchange for FUTURES accounts. "
                f"Use any of {[order_type_to_str(t) for t in self._futures_enum_parser.futures_valid_order_types]}",
            )
            return
        # Check time in force valid
        if order.time_in_force not in self._futures_enum_parser.futures_valid_time_in_force:
            self._log.error(
                f"Cannot submit order: "
                f"{time_in_force_to_str(order.time_in_force)} "
                f"not supported by the exchange. "
                f"Use any of {[time_in_force_to_str(t) for t in self._futures_enum_parser.futures_valid_time_in_force]}",
            )
            return
        # Check post-only
        if order.is_post_only and order.order_type != OrderType.LIMIT:
            self._log.error(
                f"Cannot submit order: {order_type_to_str(order.order_type)} `post_only` order. "
                "Only LIMIT `post_only` orders supported by the Binance exchange for FUTURES accounts",
            )
            return

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        # TODO: Iterate batches of 10 order cancels, also validate order is not already closed
        try:
            await self._futures_http_account.cancel_multiple_orders(
                symbol=command.instrument_id.symbol.value,
                client_order_ids=[c.client_order_id.value for c in command.cancels],
            )
        except BinanceError as e:
            error_code = BinanceErrorCode(int(e.message["code"]))
            if error_code == BinanceErrorCode.CANCEL_REJECTED:
                self._log.warning(f"Cancel rejected: {e.message}")
            else:
                self._log.exception(
                    f"Cannot cancel multiple orders: {e.message}",
                    e,
                )

    # -- WEBSOCKET EVENT HANDLERS --------------------------------------------------------------------

    def _handle_user_ws_message(self, raw: bytes) -> None:
        try:
            wrapper = self._decoder_futures_user_msg_wrapper.decode(raw)
            if not wrapper.stream or not wrapper.data:
                return  # Control message response

            self._futures_user_ws_handlers[wrapper.data.e](raw)
        except Exception as e:
            self._log.exception(f"Error on handling {raw!r}", e)

    def _handle_account_update(self, raw: bytes) -> None:
        account_update = self._decoder_futures_account_update_wrapper.decode(raw)
        account_update.data.handle_account_update(self)

    def _handle_order_trade_update(self, raw: bytes) -> None:
        order_update = self._decoder_futures_order_update_wrapper.decode(raw)
        if not (self._use_trade_lite and order_update.data.o.x == BinanceExecutionType.TRADE):
            order_update.data.o.handle_order_trade_update(self)

    def _handle_margin_call(self, raw: bytes) -> None:
        self._log.warning("MARGIN CALL received")  # Implement

    def _handle_account_config_update(self, raw: bytes) -> None:
        self._log.info("Account config updated", LogColor.BLUE)  # Implement

    def _handle_listen_key_expired(self, raw: bytes) -> None:
        self._log.warning("Listen key expired")  # Implement

    def _handle_trade_lite(self, raw: bytes) -> None:
        trade_lite = self._decoder_futures_trade_lite_wrapper.decode(raw)
        if not self._use_trade_lite:
            self._log.debug(
                "TradeLite event received but not enabled in config",
            )
            return
        order_data = trade_lite.data.to_order_data()
        order_data.handle_order_trade_update(self)
