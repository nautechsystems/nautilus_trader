# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.adapters.binance.common.constants import BINANCE_FUTURES_ALGO_ORDER_TYPES
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceEnvironment
from nautilus_trader.adapters.binance.common.enums import BinanceErrorCode
from nautilus_trader.adapters.binance.common.enums import BinanceExecutionType
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.execution import BinanceCommonExecutionClient
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEnumParser
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEventType
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.adapters.binance.futures.http.market import BinanceFuturesMarketHttpAPI
from nautilus_trader.adapters.binance.futures.providers import BinanceFuturesInstrumentProvider
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountInfo
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAlgoOrder
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesDualSidePosition
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesLeverage
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesPositionRisk
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesAccountUpdateMsg
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesAlgoUpdateMsg
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesOrderUpdateMsg
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesTradeLiteMsg
from nautilus_trader.adapters.binance.futures.schemas.user import BinanceFuturesUserMsgData
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceError
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.datetime import secs_to_millis
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import order_type_to_str
from nautilus_trader.model.enums import time_in_force_to_str
from nautilus_trader.model.identifiers import ClientOrderId
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
        The base URL for the WebSocket client (unused, kept for backward compatibility).
    config : BinanceExecClientConfig
        The configuration for the client.
    account_type : BinanceAccountType, default 'USDT_FUTURES'
        The account type for the client.
    name : str, optional
        The custom client ID.
    environment : BinanceEnvironment
        The resolved Binance environment.
    api_key : str
        The Binance API key.
    api_secret : str
        The Binance API secret.

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
        account_type: BinanceAccountType = BinanceAccountType.USDT_FUTURES,
        name: str | None = None,
        *,
        environment: BinanceEnvironment,
        api_key: str,
        api_secret: str,
    ) -> None:
        PyCondition.is_true(
            account_type.is_futures,
            "account_type was not USDT_FUTURES or COIN_FUTURES",
        )

        # Futures HTTP API
        self._futures_http_account = BinanceFuturesAccountHttpAPI(client, clock, account_type)
        self._futures_http_market = BinanceFuturesMarketHttpAPI(client, account_type)

        # Futures enum parser
        self._futures_enum_parser = BinanceFuturesEnumParser()

        # Instantiate common base class
        super().__init__(
            loop=loop,
            client=client,
            account=self._futures_http_account,
            market=self._futures_http_market,
            enum_parser=self._futures_enum_parser,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_url_ws=base_url_ws,
            name=name,
            config=config,
            environment=environment,
            api_key=api_key,
            api_secret=api_secret,
        )

        # Register additional futures websocket user data event handlers
        self._futures_user_ws_handlers = {
            BinanceFuturesEventType.ACCOUNT_UPDATE: self._handle_account_update,
            BinanceFuturesEventType.ORDER_TRADE_UPDATE: self._handle_order_trade_update,
            BinanceFuturesEventType.MARGIN_CALL: self._handle_margin_call,
            BinanceFuturesEventType.ACCOUNT_CONFIG_UPDATE: self._handle_account_config_update,
            BinanceFuturesEventType.LISTEN_KEY_EXPIRED: self._handle_listen_key_expired,
            BinanceFuturesEventType.TRADE_LITE: self._handle_trade_lite,
            BinanceFuturesEventType.ALGO_UPDATE: self._handle_algo_update,
        }

        self._use_trade_lite = config.use_trade_lite
        if self._use_trade_lite:
            self._log.info("TRADE_LITE events will be used", LogColor.BLUE)

        self._leverages = config.futures_leverages
        self._margin_types = config.futures_margin_types

        self._decoder_futures_user_msg = msgspec.json.Decoder(BinanceFuturesUserMsgData)
        self._decoder_futures_order_update = msgspec.json.Decoder(BinanceFuturesOrderUpdateMsg)
        self._decoder_futures_account_update = msgspec.json.Decoder(BinanceFuturesAccountUpdateMsg)
        self._decoder_futures_trade_lite = msgspec.json.Decoder(BinanceFuturesTradeLiteMsg)
        self._decoder_futures_algo_update = msgspec.json.Decoder(BinanceFuturesAlgoUpdateMsg)

    async def _update_account_state(self) -> None:
        account_info: BinanceFuturesAccountInfo = (
            await self._futures_http_account.query_futures_account_info(recv_window=str(5000))
        )
        if account_info.canTrade:
            self._log.info("Binance API key authenticated", LogColor.GREEN)
            self._log.info(f"API key {self._http_client.api_key_masked} has trading permissions")
        else:
            self._log.error("Binance API key does not have trading permissions")
        self.generate_account_state(
            balances=account_info.parse_to_account_balances(),
            margins=account_info.parse_to_margin_balances(),
            reported=True,
            ts_event=millis_to_nanos(account_info.updateTime),
            info={
                "total_wallet_balance": account_info.totalWalletBalance,
                "total_margin_balance": account_info.totalMarginBalance,
                "total_initial_margin": account_info.totalInitialMargin,
                "total_maint_margin": account_info.totalMaintMargin,
                "total_unrealized_profit": account_info.totalUnrealizedProfit,
                "total_cross_wallet_balance": account_info.totalCrossWalletBalance,
                "total_cross_unpnl": account_info.totalCrossUnPnl,
                "available_balance": account_info.availableBalance,
                "max_withdraw_amount": account_info.maxWithdrawAmount,
            },
        )

        await self._await_account_registered(log_registered=False)

        if self._leverages:
            async with TaskGroup() as tg:
                leverage_tasks = [
                    tg.create_task(self._futures_http_account.set_leverage(symbol, leverage))
                    for symbol, leverage in self._leverages.items()
                ]
            for task in leverage_tasks:
                res: BinanceFuturesLeverage = task.result()
                self._log.info(f"Set default leverage {res.symbol} {res.leverage}X")

        if self._margin_types:
            async with TaskGroup() as tg:
                margin_tasks = [
                    (
                        tg.create_task(self._futures_http_account.set_margin_type(symbol, type_)),
                        symbol,
                        type_,
                    )
                    for symbol, type_ in self._margin_types.items()
                ]
            for _, symbol, type_ in margin_tasks:
                self._log.info(f"Set {symbol} margin type to {type_.value}")

        # Initialize leverage for all symbols using symbolConfig endpoint
        # This ensures leverage is set correctly even for symbols without active positions
        account: MarginAccount = self.get_account()
        symbol_configs = await self._futures_http_account.query_futures_symbol_config()
        for config in symbol_configs:
            try:
                instrument_id: InstrumentId = self._get_cached_instrument_id(config.symbol)
                leverage = Decimal(config.leverage)
                account.set_leverage(instrument_id, leverage)
                self._log.debug(f"Set leverage {config.symbol} {leverage}X")
            except KeyError:
                # Symbol not loaded in instrument provider, skip
                continue

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

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        report = await super().generate_order_status_report(command)
        if report is not None:
            return report

        client_order_id = command.client_order_id.value if command.client_order_id else None
        venue_order_id = int(command.venue_order_id.value) if command.venue_order_id else None

        if client_order_id is None and venue_order_id is None:
            return None

        try:
            algo_order = await self._futures_http_account.query_algo_order(
                algo_id=venue_order_id,
                client_algo_id=client_order_id,
            )
        except BinanceError as e:
            self._log.debug(f"Algo order query also failed: {e.message}")
            return None

        if algo_order is None:
            return None

        try:
            report = algo_order.parse_to_order_status_report(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(algo_order.symbol),
                report_id=UUID4(),
                enum_parser=self._futures_enum_parser,
                ts_init=self._clock.timestamp_ns(),
            )
        except ValueError as e:
            self._log.warning(f"Cannot parse algo order: {e}")
            return None

        self._log.debug(f"Received algo {report}")
        return report

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        reports = await super().generate_order_status_reports(command)

        # Reuses cached result from base class call above
        symbol = command.instrument_id.symbol.value if command.instrument_id is not None else None
        active_symbols, _ = await self._build_active_symbols(symbol)

        start_ms = secs_to_millis(command.start.timestamp()) if command.start else None
        end_ms = secs_to_millis(command.end.timestamp()) if command.end else None

        try:
            algo_reports = await self._generate_algo_order_status_reports(
                symbol=symbol,
                active_symbols=active_symbols,
                open_only=command.open_only,
                start_ms=start_ms,
                end_ms=end_ms,
            )
            reports.extend(algo_reports)
        except BinanceError as e:
            self._log.warning(f"Cannot generate algo OrderStatusReports: {e.message}")
        finally:
            self._active_symbols_cache = None

        return reports

    async def _generate_algo_order_status_reports(
        self,
        symbol: str | None,
        active_symbols: set[str],
        open_only: bool,
        start_ms: int | None,
        end_ms: int | None,
    ) -> list[OrderStatusReport]:
        algo_orders, seen_algo_ids = await self._fetch_algo_orders(symbol)
        self._log.debug(f"Fetched {len(algo_orders)} open algo orders")

        # For historical mode, also fetch from allAlgoOrders (limited to 7 days by Binance)
        if not open_only:
            for active_symbol in active_symbols:
                response = await self._futures_http_account.query_all_algo_orders(
                    symbol=active_symbol,
                    start_time=start_ms,
                    end_time=end_ms,
                    limit=1000,
                )
                # Deduplicate - open orders may appear in both endpoints
                for order in response:
                    if order.algoId not in seen_algo_ids:
                        algo_orders.append(order)
                        seen_algo_ids.add(order.algoId)
            self._log.debug(f"Total {len(algo_orders)} algo orders after historical merge")

        reports: list[OrderStatusReport] = []
        for algo_order in algo_orders:
            report = self._parse_algo_order_report(algo_order, start_ms, end_ms)
            if report is not None:
                reports.append(report)

        return reports

    async def _fetch_algo_orders(
        self,
        symbol: str | None,
    ) -> tuple[list[BinanceFuturesAlgoOrder], set[int]]:
        algo_orders: list[BinanceFuturesAlgoOrder] = []
        seen_algo_ids: set[int] = set()

        # The openAlgoOrders endpoint has no time limit, unlike allAlgoOrders (7-day limit)
        open_orders = await self._futures_http_account.query_open_algo_orders(symbol)
        for order in open_orders:
            algo_orders.append(order)
            seen_algo_ids.add(order.algoId)

        return algo_orders, seen_algo_ids

    def _parse_algo_order_report(
        self,
        algo_order: BinanceFuturesAlgoOrder,
        start_ms: int | None,
        end_ms: int | None,
    ) -> OrderStatusReport | None:
        if start_ms is not None and algo_order.createTime < start_ms:
            return None
        if end_ms is not None and algo_order.createTime > end_ms:
            return None

        # Skip triggered algo orders - the regular orders API provides accurate
        # fill data for these
        if algo_order.actualOrderId:
            if algo_order.clientAlgoId:
                self._triggered_algo_order_ids.add(ClientOrderId(algo_order.clientAlgoId))
            self._log.debug(
                f"Skipping triggered algo order {algo_order.clientAlgoId} "
                f"(actualOrderId={algo_order.actualOrderId}) - using regular order data",
            )
            return None

        try:
            report = algo_order.parse_to_order_status_report(
                account_id=self.account_id,
                instrument_id=self._get_cached_instrument_id(algo_order.symbol),
                report_id=UUID4(),
                enum_parser=self._futures_enum_parser,
                ts_init=self._clock.timestamp_ns(),
            )
        except ValueError as e:
            self._log.warning(f"Skipping algo order during reconciliation: {e}")
            return None

        self._log.debug(f"Received algo {report}")
        return report

    def _check_order_validity(self, order: Order) -> str | None:
        # Check order type valid
        if order.order_type not in self._futures_enum_parser.futures_valid_order_types:
            valid_types = [
                order_type_to_str(t) for t in self._futures_enum_parser.futures_valid_order_types
            ]
            return (
                f"UNSUPPORTED_ORDER_TYPE: {order_type_to_str(order.order_type)} "
                f"not supported for FUTURES accounts (valid: {valid_types})"
            )

        # Check time in force valid
        if order.time_in_force not in self._futures_enum_parser.futures_valid_time_in_force:
            valid_tifs = [
                time_in_force_to_str(t)
                for t in self._futures_enum_parser.futures_valid_time_in_force
            ]
            return (
                f"UNSUPPORTED_TIME_IN_FORCE: {time_in_force_to_str(order.time_in_force)} "
                f"not supported for FUTURES accounts (valid: {valid_tifs})"
            )

        # Check post-only
        if order.is_post_only and order.order_type != OrderType.LIMIT:
            return (
                f"UNSUPPORTED_POST_ONLY: {order_type_to_str(order.order_type)} post_only order "
                "not supported (only LIMIT post_only orders supported for FUTURES accounts)"
            )

        return None

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        valid_cancels = self._filter_valid_cancels(command.cancels)
        if not valid_cancels:
            self._log.info("No valid orders to cancel in batch")
            return

        successful_cancels, failed_cancels = await self._process_cancel_batches(
            valid_cancels,
            command.instrument_id.symbol.value,
        )

        self._log.info(
            f"Batch cancel completed: {len(successful_cancels)} successful, "
            f"{len(failed_cancels)} failed out of {len(valid_cancels)} valid orders",
        )

    async def _cancel_orders_for_strategy(
        self,
        orders: list[Order],
        command: CancelAllOrders,
    ) -> None:
        if not orders:
            return

        # Split into algo orders (need individual cancel) vs regular orders (can batch)
        algo_orders: list[Order] = []
        regular_orders: list[Order] = []

        for order in orders:
            if order.order_type in BINANCE_FUTURES_ALGO_ORDER_TYPES:
                # Triggered algo orders become regular orders
                if order.client_order_id in self._triggered_algo_order_ids:
                    regular_orders.append(order)
                else:
                    algo_orders.append(order)
            else:
                regular_orders.append(order)

        # Algo orders must use individual cancel (routes to algo endpoint via _cancel_order_single)
        if algo_orders:
            await self._cancel_orders_individual(algo_orders)

        if regular_orders:
            cancels = [
                CancelOrder(
                    trader_id=command.trader_id,
                    strategy_id=command.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    command_id=UUID4(),
                    ts_init=command.ts_init,
                )
                for order in regular_orders
            ]

            valid_cancels = self._filter_valid_cancels(cancels)
            if valid_cancels:
                successful_cancels, failed_cancels = await self._process_cancel_batches(
                    valid_cancels,
                    command.instrument_id.symbol.value,
                )
                self._log.info(
                    f"Strategy cancel completed: {len(successful_cancels)} successful, "
                    f"{len(failed_cancels)} failed out of {len(valid_cancels)} valid orders",
                )

    def _filter_valid_cancels(self, cancels: list[CancelOrder]) -> list[CancelOrder]:
        # Filter out orders that are already closed or not found
        valid_cancels = []
        for cancel in cancels:
            order = self._cache.order(cancel.client_order_id)
            if order is None:
                # Note: Following single cancel behavior - log error but don't emit cancel rejected event
                # for orders not found in cache (may have been cancelled/filled via other means)
                self._log.error(f"{cancel.client_order_id!r} not found to cancel")
                continue

            if order.is_closed:
                self._log.warning(
                    f"BatchCancelOrders command for {cancel.client_order_id!r} when order already {order.status_string()} "
                    "(will not send to exchange)",
                )
                continue

            valid_cancels.append(cancel)
        return valid_cancels

    async def _process_cancel_batches(
        self,
        valid_cancels: list[CancelOrder],
        symbol: str,
    ) -> tuple[list[CancelOrder], list[CancelOrder]]:
        # Process cancel orders in batches of 10 (Binance API limit)
        batch_size = 10
        batches = [
            valid_cancels[i : i + batch_size] for i in range(0, len(valid_cancels), batch_size)
        ]

        # Process all batches concurrently for better latency
        batch_results = await asyncio.gather(
            *[self._cancel_order_batch(batch, symbol) for batch in batches],
            return_exceptions=True,
        )

        successful_cancels: list[CancelOrder] = []
        failed_cancels: list[CancelOrder] = []

        for result in batch_results:
            if isinstance(result, Exception):
                # If a batch failed with an exception, treat all orders in that batch as failed
                # This shouldn't normally happen as exceptions are handled in _cancel_order_batch
                self._log.error(f"Unexpected batch processing exception: {result}")
                continue

            success, failure = result  # type: ignore[misc]
            successful_cancels.extend(success)
            failed_cancels.extend(failure)

        return successful_cancels, failed_cancels

    async def _cancel_order_batch(
        self,
        batch: list[CancelOrder],
        symbol: str,
    ) -> tuple[list[CancelOrder], list[CancelOrder]]:
        batch_client_order_ids = [c.client_order_id.value for c in batch]
        self._log.debug(
            f"Attempting to cancel batch of {len(batch)} orders: {batch_client_order_ids}",
        )

        retry_manager = await self._retry_manager_pool.acquire()
        try:
            await retry_manager.run(
                "cancel_multiple_orders",
                batch_client_order_ids,
                self._futures_http_account.cancel_multiple_orders,
                symbol=symbol,
                client_order_ids=batch_client_order_ids,
            )

            if retry_manager.result:
                self._log.debug(f"Successfully cancelled batch: {batch_client_order_ids}")
                return batch, []
            else:
                self._log.error(
                    f"Failed to cancel batch: {batch_client_order_ids}, reason: {retry_manager.message}",
                )
                self._generate_cancel_rejected_events(batch, retry_manager.message)
                return [], batch

        except BinanceError as e:
            error_code = BinanceErrorCode(int(e.message["code"]))
            if error_code == BinanceErrorCode.CANCEL_REJECTED:
                self._log.warning(f"Cancel batch rejected: {e.message}")
            else:
                self._log.exception(f"Cannot cancel batch of orders: {e.message}", e)

            self._generate_cancel_rejected_events(batch, f"Batch cancel failed: {e.message}")
            return [], batch
        finally:
            await self._retry_manager_pool.release(retry_manager)

    def _generate_cancel_rejected_events(self, cancels: list[CancelOrder], reason: str) -> None:
        for cancel in cancels:
            self.generate_order_cancel_rejected(
                cancel.strategy_id,
                cancel.instrument_id,
                cancel.client_order_id,
                cancel.venue_order_id,
                reason,
                self._clock.timestamp_ns(),
            )

    def _handle_user_ws_message(self, raw: bytes) -> None:
        try:
            msg = self._decoder_futures_user_msg.decode(raw)
            self._futures_user_ws_handlers[msg.e](raw)
        except Exception as e:
            self._log.exception(f"Error on handling {raw!r}", e)

    def _handle_account_update(self, raw: bytes) -> None:
        account_update = self._decoder_futures_account_update.decode(raw)
        account_update.handle_account_update(self)

    def _handle_order_trade_update(self, raw: bytes) -> None:
        order_update = self._decoder_futures_order_update.decode(raw)
        if not (self._use_trade_lite and order_update.o.x == BinanceExecutionType.TRADE):
            order_update.o.handle_order_trade_update(self)

    def _handle_margin_call(self, raw: bytes) -> None:
        self._log.warning("MARGIN CALL received")  # Implement

    def _handle_account_config_update(self, raw: bytes) -> None:
        self._log.info("Account config updated", LogColor.BLUE)  # Implement

    def _handle_listen_key_expired(self, raw: bytes) -> None:
        self._log.warning("Listen key expired")

    def _handle_trade_lite(self, raw: bytes) -> None:
        trade_lite = self._decoder_futures_trade_lite.decode(raw)
        if not self._use_trade_lite:
            self._log.debug(
                "TradeLite event received but not enabled in config",
            )
            return
        order_data = trade_lite.to_order_data()
        order_data.handle_order_trade_update(self)

    def _handle_algo_update(self, raw: bytes) -> None:
        algo_update = self._decoder_futures_algo_update.decode(raw)
        order_data = algo_update.o
        self._log.debug(
            f"ALGO_UPDATE received: caid={order_data.caid}, aid={order_data.aid}, "
            f"status={order_data.X}, symbol={order_data.s}",
        )
        ts_event = millis_to_nanos(algo_update.T)
        order_data.handle_algo_update(self, ts_event)
