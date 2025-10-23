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
import os
from collections.abc import Awaitable
from collections.abc import Callable
from decimal import Decimal

from nautilus_trader.adapters.binance.common.constants import BINANCE_MAX_CALLBACK_RATE
from nautilus_trader.adapters.binance.common.constants import BINANCE_MIN_CALLBACK_RATE
from nautilus_trader.adapters.binance.common.constants import BINANCE_PRICE_MATCH_ORDER_TYPES
from nautilus_trader.adapters.binance.common.constants import BINANCE_PRICE_MATCH_VALUES
from nautilus_trader.adapters.binance.common.constants import BINANCE_RETRY_WARNINGS
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnumParser
from nautilus_trader.adapters.binance.common.enums import BinanceErrorCode
from nautilus_trader.adapters.binance.common.enums import BinanceFuturesPositionSide
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.account import BinanceOrder
from nautilus_trader.adapters.binance.common.schemas.account import BinanceUserTrade
from nautilus_trader.adapters.binance.common.schemas.user import BinanceListenKey
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.http.account import BinanceAccountHttpAPI
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.http.error import BinanceError
from nautilus_trader.adapters.binance.http.error import get_binance_error_code
from nautilus_trader.adapters.binance.http.error import should_retry
from nautilus_trader.adapters.binance.http.market import BinanceMarketHttpAPI
from nautilus_trader.adapters.binance.http.user import BinanceUserDataHttpAPI
from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import nanos_to_millis
from nautilus_trader.core.datetime import secs_to_millis
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.live.retry import RetryManagerPool
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import trailing_offset_type_to_str
from nautilus_trader.model.enums import trigger_type_to_str
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import Order
from nautilus_trader.model.orders import StopLimitOrder
from nautilus_trader.model.orders import StopMarketOrder
from nautilus_trader.model.orders import TrailingStopMarketOrder
from nautilus_trader.model.position import Position


class BinanceCommonExecutionClient(LiveExecutionClient):
    """
    Execution client providing common functionality for the Binance exchanges.

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
    instrument_provider : BinanceSpotInstrumentProvider
        The instrument provider.
    account_type : BinanceAccountType
        The account type for the client.
    base_url_ws : str
        The base URL for the WebSocket client.
    name : str, optional
        The custom client ID.
    config : BinanceExecClientConfig
        The configuration for the client.

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
        instrument_provider: InstrumentProvider,
        account_type: BinanceAccountType,
        base_url_ws: str,
        name: str | None,
        config: BinanceExecClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or config.venue.value),
            venue=config.venue,
            oms_type=OmsType.HEDGING if account_type.is_futures else OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.CASH if account_type.is_spot else AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Configuration
        self._binance_account_type: BinanceAccountType = account_type
        self._use_gtd: bool = config.use_gtd
        self._use_reduce_only: bool = config.use_reduce_only
        self._use_position_ids: bool = config.use_position_ids
        self._treat_expired_as_canceled: bool = config.treat_expired_as_canceled
        self._log_rejected_due_post_only_as_warning: bool = (
            config.log_rejected_due_post_only_as_warning
        )
        self._recv_window = config.recv_window_ms
        self._max_retries = config.max_retries or 3
        self._log.info(f"Key type: {config.key_type.value}", LogColor.BLUE)
        self._log.info(f"Account type: {self._binance_account_type.value}", LogColor.BLUE)
        self._log.info(f"{config.use_gtd=}", LogColor.BLUE)
        self._log.info(f"{config.use_reduce_only=}", LogColor.BLUE)
        self._log.info(f"{config.use_position_ids=}", LogColor.BLUE)
        self._log.info(f"{config.treat_expired_as_canceled=}", LogColor.BLUE)
        self._log.info(f"{config.recv_window_ms=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)
        self._log.info(f"{config.listen_key_ping_max_failures=}", LogColor.BLUE)
        self._log.info(f"{config.log_rejected_due_post_only_as_warning=}", LogColor.BLUE)

        self._is_dual_side_position: bool | None = None  # Initialized on connection
        self._set_account_id(
            AccountId(f"{name or config.venue.value}-{self._binance_account_type.value}-master"),
        )

        # Enum parser
        self._enum_parser = enum_parser

        # HTTP API
        self._http_client = client
        self._http_account = account
        self._http_market = market
        self._http_user = user

        # Listen keys
        self._ping_listen_keys_interval: int = 60 * 5  # Once every 5 mins (hardcoded)
        self._ping_listen_keys_task: asyncio.Task | None = None
        self._listen_key: str | None = None
        self._ping_consecutive_failures: int = 0
        self._ping_max_failures: int = config.listen_key_ping_max_failures
        self._last_successful_ping_ns: int = 0

        # WebSocket API
        self._ws_client = BinanceWebSocketClient(
            clock=clock,
            handler=self._handle_user_ws_message,
            handler_reconnect=None,
            base_url=base_url_ws,
            loop=self._loop,
        )

        # Order submission method hashmap
        self._submit_order_method: dict[
            OrderType,
            Callable[[Order, BinanceFuturesPositionSide | None, str | None], Awaitable[None]],
        ] = {
            OrderType.MARKET: self._submit_market_order,
            OrderType.LIMIT: self._submit_limit_order,
            OrderType.STOP_LIMIT: self._submit_stop_limit_order,
            OrderType.LIMIT_IF_TOUCHED: self._submit_stop_limit_order,
            OrderType.STOP_MARKET: self._submit_stop_market_order,
            OrderType.MARKET_IF_TOUCHED: self._submit_stop_market_order,
            OrderType.TRAILING_STOP_MARKET: self._submit_trailing_stop_market_order,
        }

        # Hot caches
        self._instrument_ids: dict[str, InstrumentId] = {}
        self._generate_order_status_retries: dict[ClientOrderId, int] = {}

        self._retry_manager_pool = RetryManagerPool[None](
            pool_size=100,
            max_retries=config.max_retries or 0,
            delay_initial_ms=config.retry_delay_initial_ms or 1_000,
            delay_max_ms=config.retry_delay_max_ms or 10_000,
            backoff_factor=2,
            logger=self._log,
            exc_types=(BinanceError,),
            retry_check=should_retry,
            error_logger=self._log_retry_error,
        )

        self._log.info(f"Base url HTTP {self._http_client.base_url}", LogColor.BLUE)
        self._log.info(f"Base url WebSocket {base_url_ws}", LogColor.BLUE)

    @property
    def use_position_ids(self) -> bool:
        """
        Whether a `position_id` will be assigned to order events generated by the
        client.

        Returns
        -------
        bool

        """
        return self._use_position_ids

    @property
    def treat_expired_as_canceled(self) -> bool:
        """
        Whether the `EXPIRED` execution type is treated as a `CANCEL`.

        Returns
        -------
        bool

        """
        return self._treat_expired_as_canceled

    def _stop(self) -> None:
        self._retry_manager_pool.shutdown()

    def _log_retry_error(self, message: str, exception: BaseException | None) -> None:
        error_code = get_binance_error_code(exception) if exception else None

        match error_code:
            case (
                BinanceErrorCode.GTX_ORDER_REJECT
            ) if not self._log_rejected_due_post_only_as_warning:
                self._log.info(message)
            case code if code in BINANCE_RETRY_WARNINGS:
                self._log.warning(message)
            case _:
                self._log.error(message)

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._update_account_state()
        await self._await_account_registered()
        await self._init_dual_side_position()

        response: BinanceListenKey = await self._http_user.create_listen_key()

        # Check Binance-Nautilus clock sync
        server_time: int = await self._http_market.request_server_time()
        self._log.info(f"Binance server time {server_time} UNIX (ms)")

        nautilus_time: int = self._clock.timestamp_ms()
        self._log.info(f"Nautilus clock time {nautilus_time} UNIX (ms)")

        # Set up WebSocket listen key
        self._listen_key = response.listenKey
        self._last_successful_ping_ns = self._clock.timestamp_ns()  # Initialize on connection
        self._log.info(f"Listen key {self._listen_key}")
        self._ping_listen_keys_task = self.create_task(self._ping_listen_keys())

        await self._ws_client.subscribe_listen_key(self._listen_key)

    async def _update_account_state(self) -> None:
        # Replace method in child class
        raise NotImplementedError

    async def _init_dual_side_position(self) -> None:
        # Replace method in child class
        raise NotImplementedError

    async def _ping_listen_keys(self) -> None:
        try:
            while True:
                self._log.debug(
                    f"Scheduled task 'ping_listen_keys' to run in "
                    f"{self._ping_listen_keys_interval}s",
                )
                await asyncio.sleep(self._ping_listen_keys_interval)

                if not self._listen_key:
                    self._log.warning("No listen key available for ping")
                    continue

                self._log.debug(f"Pinging WebSocket listen key {self._listen_key}")

                try:
                    await self._http_user.keepalive_listen_key(listen_key=self._listen_key)

                    # Reset failure tracking on success
                    self._ping_consecutive_failures = 0
                    self._last_successful_ping_ns = self._clock.timestamp_ns()
                    self._log.debug(f"Listen key ping successful: {self._listen_key}")

                except (BinanceClientError, BinanceError) as e:
                    self._ping_consecutive_failures += 1
                    time_since_success_secs = (
                        (self._clock.timestamp_ns() - self._last_successful_ping_ns) / 1_000_000_000
                        if self._last_successful_ping_ns > 0
                        else 0
                    )

                    self._log.error(
                        f"Listen key ping failed (attempt {self._ping_consecutive_failures}/"
                        f"{self._ping_max_failures}): {e}, "
                        f"time since last success: {time_since_success_secs:.1f}s",
                    )

                    if self._ping_consecutive_failures >= self._ping_max_failures:
                        self._log.error(
                            f"Listen key ping failed {self._ping_max_failures} consecutive times; "
                            "initiating WebSocket reconnection to prevent data loss",
                        )
                        await self._handle_listen_key_failure()
                        self._ping_consecutive_failures = 0  # Reset after handling
        except asyncio.CancelledError:
            self._log.debug("Canceled task 'ping_listen_keys'")

    async def _handle_listen_key_failure(self) -> None:
        # Handle listen key authentication failure with full recovery.
        #
        # This method attempts to recover from listen key failures by:
        # 1. Disconnecting the current WebSocket
        # 2. Creating a new listen key
        # 3. Reconnecting the WebSocket with the new key

        try:
            self._log.warning("Starting listen key recovery process")

            # Disconnect current WebSocket
            await self._ws_client.disconnect()
            self._log.debug("Disconnected WebSocket for listen key recovery")

            # Create new listen key
            response: BinanceListenKey = await self._http_user.create_listen_key()
            self._listen_key = response.listenKey
            self._last_successful_ping_ns = self._clock.timestamp_ns()
            self._log.info(f"Created new listen key for recovery: {self._listen_key}")

            # Reconnect WebSocket with new key
            await self._ws_client.subscribe_listen_key(self._listen_key)
            self._log.info("WebSocket reconnected successfully with new listen key")

        except Exception as e:
            self._log.error(f"Failed to recover from listen key failure: {e}")

            # Check if graceful shutdown is configured
            if hasattr(self, "graceful_shutdown_on_exception"):
                execution_engine = getattr(self, "_execution_engine", None)
                if execution_engine and hasattr(execution_engine, "graceful_shutdown_on_exception"):
                    if execution_engine.graceful_shutdown_on_exception:
                        execution_engine.shutdown_system(f"Listen key recovery failed: {e}")
                        return

            self._log.error(
                "Terminating process to prevent operation with invalid authentication",
            )
            os._exit(1)

    async def _disconnect(self) -> None:
        # Cancel tasks
        if self._ping_listen_keys_task:
            self._log.debug("Canceling task 'ping_listen_keys'")
            self._ping_listen_keys_task.cancel()
            self._ping_listen_keys_task = None

        await self._ws_client.disconnect()

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(  # noqa: C901 (too complex)
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        PyCondition.is_false(
            command.client_order_id is None and command.venue_order_id is None,
            "both `client_order_id` and `venue_order_id` were `None`",
        )

        retries = self._generate_order_status_retries.get(command.client_order_id, 0)
        if retries > self._max_retries:
            self._log.error(
                f"Reached maximum retries {self._max_retries}/{self._max_retries} for generating OrderStatusReport for "
                f"{repr(command.client_order_id) if command.client_order_id else ''} "
                f"{repr(command.venue_order_id) if command.venue_order_id else ''}",
            )

            # Clean up retry counter after max retries exceeded
            if (
                command.client_order_id
                and command.client_order_id in self._generate_order_status_retries
            ):
                del self._generate_order_status_retries[command.client_order_id]

            return None

        self._log.info(
            f"Generating OrderStatusReport for "
            f"{repr(command.client_order_id) if command.client_order_id else ''} "
            f"{repr(command.venue_order_id) if command.venue_order_id else ''}",
        )

        try:
            if command.venue_order_id:
                binance_order = await self._http_account.query_order(
                    symbol=command.instrument_id.symbol.value,
                    order_id=int(command.venue_order_id.value),
                )
            else:
                binance_order = await self._http_account.query_order(
                    symbol=command.instrument_id.symbol.value,
                    orig_client_order_id=(
                        command.client_order_id.value
                        if command.client_order_id is not None
                        else None
                    ),
                )
        except BinanceError as e:
            retries += 1
            self._log.error(
                f"Cannot generate order status report for {command.client_order_id!r}: {e.message}. Retry {retries}/{self._max_retries}",
            )

            # Check for None before dictionary operations
            if not command.client_order_id:
                self._log.warning("Cannot retry without a client order ID")
                return None

            self._generate_order_status_retries[command.client_order_id] = retries

            order: Order | None = self._cache.order(command.client_order_id)
            if order is None:
                self._log.warning("Order not found in cache")
                return None
            elif order.is_closed:
                return None  # Nothing else to do

            if retries >= self._max_retries:
                if command.client_order_id in self._generate_order_status_retries:
                    del self._generate_order_status_retries[command.client_order_id]

                # Determine if the rejection was specifically due to a POST-ONLY order
                # that would have executed immediately as a taker (GTX_ORDER_REJECT -5022).
                due_post_only = _is_post_only_rejection(e)

                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=command.instrument_id,
                    client_order_id=command.client_order_id,
                    reason=str(e.message),
                    ts_event=self._clock.timestamp_ns(),
                    due_post_only=due_post_only,
                )
            return None  # Error now handled

        if not binance_order or (binance_order.origQty and Decimal(binance_order.origQty) == 0):
            # Cannot proceed to generating report
            self._log.error(
                f"Cannot generate `OrderStatusReport` for {command.client_order_id=!r}, {command.venue_order_id=!r}: "
                "order not found",
            )
            return None

        report: OrderStatusReport = binance_order.parse_to_order_status_report(
            account_id=self.account_id,
            instrument_id=self._get_cached_instrument_id(binance_order.symbol),
            report_id=UUID4(),
            enum_parser=self._enum_parser,
            treat_expired_as_canceled=self._treat_expired_as_canceled,
            ts_init=self._clock.timestamp_ns(),
        )

        # Clean up retry counter on successful report generation
        if (
            command.client_order_id
            and command.client_order_id in self._generate_order_status_retries
        ):
            del self._generate_order_status_retries[command.client_order_id]

        self._log.debug(f"Received {report}")
        return report

    def _get_cache_active_symbols(self) -> set[str]:
        # Check cache for all active symbols
        open_orders: list[Order] = self._cache.orders_open(venue=self.venue)
        open_positions: list[Position] = self._cache.positions_open(venue=self.venue)
        active_symbols: set[str] = set()
        for o in open_orders:
            active_symbols.add(o.instrument_id.symbol.value)
        for p in open_positions:
            active_symbols.add(p.instrument_id.symbol.value)
        return active_symbols

    async def _get_binance_position_status_reports(
        self,
        symbol: str | None = None,
    ) -> list[PositionStatusReport]:
        # Implement in child class
        raise NotImplementedError

    async def _get_binance_active_position_symbols(
        self,
        symbol: str | None = None,
    ) -> set[str]:
        # Implement in child class
        raise NotImplementedError

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        self._log.debug("Requesting OrderStatusReports...")

        try:
            # Check Binance for all order active symbols
            symbol = (
                command.instrument_id.symbol.value if command.instrument_id is not None else None
            )
            active_symbols = self._get_cache_active_symbols()
            active_symbols.update(await self._get_binance_active_position_symbols(symbol))
            binance_open_orders = await self._http_account.query_open_orders(symbol)

            for order in binance_open_orders:
                active_symbols.add(order.symbol)

            # Get all orders for those active symbols
            binance_orders: list[BinanceOrder] = []

            if command.open_only:
                binance_orders = binance_open_orders
            else:
                for symbol in active_symbols:
                    # Here we don't pass a `start_time` or `end_time` as order reports appear to go
                    # randomly missing when these are specified. We filter on the Nautilus side below.
                    # Explicitly setting limit to the max lookback of 1000, in the future we should
                    # add pagination.
                    response = await self._http_account.query_all_orders(symbol=symbol, limit=1_000)
                    binance_orders.extend(response)
        except BinanceError as e:
            self._log.exception(f"Cannot generate OrderStatusReport: {e.message}", e)
            return []

        start_ms = secs_to_millis(command.start.timestamp()) if command.start is not None else None
        end_ms = secs_to_millis(command.end.timestamp()) if command.end is not None else None

        reports: list[OrderStatusReport] = []
        for order in binance_orders:
            if start_ms is not None and order.time < start_ms:
                continue  # Filter start on the Nautilus side
            if end_ms is not None and order.time > end_ms:
                continue  # Filter end on the Nautilus side
            if order.origQty and Decimal(order.origQty) == 0:
                continue  # Cannot parse zero quantity order (filter for Binance)
            report = order.parse_to_order_status_report(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(order.symbol),
                report_id=UUID4(),
                enum_parser=self._enum_parser,
                treat_expired_as_canceled=self._treat_expired_as_canceled,
                ts_init=self._clock.timestamp_ns(),
            )
            self._log.debug(f"Received {report}")
            reports.append(report)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        receipt_log = f"Received {len(reports)} OrderStatusReport{plural}"

        if command.log_receipt_level == LogLevel.INFO:
            self._log.info(receipt_log)
        else:
            self._log.debug(receipt_log)

        return reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        self._log.debug("Requesting FillReports...")

        try:
            # Check Binance for all trades on active symbols
            symbol = (
                command.instrument_id.symbol.value if command.instrument_id is not None else None
            )
            active_symbols = self._get_cache_active_symbols()
            active_symbols.update(await self._get_binance_active_position_symbols(symbol))
            binance_trades: list[BinanceUserTrade] = []

            for symbol in active_symbols:
                response = await self._http_account.query_user_trades(
                    symbol=symbol,
                    start_time=(
                        secs_to_millis(command.start.timestamp())
                        if command.start is not None
                        else None
                    ),
                    end_time=(
                        secs_to_millis(command.end.timestamp()) if command.end is not None else None
                    ),
                )
                binance_trades.extend(response)
        except BinanceError as e:
            self._log.exception(f"Cannot generate FillReport: {e.message}", e)
            return []

        # Parse all Binance trades
        reports: list[FillReport] = []
        for trade in binance_trades:
            if trade.symbol is None:
                self._log.warning(f"No symbol for trade {trade}")
                continue
            report = trade.parse_to_fill_report(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(trade.symbol),
                report_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                use_position_ids=self._use_position_ids,
            )
            self._log.debug(f"Received {report}")
            reports.append(report)

        # Confirm sorting in ascending order
        reports = sorted(reports, key=lambda x: x.trade_id)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} FillReport{plural}")

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        try:
            if command.instrument_id:
                self._log.info(f"Requesting PositionStatusReport for {command.instrument_id}")
                symbol = command.instrument_id.symbol.value
                reports = await self._get_binance_position_status_reports(symbol)
                if not reports:
                    now = self._clock.timestamp_ns()
                    report = PositionStatusReport(
                        account_id=self.account_id,
                        instrument_id=command.instrument_id,
                        position_side=PositionSide.FLAT,
                        quantity=Quantity.zero(),
                        report_id=UUID4(),
                        ts_last=now,
                        ts_init=now,
                    )
                    reports = [report]
            else:
                self._log.debug("Requesting PositionStatusReports...")
                reports = await self._get_binance_position_status_reports()
        except BinanceError as e:
            self._log.exception(f"Cannot generate PositionStatusReport: {e.message}", e)
            return []

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} PositionStatusReport{plural}")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def _check_order_validity(self, order: Order) -> str | None:
        # Implement in child class
        raise NotImplementedError

    def _determine_time_in_force(self, order: Order) -> BinanceTimeInForce:
        # Convert the internal TimeInForce enum to the Binance equivalent
        time_in_force: BinanceTimeInForce = self._enum_parser.parse_internal_time_in_force(
            order.time_in_force,
        )

        # When the client is configured *not* to make use of the native GTD
        # (Good-Till-Date) support on Binance we transparently downgrade GTD to
        # GTC. Comparison must be performed against the *Binance* enum; the
        # previous implementation compared against the internal Nautilus enum
        # which would always evaluate to ``False`` and therefore never apply
        # the downgrade.
        if time_in_force == BinanceTimeInForce.GTD and not self._use_gtd:
            time_in_force = BinanceTimeInForce.GTC
            self._log.info(
                f"Converted GTD `time_in_force` to GTC for {order.client_order_id}",
                LogColor.BLUE,
            )
        return time_in_force

    def _determine_good_till_date(
        self,
        order: Order,
        time_in_force: BinanceTimeInForce | None,
    ) -> int | None:
        if time_in_force is None or time_in_force != BinanceTimeInForce.GTD:
            return None

        good_till_date = nanos_to_millis(order.expire_time_ns) if order.expire_time_ns else None
        if self._binance_account_type.is_spot_or_margin:
            good_till_date = None
            self._log.warning("Cannot set GTD time in force with `expiry_time` for Binance Spot")
        return good_till_date

    def _determine_reduce_only(self, order: Order) -> bool:
        return order.is_reduce_only if self._use_reduce_only else False

    def _determine_reduce_only_str(self, order: Order) -> str | None:
        # `reduceOnly` Cannot be sent in Futures Hedge Mode
        if self._binance_account_type.is_futures and not self._is_dual_side_position:
            return str(self._determine_reduce_only(order))
        return None

    def _get_position_side_from_position_id(
        self,
        position_id: PositionId | None,
        exec_spawn_id: ClientOrderId | None,
    ) -> BinanceFuturesPositionSide | None:
        # Position ID must end with either 'LONG', 'SHORT' or 'BOTH' for Binance Futures Hedge position mode

        position_side = None

        if self._binance_account_type.is_spot_or_margin:  # Spot or Margin mode
            return position_side
        elif not self._is_dual_side_position:  # One-way position mode
            return BinanceFuturesPositionSide.BOTH

        if position_id is None and exec_spawn_id is not None:
            position_id = self._cache.position_id(exec_spawn_id)

        # For Binance Futures Hedge mode, the position side must be specified in the position_id
        PyCondition.not_none(position_id, "position_id")
        position_side = self._enum_parser.parse_position_id_to_binance_futures_position_side(
            position_id,
        )
        # Check if the position side is valid
        PyCondition.is_in(
            position_side,
            [BinanceFuturesPositionSide.LONG, BinanceFuturesPositionSide.SHORT],
            "position_side",
            "HedgeModePositionSides",
        )
        return position_side

    async def _query_account(self, _command: QueryAccount) -> None:
        await self._update_account_state()

    async def _submit_order(self, command: SubmitOrder) -> None:
        position_side = self._get_position_side_from_position_id(
            position_id=command.position_id,
            exec_spawn_id=command.order.exec_spawn_id,
        )
        await self._submit_order_inner(command.order, position_side, command.params)

    async def _submit_order_inner(
        self,
        order: Order,
        position_side: BinanceFuturesPositionSide | None,
        params: dict[str, object] | None = None,
    ) -> None:
        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order {order}")
            return

        try:
            price_match = self._extract_price_match(order, params)
        except ValueError as e:
            self._deny_order_pre_submit(order, str(e))
            return

        # Validate order before submission
        validation_error = self._validate_order_pre_submit(order)
        if validation_error:
            self._deny_order_pre_submit(order, validation_error)
            return

        self._log.debug(f"Submitting {order}, position_side={position_side}")

        # Generate event here to ensure correct ordering of events
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        retry_manager = await self._retry_manager_pool.acquire()
        try:
            await retry_manager.run(
                "submit_order",
                [order.client_order_id],
                self._submit_order_method[order.order_type],
                order,
                position_side,
                price_match,
            )
            if not retry_manager.result:
                # Determine if the rejection was specifically due to a POST-ONLY order
                # that would have executed immediately as a taker (GTX_ORDER_REJECT -5022).
                last_exc = retry_manager.last_exception
                due_post_only = (
                    _is_post_only_rejection(last_exc)
                    if isinstance(last_exc, BinanceError)
                    else False
                )

                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=retry_manager.message,
                    ts_event=self._clock.timestamp_ns(),
                    due_post_only=due_post_only,
                )
        finally:
            await self._retry_manager_pool.release(retry_manager)

    def _extract_price_match(
        self,
        order: Order,
        params: dict[str, object] | None,
    ) -> str | None:
        if params is None:
            return None

        raw_value = params.get("price_match")
        if raw_value is None:
            return None

        if not self._binance_account_type.is_futures:
            raise ValueError(
                "UNSUPPORTED: `price_match` is only supported for Binance futures accounts",
            )

        if not isinstance(raw_value, str):
            raise ValueError(
                "INVALID_ARG: `price_match` must be provided as a string value",
            )

        value = raw_value.upper()
        if value not in BINANCE_PRICE_MATCH_VALUES:
            raise ValueError(
                "INVALID_ARG: `price_match` value "
                f"{raw_value!r} is not one of {sorted(BINANCE_PRICE_MATCH_VALUES)}",
            )

        if order.is_post_only:
            raise ValueError(
                "UNSUPPORTED: `price_match` cannot be combined with post-only instructions on Binance",
            )

        display_qty = getattr(order, "display_qty", None)
        if display_qty is not None:
            raise ValueError(
                "UNSUPPORTED: `price_match` cannot be combined with iceberg/display quantities on Binance",
            )

        if order.order_type not in BINANCE_PRICE_MATCH_ORDER_TYPES:
            raise ValueError(
                f"UNSUPPORTED: `price_match` is not supported for order type {order.type_string()} on Binance",
            )

        return value

    def _deny_order_pre_submit(self, order: Order, reason: str) -> None:
        self.generate_order_denied(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            reason=reason,
            ts_event=self._clock.timestamp_ns(),
        )

    def _validate_order_pre_submit(self, order: Order) -> str | None:  # noqa: C901 (too complex)
        # Check order type and time-in-force validity
        validity_error = self._check_order_validity(order)
        if validity_error:
            return validity_error

        # Market order validations
        if isinstance(order, MarketOrder):
            if order.is_quote_quantity and not self._binance_account_type.is_spot_or_margin:
                return "UNSUPPORTED_QUOTE_QUANTITY"

        # Stop limit order validations
        elif isinstance(order, StopLimitOrder):
            if not self._binance_account_type.is_spot_or_margin:
                if order.trigger_type not in (
                    TriggerType.DEFAULT,
                    TriggerType.LAST_PRICE,
                    TriggerType.MARK_PRICE,
                ):
                    return f"INVALID_TRIGGER_TYPE: {trigger_type_to_str(order.trigger_type)}"

        # Stop market order validations
        elif isinstance(order, StopMarketOrder):
            if not self._binance_account_type.is_spot_or_margin:
                if order.trigger_type not in (
                    TriggerType.DEFAULT,
                    TriggerType.LAST_PRICE,
                    TriggerType.MARK_PRICE,
                ):
                    return f"INVALID_TRIGGER_TYPE: {trigger_type_to_str(order.trigger_type)}"

        # Trailing stop market order validations
        elif isinstance(order, TrailingStopMarketOrder):
            if order.trigger_type not in (
                TriggerType.DEFAULT,
                TriggerType.LAST_PRICE,
                TriggerType.MARK_PRICE,
            ):
                return f"INVALID_TRIGGER_TYPE: {trigger_type_to_str(order.trigger_type)}"

            if order.trailing_offset_type != TrailingOffsetType.BASIS_POINTS:
                return f"INVALID_TRAILING_OFFSET_TYPE: {trailing_offset_type_to_str(order.trailing_offset_type)}"

            callback_rate = Decimal(order.trailing_offset) / Decimal("100")
            callback_rate = callback_rate.quantize(Decimal("0.1"))

            if (
                callback_rate < BINANCE_MIN_CALLBACK_RATE
                or callback_rate > BINANCE_MAX_CALLBACK_RATE
            ):
                return f"INVALID_TRAILING_OFFSET: {callback_rate}% not in range [{BINANCE_MIN_CALLBACK_RATE}, {BINANCE_MAX_CALLBACK_RATE}]"

            if order.trigger_price is not None:
                return "INVALID_TRIGGER_PRICE: use activation_price for trailing stop orders"

        return None  # Valid

    async def _submit_market_order(
        self,
        order: MarketOrder,
        position_side: BinanceFuturesPositionSide | None,
        price_match: str | None,
    ) -> None:
        assert price_match is None  # type checking

        if order.is_quote_quantity:
            quantity = None
            quote_order_qty = str(order.quantity)
        else:
            quantity = str(order.quantity)
            quote_order_qty = None

        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            quantity=quantity,
            quote_order_qty=quote_order_qty,
            reduce_only=self._determine_reduce_only_str(order),
            new_client_order_id=order.client_order_id.value,
            recv_window=str(self._recv_window),
            position_side=position_side,
        )

    async def _submit_limit_order(
        self,
        order: LimitOrder,
        position_side: BinanceFuturesPositionSide | None,
        price_match: str | None,
    ) -> None:
        time_in_force = self._determine_time_in_force(order)
        if order.is_post_only and self._binance_account_type.is_spot_or_margin:
            time_in_force = None
        elif order.is_post_only and self._binance_account_type.is_futures:
            time_in_force = BinanceTimeInForce.GTX

        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            time_in_force=time_in_force,
            good_till_date=self._determine_good_till_date(order, time_in_force),
            quantity=str(order.quantity),
            price=None if price_match else str(order.price),
            iceberg_qty=str(order.display_qty) if order.display_qty is not None else None,
            reduce_only=self._determine_reduce_only_str(order),
            new_client_order_id=order.client_order_id.value,
            recv_window=str(self._recv_window),
            position_side=position_side,
            price_match=price_match,
        )

    async def _submit_stop_limit_order(
        self,
        order: StopLimitOrder,
        position_side: BinanceFuturesPositionSide | None,
        price_match: str | None,
    ) -> None:
        if self._binance_account_type.is_spot_or_margin:
            working_type = None
        elif order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST_PRICE):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK_PRICE:
            working_type = "MARK_PRICE"
        else:
            raise RuntimeError(
                f"Unexpected trigger_type {trigger_type_to_str(order.trigger_type)} for StopLimitOrder - "
                "should have been validated in _validate_order_pre_submit",
            )

        time_in_force = self._determine_time_in_force(order)

        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            time_in_force=time_in_force,
            good_till_date=self._determine_good_till_date(order, time_in_force),
            quantity=str(order.quantity),
            price=None if price_match else str(order.price),
            stop_price=str(order.trigger_price),
            working_type=working_type,
            iceberg_qty=str(order.display_qty) if order.display_qty is not None else None,
            reduce_only=self._determine_reduce_only_str(order),
            new_client_order_id=order.client_order_id.value,
            recv_window=str(self._recv_window),
            position_side=position_side,
            price_match=price_match,
        )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        position_side = self._get_position_side_from_position_id(
            position_id=command.position_id,
            exec_spawn_id=None,
        )

        for order in command.order_list.orders:
            if order.linked_order_ids:
                # Deny all orders in the list if any have linked orders (OCO not supported)
                for list_order in command.order_list.orders:
                    self._deny_order_pre_submit(
                        list_order,
                        "UNSUPPORTED_OCO_CONDITIONAL_ORDERS",
                    )
                return

            await self._submit_order_inner(order, position_side, command.params)

    async def _submit_stop_market_order(
        self,
        order: StopMarketOrder,
        position_side: BinanceFuturesPositionSide | None,
        price_match: str | None,
    ) -> None:
        assert price_match is None  # type checking

        if self._binance_account_type.is_spot_or_margin:
            working_type = None
        elif order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST_PRICE):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK_PRICE:
            working_type = "MARK_PRICE"
        else:
            raise RuntimeError(
                f"Unexpected trigger_type {trigger_type_to_str(order.trigger_type)} for StopMarketOrder - "
                "should have been validated in _validate_order_pre_submit",
            )

        time_in_force = self._determine_time_in_force(order)

        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            time_in_force=time_in_force,
            good_till_date=self._determine_good_till_date(order, time_in_force),
            quantity=str(order.quantity),
            stop_price=str(order.trigger_price),
            working_type=working_type,
            reduce_only=self._determine_reduce_only_str(order),
            new_client_order_id=order.client_order_id.value,
            recv_window=str(self._recv_window),
            position_side=position_side,
        )

    async def _submit_trailing_stop_market_order(
        self,
        order: TrailingStopMarketOrder,
        position_side: BinanceFuturesPositionSide | None,
        price_match: str | None,
    ) -> None:
        assert price_match is None  # type checking

        if order.trigger_type in (TriggerType.DEFAULT, TriggerType.LAST_PRICE):
            working_type = "CONTRACT_PRICE"
        elif order.trigger_type == TriggerType.MARK_PRICE:
            working_type = "MARK_PRICE"
        else:
            raise RuntimeError(
                f"Unexpected trigger_type {trigger_type_to_str(order.trigger_type)} for TrailingStopMarketOrder - "
                "should have been validated in _validate_order_pre_submit",
            )

        time_in_force = self._determine_time_in_force(order)

        # Convert basis points to percentage, preserving precision
        # Binance supports up to 1 decimal place precision for callback rates
        callback_rate = Decimal(order.trailing_offset) / Decimal("100")
        # Round to 1 decimal place only if necessary to meet Binance requirements
        callback_rate = callback_rate.quantize(Decimal("0.1"))

        activation_price: Price | None = order.activation_price

        await self._http_account.new_order(
            symbol=order.instrument_id.symbol.value,
            side=self._enum_parser.parse_internal_order_side(order.side),
            order_type=self._enum_parser.parse_internal_order_type(order),
            time_in_force=time_in_force,
            good_till_date=self._determine_good_till_date(order, time_in_force),
            quantity=str(order.quantity),
            activation_price=str(activation_price) if activation_price is not None else None,
            callback_rate=str(callback_rate),
            working_type=working_type,
            reduce_only=self._determine_reduce_only_str(order),
            new_client_order_id=order.client_order_id.value,
            recv_window=str(self._recv_window),
            position_side=position_side,
        )

    def _get_cached_instrument_id(self, symbol: str) -> InstrumentId:
        nautilus_symbol: str = BinanceSymbol(symbol).parse_as_nautilus(
            self._binance_account_type,
        )
        instrument_id: InstrumentId | None = self._instrument_ids.get(nautilus_symbol)
        if not instrument_id:
            instrument_id = InstrumentId(Symbol(nautilus_symbol), self.venue)
            self._instrument_ids[nautilus_symbol] = instrument_id
        return instrument_id

    async def _modify_order(self, command: ModifyOrder) -> None:
        if self._binance_account_type.is_spot_or_margin:
            self._log.error(
                "Cannot modify order: only supported for `USDT_FUTURES` and `COIN_FUTURES` account types",
            )
            return

        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found to modify")
            return

        if order.order_type != OrderType.LIMIT:
            self._log.error(
                "Cannot modify order: "
                f"only LIMIT orders supported by the venue (was {order.type_string()})",
            )
            return

        retry_manager = await self._retry_manager_pool.acquire()
        try:
            await retry_manager.run(
                "modify_order",
                [order.client_order_id, order.venue_order_id],
                self._http_account.modify_order,
                symbol=order.instrument_id.symbol.value,
                order_id=int(order.venue_order_id.value) if order.venue_order_id else None,
                side=self._enum_parser.parse_internal_order_side(order.side),
                quantity=str(command.quantity) if command.quantity else str(order.quantity),
                price=str(command.price) if command.price else str(order.price),
            )
            if not retry_manager.result:
                self.generate_order_modify_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                    retry_manager.message,
                    self._clock.timestamp_ns(),
                )
        finally:
            await self._retry_manager_pool.release(retry_manager)

    async def _cancel_order(self, command: CancelOrder) -> None:
        retry_manager = await self._retry_manager_pool.acquire()
        try:
            await retry_manager.run(
                "cancel_order",
                [command.client_order_id, command.venue_order_id],
                self._cancel_order_single,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
            )
            if not retry_manager.result:
                self.generate_order_cancel_rejected(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                    retry_manager.message,
                    self._clock.timestamp_ns(),
                )
        finally:
            await self._retry_manager_pool.release(retry_manager)

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        open_orders_strategy: list[Order] = self._cache.orders_open(
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
        )

        # Check total orders for instrument
        open_orders_total_count = self._cache.orders_open_count(
            instrument_id=command.instrument_id,
        )

        if open_orders_total_count == len(open_orders_strategy):
            retry_manager = await self._retry_manager_pool.acquire()
            try:
                await retry_manager.run(
                    "cancel_all_open_orders",
                    [command.instrument_id],
                    self._http_account.cancel_all_open_orders,
                    symbol=command.instrument_id.symbol.value,
                )
                if not retry_manager.result:
                    if retry_manager.message is not None:
                        if "Unknown order sent" in retry_manager.message:
                            self._log.info(
                                "No open orders to cancel according to Binance",
                                LogColor.GREEN,
                            )
                            return
                    for order in open_orders_strategy:
                        if order.is_closed:
                            continue
                        self.generate_order_cancel_rejected(
                            order.strategy_id,
                            order.instrument_id,
                            order.client_order_id,
                            order.venue_order_id,
                            retry_manager.message,
                            self._clock.timestamp_ns(),
                        )
                return
            finally:
                await self._retry_manager_pool.release(retry_manager)

        # Not every strategy order is included in all orders - so must cancel individually
        # TODO: A future improvement could be to asyncio.gather all cancel tasks
        for order in open_orders_strategy:
            retry_manager = await self._retry_manager_pool.acquire()
            try:
                await retry_manager.run(
                    "cancel_order",
                    [order.client_order_id, order.venue_order_id],
                    self._cancel_order_single,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                )
                if not retry_manager.result:
                    self.generate_order_cancel_rejected(
                        order.strategy_id,
                        order.instrument_id,
                        order.client_order_id,
                        order.venue_order_id,
                        retry_manager.message,
                        self._clock.timestamp_ns(),
                    )
            finally:
                await self._retry_manager_pool.release(retry_manager)

    async def _cancel_order_single(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
    ) -> None:
        order: Order | None = self._cache.order(client_order_id)
        if order is None:
            # Cannot generate cancel rejected event without order in cache
            self._log.error(f"{client_order_id!r} not found to cancel")
            return

        if order.is_closed:
            self._log.warning(
                f"CancelOrder command for {client_order_id!r} when order already {order.status_string()} "
                "(will not send to exchange)",
            )
            return

        await self._http_account.cancel_order(
            symbol=instrument_id.symbol.value,
            order_id=int(venue_order_id.value) if venue_order_id else None,
            orig_client_order_id=client_order_id.value if client_order_id else None,
        )

    # -- WEBSOCKET EVENT HANDLERS -----------------------------------------------------------------

    def _handle_user_ws_message(self, raw: bytes) -> None:
        # Implement in child class
        raise NotImplementedError


def _is_post_only_rejection(error: BinanceError) -> bool:
    error_code = get_binance_error_code(error)
    return error_code == BinanceErrorCode.GTX_ORDER_REJECT
