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
from decimal import Decimal
from typing import Optional

import msgspec
import pandas as pd

from nautilus_trader.accounting.accounts.margin import MarginAccount
from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.functions import format_symbol
from nautilus_trader.adapters.binance.common.functions import parse_symbol
from nautilus_trader.adapters.binance.common.schemas import BinanceListenKey
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEventType
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesTimeInForce
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.adapters.binance.futures.http.market import BinanceFuturesMarketHttpAPI
from nautilus_trader.adapters.binance.futures.http.user import BinanceFuturesUserDataHttpAPI
from nautilus_trader.adapters.binance.futures.parsing.account import parse_account_balances_http
from nautilus_trader.adapters.binance.futures.parsing.account import parse_account_balances_ws
from nautilus_trader.adapters.binance.futures.parsing.account import parse_account_margins_http
from nautilus_trader.adapters.binance.futures.parsing.execution import binance_order_type
from nautilus_trader.adapters.binance.futures.parsing.execution import parse_order_report_http
from nautilus_trader.adapters.binance.futures.parsing.execution import parse_order_type
from nautilus_trader.adapters.binance.futures.parsing.execution import parse_position_report_http
from nautilus_trader.adapters.binance.futures.parsing.execution import parse_time_in_force
from nautilus_trader.adapters.binance.futures.parsing.execution import parse_trade_report_http
from nautilus_trader.adapters.binance.futures.parsing.execution import parse_trigger_type
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.futures.rules import BINANCE_FUTURES_VALID_ORDER_TYPES
from nautilus_trader.adapters.binance.futures.rules import BINANCE_FUTURES_VALID_TIF
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountInfo
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountTrade
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesOrder
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesPositionRisk
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesAccountUpdateMsg
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesAccountUpdateWrapper
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesOrderData
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesOrderUpdateMsg
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesOrderUpdateWrapper
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesUserMsgWrapper
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceError
from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import secs_to_millis
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.c_enums.order_type import OrderTypeParser
from nautilus_trader.model.c_enums.trailing_offset_type import TrailingOffsetTypeParser
from nautilus_trader.model.c_enums.trigger_type import TriggerTypeParser
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OMSType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderSideParser
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForceParser
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_market import TrailingStopMarketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus


class BinanceFuturesExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the `Binance Futures` exchange.

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
    logger : Logger
        The logger for the client.
    instrument_provider : BinanceFuturesInstrumentProvider
        The instrument provider.
    account_type : BinanceAccountType
        The account type for the client.
    base_url_ws : str, optional
        The base URL for the WebSocket client.
    clock_sync_interval_secs : int, default 900
        The interval (seconds) between syncing the Nautilus clock with the Binance server(s) clock.
        If zero, then will *not* perform syncing.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: BinanceFuturesInstrumentProvider,
        account_type: BinanceAccountType = BinanceAccountType.FUTURES_USDT,
        base_url_ws: Optional[str] = None,
        clock_sync_interval_secs: int = 900,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(BINANCE_VENUE.value),
            venue=BINANCE_VENUE,
            oms_type=OMSType.HEDGING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._binance_account_type = account_type
        self._log.info(f"Account type: {self._binance_account_type.value}.", LogColor.BLUE)

        self._set_account_id(AccountId(f"{BINANCE_VENUE.value}-futures-master"))

        # Clock sync
        self._clock_sync_interval_secs = clock_sync_interval_secs

        # Tasks
        self._task_clock_sync: Optional[asyncio.Task] = None

        # HTTP API
        self._http_client = client
        self._http_account = BinanceFuturesAccountHttpAPI(client=client, account_type=account_type)
        self._http_market = BinanceFuturesMarketHttpAPI(client=client, account_type=account_type)
        self._http_user = BinanceFuturesUserDataHttpAPI(client=client, account_type=account_type)

        # Listen keys
        self._ping_listen_keys_interval: int = 60 * 5  # Once every 5 mins (hardcode)
        self._ping_listen_keys_task: Optional[asyncio.Task] = None
        self._listen_key: Optional[str] = None

        # WebSocket API
        self._ws_client = BinanceWebSocketClient(
            loop=loop,
            clock=clock,
            logger=logger,
            handler=self._handle_user_ws_message,
            base_url=base_url_ws,
        )

        # Hot caches
        self._instrument_ids: dict[str, InstrumentId] = {}

        self._log.info(f"Base URL HTTP {self._http_client.base_url}.", LogColor.BLUE)
        self._log.info(f"Base URL WebSocket {base_url_ws}.", LogColor.BLUE)

    async def _connect(self) -> None:
        # Connect HTTP client
        if not self._http_client.connected:
            await self._http_client.connect()

        await self._instrument_provider.initialize()

        # Authenticate API key and update account(s)
        account_info: BinanceFuturesAccountInfo = await self._http_account.account(recv_window=5000)
        self._authenticate_api_key(account_info=account_info)

        binance_positions: list[BinanceFuturesPositionRisk]
        binance_positions = await self._http_account.get_position_risk()
        await self._update_account_state(
            account_info=account_info,
            position_risks=binance_positions,
        )

        # Get listen keys
        msg: BinanceListenKey = await self._http_user.create_listen_key()

        self._listen_key = msg.listenKey
        self._log.info(f"Listen key {self._listen_key}")
        self._ping_listen_keys_task = self._loop.create_task(self._ping_listen_keys())

        # Setup clock sync
        if self._clock_sync_interval_secs > 0:
            self._task_clock_sync = self._loop.create_task(self._sync_clock_with_binance_server())

        # Connect WebSocket client
        self._ws_client.subscribe(key=self._listen_key)
        await self._ws_client.connect()

    def _authenticate_api_key(self, account_info: BinanceFuturesAccountInfo) -> None:
        if account_info.canTrade:
            self._log.info("Binance API key authenticated.", LogColor.GREEN)
            self._log.info(f"API key {self._http_client.api_key} has trading permissions.")
        else:
            self._log.error("Binance API key does not have trading permissions.")

    async def _update_account_state(
        self,
        account_info: BinanceFuturesAccountInfo,
        position_risks: list[BinanceFuturesPositionRisk],
    ) -> None:
        self.generate_account_state(
            balances=parse_account_balances_http(assets=account_info.assets),
            margins=parse_account_margins_http(assets=account_info.assets),
            reported=True,
            ts_event=millis_to_nanos(account_info.updateTime),
        )
        while self.get_account() is None:
            await asyncio.sleep(0.1)

        account: MarginAccount = self.get_account()

        for position in position_risks:
            instrument_id: InstrumentId = self._get_cached_instrument_id(position.symbol)
            leverage = Decimal(position.leverage)
            account.set_leverage(instrument_id, leverage)
            self._log.debug(f"Set leverage {position.symbol} {leverage}X")

    async def _ping_listen_keys(self) -> None:
        while True:
            self._log.debug(
                f"Scheduled `ping_listen_keys` to run in " f"{self._ping_listen_keys_interval}s.",
            )
            await asyncio.sleep(self._ping_listen_keys_interval)
            if self._listen_key:
                self._log.debug(f"Pinging WebSocket listen key {self._listen_key}...")
                await self._http_user.ping_listen_key(self._listen_key)

    async def _sync_clock_with_binance_server(self) -> None:
        while True:
            # self._log.info(
            #     f"Syncing Nautilus clock with Binance server...",
            # )
            response: dict[str, int] = await self._http_market.time()
            server_time: int = response["serverTime"]
            self._log.info(f"Binance server time {server_time} UNIX (ms).")

            nautilus_time = self._clock.timestamp_ms()
            self._log.info(f"Nautilus clock time {nautilus_time} UNIX (ms).")

            # offset_ns = millis_to_nanos(nautilus_time - server_time)
            # self._log.info(f"Setting Nautilus clock offset {offset_ns} (ns).")
            # self._clock.set_offset(offset_ns)

            await asyncio.sleep(self._clock_sync_interval_secs)

    async def _disconnect(self) -> None:
        # Cancel tasks
        if self._ping_listen_keys_task:
            self._log.debug("Canceling `ping_listen_keys` task...")
            self._ping_listen_keys_task.cancel()

        if self._task_clock_sync:
            self._log.debug("Canceling `task_clock_sync` task...")
            self._task_clock_sync.cancel()

        # Disconnect WebSocket clients
        if self._ws_client.is_connected:
            await self._ws_client.disconnect()

        # Disconnect HTTP client
        if self._http_client.connected:
            await self._http_client.disconnect()

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: Optional[ClientOrderId] = None,
        venue_order_id: Optional[VenueOrderId] = None,
    ) -> Optional[OrderStatusReport]:
        PyCondition.false(
            client_order_id is None and venue_order_id is None,
            "both `client_order_id` and `venue_order_id` were `None`",
        )

        self._log.info(
            f"Generating OrderStatusReport for "
            f"{repr(client_order_id) if client_order_id else ''} "
            f"{repr(venue_order_id) if venue_order_id else ''}...",
        )

        try:
            binance_order: Optional[BinanceFuturesOrder]
            if venue_order_id:
                binance_order = await self._http_account.get_order(
                    symbol=instrument_id.symbol.value,
                    order_id=venue_order_id.value,
                )
            else:
                binance_order = await self._http_account.get_order(
                    symbol=instrument_id.symbol.value,
                    orig_client_order_id=client_order_id.value
                    if client_order_id is not None
                    else None,
                )
        except BinanceError as e:
            self._log.error(
                f"Cannot generate order status report for {repr(client_order_id)}: {e.message}",
            )
            return None

        if not binance_order:
            return None

        report: OrderStatusReport = parse_order_report_http(
            account_id=self.account_id,
            instrument_id=self._get_cached_instrument_id(binance_order.symbol),
            data=binance_order,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        self._log.debug(f"Received {report}.")
        return report

    async def generate_order_status_reports(  # noqa (C901 too complex)
        self,
        instrument_id: InstrumentId = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        self._log.info(f"Generating OrderStatusReports for {self.id}...")

        # Check cache for all active symbols
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)

        active_symbols: set[str] = set()
        for o in open_orders:
            active_symbols.add(format_symbol(o.instrument_id.symbol.value))
        for p in open_positions:
            active_symbols.add(format_symbol(p.instrument_id.symbol.value))

        binance_orders: list[BinanceFuturesOrder] = []
        reports: dict[VenueOrderId, OrderStatusReport] = {}

        try:
            # Check Binance for all active positions
            binance_positions: list[BinanceFuturesPositionRisk]
            binance_positions = await self._http_account.get_position_risk()
            for data in binance_positions:
                if Decimal(data.positionAmt) == 0:
                    continue  # Flat position
                # Add active symbol
                active_symbols.add(data.symbol)

            # Check Binance for all open orders
            binance_open_orders: list[BinanceFuturesOrder]
            binance_open_orders = await self._http_account.get_open_orders(
                symbol=instrument_id.symbol.value if instrument_id is not None else None,
            )
            binance_orders.extend(binance_open_orders)
            # Add active symbol
            for data in binance_orders:
                active_symbols.add(data.symbol)

            # Check Binance for all orders for active symbols
            for symbol in active_symbols:
                response = await self._http_account.get_orders(
                    symbol=symbol,
                    start_time=secs_to_millis(start.timestamp()) if start is not None else None,
                    end_time=secs_to_millis(end.timestamp()) if end is not None else None,
                )
                binance_orders.extend(response)
        except BinanceError as e:
            self._log.exception(f"Cannot generate order status report: {e.message}", e)
            return []

        # Parse all Binance orders
        for data in binance_orders:
            report = parse_order_report_http(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(data.symbol),
                data=data,
                report_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._log.debug(f"Received {report}.")
            reports[report.venue_order_id] = report  # One report per order

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} OrderStatusReport{plural}.")

        return list(reports.values())

    async def generate_trade_reports(  # noqa (C901 too complex)
        self,
        instrument_id: InstrumentId = None,
        venue_order_id: VenueOrderId = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[TradeReport]:
        self._log.info(f"Generating TradeReports for {self.id}...")

        # Check cache for all active symbols
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)

        active_symbols: set[str] = set()
        for o in open_orders:
            active_symbols.add(format_symbol(o.instrument_id.symbol.value))
        for p in open_positions:
            active_symbols.add(format_symbol(p.instrument_id.symbol.value))

        binance_trades: list[BinanceFuturesAccountTrade] = []
        reports: list[TradeReport] = []

        try:
            # Check Binance for all active positions
            binance_positions: list[BinanceFuturesPositionRisk]
            binance_positions = await self._http_account.get_position_risk()
            for data in binance_positions:
                if Decimal(data.positionAmt) == 0:
                    continue  # Flat position
                # Add active symbol
                active_symbols.add(data.symbol)

            # Check Binance for trades on all active symbols
            for symbol in active_symbols:
                symbol_trades = await self._http_account.get_account_trades(
                    symbol=symbol,
                    start_time=secs_to_millis(start.timestamp()) if start is not None else None,
                    end_time=secs_to_millis(end.timestamp()) if end is not None else None,
                )
                binance_trades.extend(symbol_trades)
        except BinanceError as e:
            self._log.exception(f"Cannot generate trade report: {e.message}", e)
            return []

        # Parse all Binance trades
        for data in binance_trades:
            report = parse_trade_report_http(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(data.symbol),
                data=data,
                report_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._log.debug(f"Received {report}.")
            reports.append(report)

        # Confirm sorting in ascending order
        reports = sorted(reports, key=lambda x: x.trade_id)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} TradeReport{plural}.")

        return reports

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[PositionStatusReport]:
        self._log.info(f"Generating PositionStatusReports for {self.id}...")

        reports: list[PositionStatusReport] = []

        try:
            # Check Binance for all active positions
            binance_positions: list[BinanceFuturesPositionRisk]
            binance_positions = await self._http_account.get_position_risk()
        except BinanceError as e:
            self._log.exception(f"Cannot generate position status report: {e.message}", e)
            return []

        # Parse all Binance positions
        for data in binance_positions:
            if Decimal(data.positionAmt) == 0:
                continue  # Flat position

            report: PositionStatusReport = parse_position_report_http(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(data.symbol),
                data=data,
                report_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
            )

            self._log.debug(f"Received {report}.")
            reports.append(report)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} PositionStatusReport{plural}.")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:  # noqa (too complex)
        order: Order = command.order

        # Check order type valid
        if order.order_type not in BINANCE_FUTURES_VALID_ORDER_TYPES:
            self._log.error(
                f"Cannot submit order: {OrderTypeParser.to_str_py(order.order_type)} "
                f"orders not supported by the Binance exchange for FUTURES accounts. "
                f"Use any of {[OrderTypeParser.to_str_py(t) for t in BINANCE_FUTURES_VALID_ORDER_TYPES]}",
            )
            return

        # Check time in force valid
        if order.time_in_force not in BINANCE_FUTURES_VALID_TIF:
            self._log.error(
                f"Cannot submit order: "
                f"{TimeInForceParser.to_str_py(order.time_in_force)} "
                f"not supported by the exchange. Use any of {BINANCE_FUTURES_VALID_TIF}.",
            )
            return

        # Check post-only
        if order.is_post_only and order.order_type != OrderType.LIMIT:
            self._log.error(
                f"Cannot submit order: {OrderTypeParser.to_str_py(order.order_type)} `post_only` order. "
                "Only LIMIT `post_only` orders supported by the Binance exchange for FUTURES accounts.",
            )
            return

        # Generate event here to ensure correct ordering of events
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        try:
            if order.order_type == OrderType.MARKET:
                await self._submit_market_order(order)
            elif order.order_type == OrderType.LIMIT:
                await self._submit_limit_order(order)
            elif order.order_type in (OrderType.STOP_MARKET, OrderType.MARKET_IF_TOUCHED):
                await self._submit_stop_market_order(order)
            elif order.order_type in (OrderType.STOP_LIMIT, OrderType.LIMIT_IF_TOUCHED):
                await self._submit_stop_limit_order(order)
            elif order.order_type == OrderType.TRAILING_STOP_MARKET:
                await self._submit_trailing_stop_market_order(order)
        except BinanceError as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=e.message,
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_market_order(self, order: MarketOrder) -> None:
        await self._http_account.new_order(
            symbol=format_symbol(order.instrument_id.symbol.value),
            side=OrderSideParser.to_str_py(order.side),
            type="MARKET",
            quantity=str(order.quantity),
            new_client_order_id=order.client_order_id.value,
            recv_window=5000,
        )

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        time_in_force = TimeInForceParser.to_str_py(order.time_in_force)
        if order.is_post_only:
            time_in_force = "GTX"

        await self._http_account.new_order(
            symbol=format_symbol(order.instrument_id.symbol.value),
            side=OrderSideParser.to_str_py(order.side),
            type=binance_order_type(order).value,
            time_in_force=time_in_force,
            quantity=str(order.quantity),
            price=str(order.price),
            reduce_only=order.is_reduce_only,  # Cannot be sent with Hedge-Mode or closePosition
            new_client_order_id=order.client_order_id.value,
            recv_window=5000,
        )

    async def _submit_stop_market_order(self, order: StopMarketOrder) -> None:
        if order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK:
            working_type = "MARK_PRICE"
        else:
            self._log.error(
                f"Cannot submit order: invalid `order.trigger_type`, was "
                f"{TriggerTypeParser.to_str_py(order.trigger_price)}. {order}",
            )
            return

        await self._http_account.new_order(
            symbol=format_symbol(order.instrument_id.symbol.value),
            side=OrderSideParser.to_str_py(order.side),
            type=binance_order_type(order).value,
            time_in_force=TimeInForceParser.to_str_py(order.time_in_force),
            quantity=str(order.quantity),
            stop_price=str(order.trigger_price),
            working_type=working_type,
            reduce_only=order.is_reduce_only,  # Cannot be sent with Hedge-Mode or closePosition
            new_client_order_id=order.client_order_id.value,
            recv_window=5000,
        )

    async def _submit_stop_limit_order(self, order: StopMarketOrder) -> None:
        if order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK:
            working_type = "MARK_PRICE"
        else:
            self._log.error(
                f"Cannot submit order: invalid `order.trigger_type`, was "
                f"{TriggerTypeParser.to_str_py(order.trigger_price)}. {order}",
            )
            return

        await self._http_account.new_order(
            symbol=format_symbol(order.instrument_id.symbol.value),
            side=OrderSideParser.to_str_py(order.side),
            type=binance_order_type(order).value,
            time_in_force=TimeInForceParser.to_str_py(order.time_in_force),
            quantity=str(order.quantity),
            price=str(order.price),
            stop_price=str(order.trigger_price),
            working_type=working_type,
            reduce_only=order.is_reduce_only,  # Cannot be sent with Hedge-Mode or closePosition
            new_client_order_id=order.client_order_id.value,
            recv_window=5000,
        )

    async def _submit_trailing_stop_market_order(self, order: TrailingStopMarketOrder) -> None:
        if order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK:
            working_type = "MARK_PRICE"
        else:
            self._log.error(
                f"Cannot submit order: invalid `order.trigger_type`, was "
                f"{TriggerTypeParser.to_str_py(order.trigger_price)}. {order}",
            )
            return

        if order.trailing_offset_type not in (
            TrailingOffsetType.DEFAULT,
            TrailingOffsetType.BASIS_POINTS,
        ):
            self._log.error(
                f"Cannot submit order: invalid `order.trailing_offset_type`, was "
                f"{TrailingOffsetTypeParser.to_str_py(order.trailing_offset_type)} (use `BASIS_POINTS`). "
                f"{order}",
            )
            return

        # Ensure activation price
        activation_price: Optional[Price] = order.trigger_price
        if not activation_price:
            quote = self._cache.quote_tick(order.instrument_id)
            trade = self._cache.trade_tick(order.instrument_id)
            if quote:
                if order.side == OrderSide.BUY:
                    activation_price = quote.ask
                elif order.side == OrderSide.SELL:
                    activation_price = quote.bid
            elif trade:
                activation_price = trade.price
            else:
                self._log.error(
                    "Cannot submit order: no trigger price specified for Binance activation price "
                    f"and could not find quotes or trades for {order.instrument_id}",
                )

        await self._http_account.new_order(
            symbol=format_symbol(order.instrument_id.symbol.value),
            side=OrderSideParser.to_str_py(order.side),
            type=binance_order_type(order).value,
            time_in_force=TimeInForceParser.to_str_py(order.time_in_force),
            quantity=str(order.quantity),
            activation_price=str(activation_price),
            callback_rate=str(order.trailing_offset / 100),
            working_type=working_type,
            reduce_only=order.is_reduce_only,  # Cannot be sent with Hedge-Mode or closePosition
            new_client_order_id=order.client_order_id.value,
            recv_window=5000,
        )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        for order in command.order_list.orders:
            if order.linked_order_ids:  # TODO(cs): Implement
                self._log.warning(f"Cannot yet handle OCO conditional orders, {order}.")
            await self._submit_order(order)

    async def _modify_order(self, command: ModifyOrder) -> None:
        self._log.error(  # pragma: no cover
            "Cannot modify order: Not supported by the exchange.",  # pragma: no cover
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        self.generate_order_pending_cancel(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        try:
            if command.venue_order_id is not None:
                await self._http_account.cancel_order(
                    symbol=format_symbol(command.instrument_id.symbol.value),
                    order_id=command.venue_order_id.value,
                )
            else:
                await self._http_account.cancel_order(
                    symbol=format_symbol(command.instrument_id.symbol.value),
                    orig_client_order_id=command.client_order_id.value,
                )
        except BinanceError as e:
            self._log.exception(
                f"Cannot cancel order "
                f"ClientOrderId({command.client_order_id}), "
                f"VenueOrderId{command.venue_order_id}: "
                f"{e.message}",
                e,
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        # Cancel all in-flight orders
        inflight_orders = self._cache.orders_inflight(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )
        for order in inflight_orders:
            self.generate_order_pending_cancel(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

        # Cancel all open orders
        open_orders = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )
        for order in open_orders:
            self.generate_order_pending_cancel(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

        try:
            await self._http_account.cancel_open_orders(
                symbol=format_symbol(command.instrument_id.symbol.value),
            )
        except BinanceError as e:
            self._log.exception(f"Cannot cancel open orders: {e.message}", e)

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        nautilus_symbol: str = parse_symbol(symbol, account_type=self._binance_account_type)
        instrument_id: Optional[InstrumentId] = self._instrument_ids.get(nautilus_symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(nautilus_symbol), BINANCE_VENUE)
            self._instrument_ids[nautilus_symbol] = instrument_id
        return instrument_id

    def _handle_user_ws_message(self, raw: bytes) -> None:
        # TODO(cs): Uncomment for development
        # self._log.info(str(json.dumps(msgspec.json.decode(raw), indent=4)), color=LogColor.MAGENTA)

        wrapper = msgspec.json.decode(raw, type=BinanceFuturesUserMsgWrapper)

        try:
            if wrapper.data.e == BinanceFuturesEventType.ACCOUNT_UPDATE:
                msg = msgspec.json.decode(raw, type=BinanceFuturesAccountUpdateWrapper)
                self._handle_account_update(msg.data)
            elif wrapper.data.e == BinanceFuturesEventType.ORDER_TRADE_UPDATE:
                msg = msgspec.json.decode(raw, type=BinanceFuturesOrderUpdateWrapper)
                self._handle_order_trade_update(msg.data)
            elif wrapper.data.e == BinanceFuturesEventType.MARGIN_CALL:
                self._log.warning("MARGIN CALL received.")  # Implement
            elif wrapper.data.e == BinanceFuturesEventType.ACCOUNT_CONFIG_UPDATE:
                self._log.info("Account config updated.", LogColor.BLUE)  # Implement
            elif wrapper.data.e == BinanceFuturesEventType.LISTEN_KEY_EXPIRED:
                self._log.warning("Listen key expired.")  # Implement
        except Exception as e:
            self._log.exception(f"Error on handling {repr(raw)}", e)

    def _handle_account_update(self, msg: BinanceFuturesAccountUpdateMsg) -> None:
        self.generate_account_state(
            balances=parse_account_balances_ws(raw_balances=msg.a.B),
            margins=[],
            reported=True,
            ts_event=millis_to_nanos(msg.T),
        )

    def _handle_order_trade_update(self, msg: BinanceFuturesOrderUpdateMsg) -> None:
        data: BinanceFuturesOrderData = msg.o
        instrument_id: InstrumentId = self._get_cached_instrument_id(data.s)
        client_order_id = ClientOrderId(data.c) if data.c != "" else None
        venue_order_id = VenueOrderId(str(data.i))
        ts_event = millis_to_nanos(msg.T)

        # Fetch strategy ID
        strategy_id: StrategyId = self._cache.strategy_id_for_order(client_order_id)
        if strategy_id is None:
            if strategy_id is None:
                self._generate_external_order_report(
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    msg.o,
                    ts_event,
                )
                return

        if data.x == BinanceExecutionType.NEW:
            self.generate_order_accepted(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        elif data.x == BinanceExecutionType.TRADE:
            instrument: Instrument = self._instrument_provider.find(instrument_id=instrument_id)

            # Determine commission
            if data.N is not None:
                commission = Money.from_str(f"{data.n} {data.N}")
            else:
                # Commission in margin collateral currency
                commission = Money(0, instrument.quote_currency)

            self.generate_order_filled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                venue_position_id=PositionId(f"{instrument_id}-{data.ps.value}"),
                trade_id=TradeId(str(data.t)),
                order_side=OrderSide.BUY if data.S == BinanceOrderSide.BUY else OrderSide.SELL,
                order_type=parse_order_type(data.o),
                last_qty=Quantity.from_str(data.l),
                last_px=Price.from_str(data.L),
                quote_currency=instrument.quote_currency,
                commission=commission,
                liquidity_side=LiquiditySide.MAKER if data.m else LiquiditySide.TAKER,
                ts_event=ts_event,
            )
        elif data.x == BinanceExecutionType.CANCELED:
            self.generate_order_canceled(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        elif data.x == BinanceExecutionType.EXPIRED:
            self.generate_order_expired(
                strategy_id=strategy_id,
                instrument_id=instrument_id,
                client_order_id=client_order_id,
                venue_order_id=venue_order_id,
                ts_event=ts_event,
            )
        else:
            self._log.error(
                f"Cannot handle ORDER_TRADE_UPDATE: unrecognized type {data.x.value}",
            )

    def _generate_external_order_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        data: BinanceFuturesOrderData,
        ts_event: int,
    ) -> None:
        report = OrderStatusReport(
            account_id=self.account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            order_side=OrderSide.BUY if data.S == BinanceOrderSide.BUY else OrderSide.SELL,
            order_type=parse_order_type(data.o),
            time_in_force=parse_time_in_force(data.f),
            order_status=OrderStatus.ACCEPTED,
            price=Price.from_str(data.p) if data.p is not None else None,
            trigger_price=Price.from_str(data.sp) if data.sp is not None else None,
            trigger_type=parse_trigger_type(data.wt),
            trailing_offset=Decimal(data.cr) * 100 if data.cr is not None else None,
            trailing_offset_type=TrailingOffsetType.BASIS_POINTS,
            quantity=Quantity.from_str(data.q),
            filled_qty=Quantity.from_str(data.z),
            avg_px=None,
            post_only=data.f == BinanceFuturesTimeInForce.GTX,
            reduce_only=data.R,
            report_id=UUID4(),
            ts_accepted=ts_event,
            ts_last=ts_event,
            ts_init=self._clock.timestamp_ns(),
        )

        self._send_order_status_report(report)
