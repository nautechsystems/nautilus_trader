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
from typing import Optional

import pandas as pd

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnumParser
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.account import BinanceOrder
from nautilus_trader.adapters.binance.common.schemas.account import BinanceUserTrade
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.common.schemas.user import BinanceListenKey
from nautilus_trader.adapters.binance.http.account import BinanceAccountHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceError
from nautilus_trader.adapters.binance.http.market import BinanceMarketHttpAPI
from nautilus_trader.adapters.binance.http.user import BinanceUserDataHttpAPI
from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
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
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import trailing_offset_type_to_str
from nautilus_trader.model.enums import trigger_type_to_str
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.model.orders.stop_limit import StopLimitOrder
from nautilus_trader.model.orders.stop_market import StopMarketOrder
from nautilus_trader.model.orders.trailing_stop_market import TrailingStopMarketOrder
from nautilus_trader.model.position import Position
from nautilus_trader.msgbus.bus import MessageBus


class BinanceCommonExecutionClient(LiveExecutionClient):
    """
    Execution client providing common functionality for the `Binance` exchanges.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : BinanceHttpClient
        The binance HTTP client.
    account : BinanceAccountHttpAPI
        The binance Account HTTP API.
    market : BinanceMarketHttpAPI
        The binance Market HTTP API.
    user : BinanceUserHttpAPI
        The binance User HTTP API.
    enum_parser : BinanceEnumParser
        The parser for Binance enums.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    instrument_provider : BinanceSpotInstrumentProvider
        The instrument provider.
    account_type : BinanceAccountType
        The account type for the client.
    base_url_ws : str, optional
        The base URL for the WebSocket client.
    clock_sync_interval_secs : int, default 0
        The interval (seconds) between syncing the Nautilus clock with the Binance server(s) clock.
        If zero, then will *not* perform syncing.
    warn_gtd_to_gtc : bool, default True
        If log warning for GTD time in force transformed to GTC.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        account: BinanceAccountHttpAPI,
        market: BinanceMarketHttpAPI,
        user: BinanceUserDataHttpAPI,
        enum_parser: BinanceEnumParser,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InstrumentProvider,
        account_type: BinanceAccountType,
        base_url_ws: Optional[str] = None,
        clock_sync_interval_secs: int = 0,
        warn_gtd_to_gtc: bool = True,
    ):
        super().__init__(
            loop=loop,
            client_id=ClientId(BINANCE_VENUE.value),
            venue=BINANCE_VENUE,
            oms_type=OmsType.HEDGING if account_type.is_futures else OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH if account_type.is_spot else AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self._binance_account_type = account_type
        self._warn_gtd_to_gtc = warn_gtd_to_gtc
        self._log.info(f"Account type: {self._binance_account_type.value}.", LogColor.BLUE)

        self._set_account_id(AccountId(f"{BINANCE_VENUE.value}-spot-master"))

        # Clock sync
        self._clock_sync_interval_secs = clock_sync_interval_secs

        # Tasks
        self._task_clock_sync: Optional[asyncio.Task] = None

        # Enum parser
        self._enum_parser = enum_parser

        # Http API
        self._http_client = client
        self._http_account = account
        self._http_market = market
        self._http_user = user

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

        # Order submission method hashmap
        self._submit_order_method = {
            OrderType.MARKET: self._submit_market_order,
            OrderType.LIMIT: self._submit_limit_order,
            OrderType.STOP_LIMIT: self._submit_stop_limit_order,
            OrderType.LIMIT_IF_TOUCHED: self._submit_stop_limit_order,
            OrderType.STOP_MARKET: self._submit_stop_market_order,
            OrderType.MARKET_IF_TOUCHED: self._submit_stop_market_order,
            OrderType.TRAILING_STOP_MARKET: self._submit_trailing_stop_market_order,
        }

        self._log.info(f"Base URL HTTP {self._http_client.base_url}.", LogColor.BLUE)
        self._log.info(f"Base URL WebSocket {base_url_ws}.", LogColor.BLUE)

    async def _connect(self) -> None:
        # Connect HTTP client
        if not self._http_client.connected:
            await self._http_client.connect()
        try:
            # Initialize instrument provider
            await self._instrument_provider.initialize()
            # Authenticate API key and update account(s)
            await self._update_account_state()
            # Get listen keys
            response: BinanceListenKey = await self._http_user.create_listen_key()
        except BinanceError as e:
            self._log.exception(f"Error on connect: {e.message}", e)
            return
        self._listen_key = response.listenKey
        self._log.info(f"Listen key {self._listen_key}")
        self._ping_listen_keys_task = self.create_task(self._ping_listen_keys())

        # Setup clock sync
        if self._clock_sync_interval_secs > 0:
            self._task_clock_sync = self.create_task(self._sync_clock_with_binance_server())

        # Connect WebSocket client
        self._ws_client.subscribe(key=self._listen_key)
        await self._ws_client.connect()

    async def _update_account_state(self) -> None:
        # Replace method in child class
        raise NotImplementedError

    async def _ping_listen_keys(self) -> None:
        try:
            while True:
                self._log.debug(
                    f"Scheduled `ping_listen_keys` to run in "
                    f"{self._ping_listen_keys_interval}s.",
                )
                await asyncio.sleep(self._ping_listen_keys_interval)
                if self._listen_key:
                    self._log.debug(f"Pinging WebSocket listen key {self._listen_key}...")
                    await self._http_user.keepalive_listen_key(listen_key=self._listen_key)
        except asyncio.CancelledError:
            self._log.debug("`ping_listen_keys` task was canceled.")

    async def _sync_clock_with_binance_server(self) -> None:
        try:
            while True:
                # self._log.info(
                #     f"Syncing Nautilus clock with Binance server...",
                # )
                server_time = await self._http_market.request_server_time()
                self._log.info(f"Binance server time {server_time} UNIX (ms).")

                nautilus_time = self._clock.timestamp_ms()
                self._log.info(f"Nautilus clock time {nautilus_time} UNIX (ms).")

                # offset_ns = millis_to_nanos(nautilus_time - server_time)
                # self._log.info(f"Setting Nautilus clock offset {offset_ns} (ns).")
                # self._clock.set_offset(offset_ns)

                await asyncio.sleep(self._clock_sync_interval_secs)
        except asyncio.CancelledError:
            self._log.debug("`sync_clock_with_binance_server` task was canceled.")

    async def _disconnect(self) -> None:
        # Cancel tasks
        if self._ping_listen_keys_task:
            self._log.debug("Canceling `ping_listen_keys` task...")
            self._ping_listen_keys_task.cancel()
            self._ping_listen_keys_task.done()

        if self._task_clock_sync:
            self._log.debug("Canceling `task_clock_sync` task...")
            self._task_clock_sync.cancel()
            self._task_clock_sync.done()

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
            if venue_order_id:
                binance_order = await self._http_account.query_order(
                    symbol=instrument_id.symbol.value,
                    order_id=venue_order_id.value,
                )
            else:
                binance_order = await self._http_account.query_order(
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

        report: OrderStatusReport = binance_order.parse_to_order_status_report(
            account_id=self.account_id,
            instrument_id=self._get_cached_instrument_id(binance_order.symbol),
            report_id=UUID4(),
            enum_parser=self._enum_parser,
            ts_init=self._clock.timestamp_ns(),
        )

        self._log.debug(f"Received {report}.")
        return report

    def _get_cache_active_symbols(self) -> list[str]:
        # Check cache for all active symbols
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)
        active_symbols: list[str] = []
        for o in open_orders:
            active_symbols.append(o.instrument_id.symbol.value)
        for p in open_positions:
            active_symbols.append(p.instrument_id.symbol.value)
        return active_symbols

    async def _get_binance_position_status_reports(
        self,
        symbol: Optional[str] = None,
    ) -> list[str]:
        # Implement in child class
        raise NotImplementedError

    async def _get_binance_active_position_symbols(
        self,
        symbol: Optional[str] = None,
    ) -> list[str]:
        # Implement in child class
        raise NotImplementedError

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        self._log.info(f"Generating OrderStatusReports for {self.id}...")

        try:
            # Check Binance for all order active symbols
            symbol = instrument_id.symbol.value if instrument_id is not None else None
            active_symbols = self._get_cache_active_symbols()
            active_symbols.extend(await self._get_binance_active_position_symbols(symbol))
            binance_open_orders = await self._http_account.query_open_orders(symbol)
            for order in binance_open_orders:
                active_symbols.append(order.symbol)
            # Get all orders for those active symbols
            binance_orders: list[BinanceOrder] = []
            for symbol in active_symbols:
                response = await self._http_account.query_all_orders(
                    symbol=symbol,
                    start_time=secs_to_millis(start.timestamp()) if start is not None else None,
                    end_time=secs_to_millis(end.timestamp()) if end is not None else None,
                )
                binance_orders.extend(response)
        except BinanceError as e:
            self._log.exception(f"Cannot generate OrderStatusReport: {e.message}", e)
            return []

        reports: list[OrderStatusReport] = []
        for order in binance_orders:
            # Apply filter (always report open orders regardless of start, end filter)
            # TODO(cs): Time filter is WIP
            # timestamp = pd.to_datetime(data["time"], utc=True)
            # if data["status"] not in ("NEW", "PARTIALLY_FILLED", "PENDING_CANCEL"):
            #     if start is not None and timestamp < start:
            #         continue
            #     if end is not None and timestamp > end:
            #         continue
            report = order.parse_to_order_status_report(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(order.symbol),
                report_id=UUID4(),
                enum_parser=self._enum_parser,
                ts_init=self._clock.timestamp_ns(),
            )
            self._log.debug(f"Received {reports}.")
            reports.append(report)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} OrderStatusReport{plural}.")

        return reports

    async def generate_trade_reports(
        self,
        instrument_id: InstrumentId = None,
        venue_order_id: VenueOrderId = None,
        start: Optional[pd.Timestamp] = None,
        end: Optional[pd.Timestamp] = None,
    ) -> list[TradeReport]:
        self._log.info(f"Generating TradeReports for {self.id}...")

        try:
            # Check Binance for all trades on active symbols
            symbol = instrument_id.symbol.value if instrument_id is not None else None
            active_symbols = self._get_cache_active_symbols()
            active_symbols.extend(await self._get_binance_active_position_symbols(symbol))
            binance_trades: list[BinanceUserTrade] = []
            for symbol in active_symbols:
                response = await self._http_account.query_user_trades(
                    symbol=symbol,
                    start_time=secs_to_millis(start.timestamp()) if start is not None else None,
                    end_time=secs_to_millis(end.timestamp()) if end is not None else None,
                )
                binance_trades.extend(response)
        except BinanceError as e:
            self._log.exception(f"Cannot generate TradeReport: {e.message}", e)
            return []

        # Parse all Binance trades
        reports: list[TradeReport] = []
        for trade in binance_trades:
            # Apply filter
            # TODO(cs): Time filter is WIP
            # timestamp = pd.to_datetime(data["time"], utc=True)
            # if start is not None and timestamp < start:
            #     continue
            # if end is not None and timestamp > end:
            #     continue
            if trade.symbol is None:
                self.log.warning(f"No symbol for trade {trade}.")
                continue
            report = trade.parse_to_trade_report(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(trade.symbol),
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

        try:
            symbol = instrument_id.symbol.value if instrument_id is not None else None
            reports = await self._get_binance_position_status_reports(symbol)
        except BinanceError as e:
            self._log.exception(f"Cannot generate PositionStatusReport: {e.message}", e)
            return []

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} PositionStatusReport{plural}.")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        order: Order = command.order

        # Check validity
        self._check_order_validity(order)
        self._log.debug(f"Submitting {order}.")

        # Generate event here to ensure correct ordering of events
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )
        try:
            await self._submit_order_method[order.order_type](order)
        except BinanceError as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=e.message,
                ts_event=self._clock.timestamp_ns(),
            )
        except KeyError:
            raise RuntimeError(f"unsupported order type, was {order.order_type}")

    def _check_order_validity(self, order: Order):
        # Implement in child class
        raise NotImplementedError

    async def _submit_market_order(self, order: MarketOrder) -> None:
        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            quantity=str(order.quantity),
            new_client_order_id=order.client_order_id.value,
            recv_window=str(5000),
        )

    async def _submit_limit_order(self, order: LimitOrder) -> None:
        time_in_force = self._enum_parser.parse_internal_time_in_force(order.time_in_force)
        if order.time_in_force == TimeInForce.GTD and time_in_force == BinanceTimeInForce.GTC:
            if self._warn_gtd_to_gtc:
                self._log.warning("Converted GTD `time_in_force` to GTC.")
        if order.is_post_only and self._binance_account_type.is_spot_or_margin:
            time_in_force = None
        elif order.is_post_only and self._binance_account_type.is_futures:
            time_in_force = BinanceTimeInForce.GTX

        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            time_in_force=time_in_force,
            quantity=str(order.quantity),
            price=str(order.price),
            iceberg_qty=str(order.display_qty) if order.display_qty is not None else None,
            reduce_only=str(order.is_reduce_only) if order.is_reduce_only is True else None,
            new_client_order_id=order.client_order_id.value,
            recv_window=str(5000),
        )

    async def _submit_stop_limit_order(self, order: StopLimitOrder) -> None:
        time_in_force = self._enum_parser.parse_internal_time_in_force(order.time_in_force)

        if self._binance_account_type.is_spot_or_margin:
            working_type = None
        elif order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST_TRADE):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK_PRICE:
            working_type = "MARK_PRICE"
        else:
            self._log.error(
                f"Cannot submit order: invalid `order.trigger_type`, was "
                f"{trigger_type_to_str(order.trigger_price)}. {order}",
            )
            return

        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            time_in_force=time_in_force,
            quantity=str(order.quantity),
            price=str(order.price),
            stop_price=str(order.trigger_price),
            working_type=working_type,
            iceberg_qty=str(order.display_qty) if order.display_qty is not None else None,
            reduce_only=str(order.is_reduce_only) if order.is_reduce_only is True else None,
            new_client_order_id=order.client_order_id.value,
            recv_window=str(5000),
        )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        for order in command.order_list:
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

        for order in command.order_list:
            if order.linked_order_ids:  # TODO(cs): Implement
                self._log.warning(f"Cannot yet handle OCO conditional orders, {order}.")
            await self._submit_order(order)

    async def _submit_stop_market_order(self, order: StopMarketOrder) -> None:
        time_in_force = self._enum_parser.parse_internal_time_in_force(order.time_in_force)

        if self._binance_account_type.is_spot_or_margin:
            working_type = None
        elif order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST_TRADE):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK_PRICE:
            working_type = "MARK_PRICE"
        else:
            self._log.error(
                f"Cannot submit order: invalid `order.trigger_type`, was "
                f"{trigger_type_to_str(order.trigger_price)}. {order}",
            )
            return

        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            time_in_force=time_in_force,
            quantity=str(order.quantity),
            stop_price=str(order.trigger_price),
            working_type=working_type,
            reduce_only=str(order.is_reduce_only) if order.is_reduce_only is True else None,
            new_client_order_id=order.client_order_id.value,
            recv_window=str(5000),
        )

    async def _submit_trailing_stop_market_order(self, order: TrailingStopMarketOrder) -> None:
        time_in_force = self._enum_parser.parse_internal_time_in_force(order.time_in_force)

        if order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST_TRADE):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK_PRICE:
            working_type = "MARK_PRICE"
        else:
            self._log.error(
                f"Cannot submit order: invalid `order.trigger_type`, was "
                f"{trigger_type_to_str(order.trigger_price)}. {order}",
            )
            return

        if order.trailing_offset_type != TrailingOffsetType.BASIS_POINTS:
            self._log.error(
                f"Cannot submit order: invalid `order.trailing_offset_type`, was "
                f"{trailing_offset_type_to_str(order.trailing_offset_type)} (use `BASIS_POINTS`). "
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
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            time_in_force=time_in_force,
            quantity=str(order.quantity),
            activation_price=str(activation_price),
            callback_rate=str(order.trailing_offset / 100),
            working_type=working_type,
            reduce_only=str(order.is_reduce_only) if order.is_reduce_only is True else None,
            new_client_order_id=order.client_order_id.value,
            recv_window=str(5000),
        )

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        # Parse instrument ID
        nautilus_symbol: str = BinanceSymbol(symbol).parse_binance_to_internal(
            self._binance_account_type,
        )
        instrument_id: Optional[InstrumentId] = self._instrument_ids.get(nautilus_symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(nautilus_symbol), BINANCE_VENUE)
            self._instrument_ids[nautilus_symbol] = instrument_id
        return instrument_id

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

        await self._cancel_order_single(
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
        )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        open_orders_strategy = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )
        for order in open_orders_strategy:
            if order.is_pending_cancel:
                continue  # Already pending cancel
            self.generate_order_pending_cancel(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

        # Check total orders for instrument
        open_orders_total_count = self._cache.orders_open_count(
            instrument_id=command.instrument_id,
        )

        try:
            if open_orders_total_count == len(open_orders_strategy):
                await self._http_account.cancel_all_open_orders(
                    symbol=command.instrument_id.symbol.value,
                )
            else:
                for order in open_orders_strategy:
                    await self._cancel_order_single(
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                    )
        except BinanceError as e:
            self._log.exception(f"Cannot cancel open orders: {e.message}", e)

    async def _cancel_order_single(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
    ) -> None:
        try:
            if venue_order_id is not None:
                await self._http_account.cancel_order(
                    symbol=instrument_id.symbol.value,
                    order_id=venue_order_id.value,
                )
            else:
                await self._http_account.cancel_order(
                    symbol=instrument_id.symbol.value,
                    orig_client_order_id=client_order_id.value,
                )
        except BinanceError as e:
            self._log.exception(
                f"Cannot cancel order "
                f"{repr(client_order_id)}, "
                f"{repr(venue_order_id)}: "
                f"{e.message}",
                e,
            )

    # -- WEBSOCKET EVENT HANDLERS --------------------------------------------------------------------

    def _handle_user_ws_message(self, raw: bytes) -> None:
        # Implement in child class
        raise NotImplementedError
