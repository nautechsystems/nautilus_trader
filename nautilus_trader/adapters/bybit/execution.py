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
"""
Bybit execution client implementation.

This module provides a LiveExecutionClient that interfaces with Bybit's REST and
WebSocket APIs for order management and execution. The client uses Rust-based HTTP and
WebSocket clients exposed via PyO3 for performance.

"""

import asyncio
import contextlib
from asyncio import Queue
from decimal import Decimal
from typing import Any

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.config import _resolve_environment
from nautilus_trader.adapters.bybit.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.providers import BybitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.core.nautilus_pyo3 import BybitAccountType
from nautilus_trader.core.nautilus_pyo3 import BybitMarginAction
from nautilus_trader.core.nautilus_pyo3 import BybitPositionIdx
from nautilus_trader.core.nautilus_pyo3 import BybitPositionMode
from nautilus_trader.core.nautilus_pyo3 import BybitProductType
from nautilus_trader.execution.messages import BatchCancelOrders
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
from nautilus_trader.live.enqueue import ThrottledEnqueuer
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.functions import order_side_to_pyo3
from nautilus_trader.model.functions import order_type_to_pyo3
from nautilus_trader.model.functions import time_in_force_to_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


class BybitExecutionClient(LiveExecutionClient):
    """
    Execution client for Bybit exchange.

    Provides order management and execution via Bybit's REST and WebSocket APIs.
    Supports both HTTP and WebSocket-based order submission.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.BybitHttpClient
        The Bybit HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BybitInstrumentProvider
        The instrument provider.
    config : BybitExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.BybitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BybitInstrumentProvider,
        config: BybitExecClientConfig,
        name: str | None,
    ) -> None:
        # None = all product types
        product_types = config.product_types or (
            BybitProductType.SPOT,
            BybitProductType.LINEAR,
            BybitProductType.INVERSE,
            BybitProductType.OPTION,
        )

        if set(product_types) == {BybitProductType.SPOT}:
            self._account_type = AccountType.CASH
            AccountFactory.register_cash_borrowing(BYBIT_VENUE.value)
        else:
            # UTA (Unified Trading Account) for derivatives or mixed products
            self._account_type = AccountType.MARGIN

        super().__init__(
            loop=loop,
            client_id=ClientId(name or BYBIT_VENUE.value),
            venue=BYBIT_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=self._account_type,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Configuration
        self._config = config
        self._product_types = list(product_types)
        environment = _resolve_environment(config.environment, config.demo, config.testnet)
        self._is_demo = environment == nautilus_pyo3.BybitEnvironment.DEMO
        self._use_gtd = config.use_gtd
        self._use_ws_execution_fast = config.use_ws_execution_fast
        self._use_http_batch_api = config.use_http_batch_api
        self._futures_leverages = config.futures_leverages
        self._margin_mode = config.margin_mode
        self._position_mode = config.position_mode
        self._use_spot_position_reports = config.use_spot_position_reports
        self._ignore_uncached_instrument_executions = config.ignore_uncached_instrument_executions

        self._log.info(f"Account type: {self._account_type.name}", LogColor.BLUE)
        self._log.info(f"Product types: {[str(p) for p in self._product_types]}", LogColor.BLUE)
        self._log.info(f"{config.demo=}", LogColor.BLUE)
        self._log.info(f"{config.testnet=}", LogColor.BLUE)
        self._log.info(f"{config.use_gtd=}", LogColor.BLUE)
        self._log.info(f"{config.use_ws_execution_fast=}", LogColor.BLUE)
        self._log.info(f"{config.use_http_batch_api=}", LogColor.BLUE)
        self._log.info(f"{config.use_spot_position_reports=}", LogColor.BLUE)
        self._log.info(f"{config.ignore_uncached_instrument_executions=}", LogColor.BLUE)
        self._log.info(f"{config.ws_trade_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.proxy_url=}", LogColor.BLUE)

        # Set account ID
        account_id = AccountId(f"{name or BYBIT_VENUE.value}-UNIFIED")
        self._set_account_id(account_id)

        # Create pyo3 account ID for Rust HTTP client
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        # HTTP API
        self._http_client = client
        masked_key = self._http_client.api_key_masked
        self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

        # Configure HTTP client settings
        self._http_client.set_use_spot_position_reports(self._use_spot_position_reports)

        environment = _resolve_environment(config.environment, config.demo, config.testnet)

        # WebSocket API - Private channel
        self._ws_private_client = nautilus_pyo3.BybitWebSocketClient.new_private(
            environment=environment,
            api_key=config.api_key,
            api_secret=config.api_secret,
            url=config.base_url_ws_private,
            heartbeat=20,
            proxy_url=config.proxy_url,
        )

        self._ws_trade_client: nautilus_pyo3.BybitWebSocketClient = (
            nautilus_pyo3.BybitWebSocketClient.new_trade(
                environment=environment,
                api_key=config.api_key,
                api_secret=config.api_secret,
                url=config.base_url_ws_trade,
                heartbeat=20,
                proxy_url=config.proxy_url,
            )
        )
        self._ws_client_futures: set[asyncio.Future] = set()

        # Hot cache for accumulating spot borrow fills (only)
        self._order_filled_qty: dict[ClientOrderId, Quantity] = {}

        # Repayment queue system: one queue per base currency
        self._repay_queues: dict[str, Queue[Decimal]] = {}
        self._repay_enqueuers: dict[str, ThrottledEnqueuer[Decimal]] = {}
        self._repay_queue_interval_secs = config.repay_queue_interval_secs

        # Start repayment processor coroutine
        self._repay_task = loop.create_task(
            self._process_repayment_queues(),
            name="repay_processor",
        )

    @property
    def bybit_instrument_provider(self) -> BybitInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._cache_instruments()
        await self._update_account_state()
        await self._await_account_registered()

        try:
            details = await self._http_client.get_account_details()
            self._ws_trade_client.set_mm_level(details.mkt_maker_level)
        except Exception as e:
            self._log.warning(f"Error requesting account details for MM level: {e}")

        # Set account_id on WebSocket clients so they can parse account messages
        self._ws_private_client.set_account_id(self.pyo3_account_id)
        self._ws_trade_client.set_account_id(self.pyo3_account_id)

        # Connect to private WebSocket
        await self._ws_private_client.connect(loop_=self._loop, callback=self._handle_msg)

        # Wait for connection to be established
        await self._ws_private_client.wait_until_active(timeout_secs=10.0)
        self._log.info("Connected to private WebSocket", LogColor.BLUE)

        await self._ws_private_client.subscribe_orders()
        await self._ws_private_client.subscribe_executions()
        await self._ws_private_client.subscribe_positions()
        await self._ws_private_client.subscribe_wallet()

        # Connect to trade WebSocket (Bybit demo doesn't support WS Trade API)
        if self._is_demo:
            self._log.info(
                "Demo mode: Using HTTP REST API for order operations "
                "(WebSocket Trade API not supported)",
                LogColor.YELLOW,
            )
        else:
            await self._ws_trade_client.connect(loop_=self._loop, callback=self._handle_msg)
            await self._ws_trade_client.wait_until_active(timeout_secs=10.0)
            self._log.info("Connected to trade WebSocket", LogColor.BLUE)

    async def _disconnect(self) -> None:
        self._http_client.cancel_all_requests()

        # Close private WebSocket
        if not self._ws_private_client.is_closed():
            self._log.info("Disconnecting private websocket")
            await self._ws_private_client.close()

        if not self._ws_trade_client.is_closed():
            self._log.info("Disconnecting trade websocket")
            await self._ws_trade_client.close()

        # Cancel any pending futures
        for future in self._ws_client_futures:
            if not future.done():
                future.cancel()

        if self._ws_client_futures:
            try:
                await asyncio.wait_for(
                    asyncio.gather(*self._ws_client_futures, return_exceptions=True),
                    timeout=2.0,
                )
            except TimeoutError:
                self._log.warning("Timeout while waiting for websockets shutdown to complete")

        self._ws_client_futures.clear()

        # Cancel repayment processor task
        if self._repay_task and not self._repay_task.done():
            self._repay_task.cancel()
            with contextlib.suppress(asyncio.CancelledError):
                await self._repay_task

        # Cancel pending enqueuer tasks
        for enqueuer in self._repay_enqueuers.values():
            enqueuer.cancel_pending_tasks()

    async def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.bybit_instrument_provider.instruments_pyo3()

        for inst in instruments_pyo3:
            self._http_client.cache_instrument(inst)
            self._ws_private_client.cache_instrument(inst)
            self._ws_trade_client.cache_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _update_account_state(self) -> None:
        if self._account_type == AccountType.CASH:
            account_type = BybitAccountType.UNIFIED  # Spot uses unified account
        else:
            account_type = BybitAccountType.UNIFIED

        pyo3_account_state = await self._http_client.request_account_state(
            account_type=account_type,
            account_id=self.pyo3_account_id,
        )
        account_state = AccountState.from_dict(pyo3_account_state.to_dict())

        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        await self._apply_account_configuration()

    async def _apply_account_configuration(self) -> None:
        if self._futures_leverages:
            await self._apply_leverage_settings()

        if self._position_mode:
            await self._apply_position_mode_settings()

        if self._margin_mode:
            await self._apply_margin_mode_setting()

    async def _apply_leverage_settings(self) -> None:
        if self._futures_leverages is None:
            return

        tasks = []

        for symbol_str, leverage in self._futures_leverages.items():
            try:
                product_type = nautilus_pyo3.bybit_product_type_from_symbol(symbol_str)
                if product_type in (BybitProductType.LINEAR, BybitProductType.INVERSE):
                    tasks.append(self.set_leverage(symbol_str, leverage))
            except Exception as e:
                self._log.warning(f"Failed to parse symbol {symbol_str}: {e}")
        if tasks:
            await asyncio.gather(*tasks, return_exceptions=True)

    async def _apply_position_mode_settings(self) -> None:
        if self._position_mode is None:
            return

        tasks = []

        for symbol_str, mode in self._position_mode.items():
            try:
                product_type = nautilus_pyo3.bybit_product_type_from_symbol(symbol_str)
                if product_type in (BybitProductType.LINEAR, BybitProductType.INVERSE):
                    tasks.append(self.set_position_mode(symbol_str, mode))
            except Exception as e:
                self._log.warning(f"Failed to parse symbol {symbol_str}: {e}")
        if tasks:
            await asyncio.gather(*tasks, return_exceptions=True)

    async def _apply_margin_mode_setting(self) -> None:
        try:
            assert self._margin_mode is not None  # type checking
            await self._http_client.set_margin_mode(self._margin_mode)
            self._log.info(f"Set account margin mode to {self._margin_mode}")
        except Exception as e:
            error_msg = str(e).lower()
            if "not been modified" in error_msg:
                self._log.info(f"Margin mode already set to {self._margin_mode}")
            elif "needs to be equal to or greater than" in error_msg:
                self._log.warning(f"Cannot set margin mode: {e}")
            else:
                self._log.error(f"Failed to set margin mode: {e}")
                raise

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        self._log.debug(
            f"Requesting OrderStatusReports "
            f"{repr(command.instrument_id) if command.instrument_id else ''}"
            "...",
        )

        pyo3_reports: list[nautilus_pyo3.OrderStatusReport] = []
        reports: list[OrderStatusReport] = []

        try:
            # Extract instrument_id if provided
            pyo3_instrument_id = None

            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            for product_type in self._product_types:
                response = await self._http_client.request_order_status_reports(
                    account_id=self.pyo3_account_id,
                    product_type=product_type,
                    instrument_id=pyo3_instrument_id,
                    open_only=command.open_only,
                    start=ensure_pydatetime_utc(command.start),
                    end=ensure_pydatetime_utc(command.end),
                )
                pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except ValueError as e:
            if "request canceled" in str(e).lower():
                self._log.debug("OrderStatusReports request cancelled during shutdown")
            elif "symbol` must be initialized" in str(e):
                self._log.warning(
                    "Order history contains instruments not in cache - "
                    "this is expected if orders exist for uncached product types or delisted symbols. "
                    f"Cached instruments: {len(self.bybit_instrument_provider.instruments_pyo3())}",
                    LogColor.YELLOW,
                )
            else:
                self._log.exception("Failed to generate OrderStatusReports", e)
        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReports", e)

        self._log_report_receipt(
            len(reports),
            "OrderStatusReport",
            command.log_receipt_level,
        )

        return reports

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        self._log.debug(
            f"Requesting OrderStatusReport for {command.client_order_id!r}...",
        )

        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
            product_type = nautilus_pyo3.bybit_product_type_from_symbol(
                command.instrument_id.symbol.value,
            )
            pyo3_client_order_id = (
                nautilus_pyo3.ClientOrderId(command.client_order_id.value)
                if command.client_order_id
                else None
            )
            pyo3_venue_order_id = (
                nautilus_pyo3.VenueOrderId(command.venue_order_id.value)
                if command.venue_order_id
                else None
            )

            self._log.debug(
                f"About to call query_order: product_type={product_type}, "
                f"instrument_id={pyo3_instrument_id}, "
                f"client_order_id={pyo3_client_order_id}",
                LogColor.MAGENTA,
            )

            pyo3_report = await self._http_client.query_order(
                account_id=self.pyo3_account_id,
                product_type=product_type,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
            )

            self._log.debug(f"query_order returned: {pyo3_report}", LogColor.MAGENTA)

            if pyo3_report is None:
                self._log.warning(f"No order status report found for {command.client_order_id!r}")
                return None

            report = OrderStatusReport.from_pyo3(pyo3_report)
            self._log.debug(f"Received {report}", LogColor.MAGENTA)
            return report
        except ValueError as e:
            if "request canceled" in str(e).lower():
                self._log.debug("OrderStatusReport query cancelled during shutdown")
            elif "not found in cache" in str(e):
                self._log.warning(
                    f"Instrument {command.instrument_id} not in cache when querying order {command.client_order_id!r} - "
                    "order may have been placed before instruments were cached",
                    LogColor.YELLOW,
                )
            elif "must be initialized" in str(e):
                self._log.error(
                    f"PyO3 field initialization error querying order {command.client_order_id!r}: {e}. "
                    f"This may indicate an instrument caching issue for {command.instrument_id}",
                )
            else:
                self._log.exception(
                    f"Failed to generate OrderStatusReport for {command.client_order_id!r}",
                    e,
                )
            return None
        except Exception as e:
            self._log.exception(
                f"Failed to generate OrderStatusReport for {command.client_order_id!r}",
                e,
            )
            return None

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        self._log.debug(
            f"Requesting FillReports "
            f"{repr(command.instrument_id) if command.instrument_id else ''}"
            "...",
        )

        pyo3_reports: list[nautilus_pyo3.FillReport] = []
        reports: list[FillReport] = []

        try:
            for product_type in self._product_types:
                pyo3_instrument_id = None

                if command.instrument_id:
                    pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                        command.instrument_id.value,
                    )

                start_ms = None
                end_ms = None

                if command.start:
                    start_dt = ensure_pydatetime_utc(command.start)
                    if start_dt:
                        start_ms = int(start_dt.timestamp() * 1000)
                if command.end:
                    end_dt = ensure_pydatetime_utc(command.end)
                    if end_dt:
                        end_ms = int(end_dt.timestamp() * 1000)

                response = await self._http_client.request_fill_reports(
                    account_id=self.pyo3_account_id,
                    product_type=product_type,
                    instrument_id=pyo3_instrument_id,
                    start=start_ms,
                    end=end_ms,
                )
                pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "FillReports")

        self._log_report_receipt(len(reports), "FillReport", LogLevel.INFO)

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        self._log.debug("Requesting PositionStatusReports...")

        pyo3_reports: list[nautilus_pyo3.PositionStatusReport] = []
        reports: list[PositionStatusReport] = []

        try:
            pyo3_instrument_id = None
            product_types_to_query = self._product_types

            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

                try:
                    product_type = nautilus_pyo3.bybit_product_type_from_symbol(
                        command.instrument_id.symbol.value,
                    )
                    product_types_to_query = [product_type]
                except ValueError:
                    # Symbol lacks suffix, fall back to querying all configured types
                    pass

            for product_type in product_types_to_query:
                response = await self._http_client.request_position_status_reports(
                    account_id=self.pyo3_account_id,
                    product_type=product_type,
                    instrument_id=pyo3_instrument_id,
                )
                pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = PositionStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "PositionStatusReports")

        self._log_report_receipt(
            len(reports),
            "PositionStatusReport",
            command.log_receipt_level,
        )

        return reports

    async def set_leverage(
        self,
        symbol: str,
        leverage: int,
    ) -> None:
        """
        Set leverage for a symbol.

        Parameters
        ----------
        symbol : str
            The symbol string (e.g., "ETHUSDT-LINEAR").
        leverage : int
            The leverage value to set.

        """
        try:
            raw_symbol = nautilus_pyo3.bybit_extract_raw_symbol(symbol)
            product_type = nautilus_pyo3.bybit_product_type_from_symbol(symbol)

            if product_type == BybitProductType.OPTION:
                self._log.warning(
                    f"Leverage not supported for options, skipping {symbol}",
                )
                return

            await self._http_client.set_leverage(
                product_type=product_type,
                symbol=raw_symbol,
                buy_leverage=str(leverage),
                sell_leverage=str(leverage),
            )
            self._log.info(f"Set symbol `{symbol}` leverage to `{leverage}`")
        except Exception as e:
            error_msg = str(e).lower()
            # Bybit error code 110043: Set leverage has not been modified (already set)
            if "110043" in error_msg or "not been modified" in error_msg:
                self._log.info(f"Symbol `{symbol}` leverage already set to `{leverage}`")
            else:
                self._log.error(f"Failed to set leverage for {symbol}: {e}")
                raise

    async def set_position_mode(
        self,
        symbol: str,
        mode: BybitPositionMode,
    ) -> None:
        """
        Set position mode for a symbol.

        Parameters
        ----------
        symbol : str
            The symbol string (e.g., "ETHUSDT-LINEAR").
        mode : BybitPositionMode
            The position mode to set.

        """
        try:
            raw_symbol = nautilus_pyo3.bybit_extract_raw_symbol(symbol)
            product_type = nautilus_pyo3.bybit_product_type_from_symbol(symbol)

            if product_type == BybitProductType.OPTION:
                self._log.warning(
                    f"Position mode not supported for options, skipping {symbol}",
                )
                return

            await self._http_client.switch_mode(
                product_type=product_type,
                mode=mode,
                symbol=raw_symbol,
            )
            self._log.info(f"Set symbol `{symbol}` position mode to `{mode}`")
        except Exception as e:
            error_msg = str(e).lower()
            # Bybit error code 110025: Position mode has not been modified (already set)
            if "110025" in error_msg or "not been modified" in error_msg:
                self._log.info(f"Symbol `{symbol}` position mode already set to `{mode}`")
            else:
                self._log.error(f"Failed to set position mode for {symbol}: {e}")
                raise

    def _resolve_position_idx(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        is_reduce_only: bool,
        manual_override: int | None,
    ) -> BybitPositionIdx | None:
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(instrument_id.symbol.value)
        if product_type not in (BybitProductType.LINEAR, BybitProductType.INVERSE):
            return None
        mode = self._position_mode.get(instrument_id.symbol.value) if self._position_mode else None
        override = (
            BybitPositionIdx.from_str(manual_override) if manual_override is not None else None
        )
        return nautilus_pyo3.bybit_resolve_position_idx(
            position_mode=mode,
            order_side=order_side_to_pyo3(order_side),
            is_reduce_only=is_reduce_only,
            manual_override=override,
        )

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    def _check_order_validity(
        self,
        order: Order,
        product_type: BybitProductType,
    ) -> str | None:
        if order.is_post_only and order.order_type != OrderType.LIMIT:
            return "UNSUPPORTED_POST_ONLY"

        if order.is_reduce_only and product_type == BybitProductType.SPOT:
            return "UNSUPPORTED_REDUCE_ONLY_SPOT"

        return None

    async def _query_account(self, command: QueryAccount) -> None:
        params = command.params or {}
        action = params.get("action")

        if action is None:
            await self._update_account_state()
            return

        if action == BybitMarginAction.BORROW:
            await self._handle_borrow_action(command, params)
        elif action == BybitMarginAction.REPAY:
            await self._handle_repay_action(command, params)
        elif action == BybitMarginAction.GET_BORROW_AMOUNT:
            await self._handle_get_borrow_amount_action(command, params)
        else:
            self._log.warning(f"Unknown query_account action: {action}")
            await self._update_account_state()

    async def _handle_borrow_action(self, command: QueryAccount, params: dict[str, Any]) -> None:
        coin = params.get("coin")
        amount = params.get("amount")

        if not coin or not amount:
            self._log.error("Borrow action requires 'coin' and 'amount' params")
            return

        ts_now = self._clock.timestamp_ns()
        success = False
        message = ""

        try:
            pyo3_amount = nautilus_pyo3.Quantity.from_str(str(amount))
            await self._http_client.borrow_spot(coin, pyo3_amount)
            success = True
            self._log.info(f"Successfully borrowed {amount} {coin}", LogColor.GREEN)
        except Exception as e:
            message = str(e)
            self._log.error(f"Borrow failed: {e}")

        response = nautilus_pyo3.BybitMarginBorrowResult(
            coin=coin,
            amount=str(amount),
            success=success,
            message=message,
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self._publish_margin_data(response)

    async def _handle_repay_action(self, command: QueryAccount, params: dict[str, Any]) -> None:
        coin = params.get("coin")
        amount = params.get("amount")  # Optional - None means repay all

        if not coin:
            self._log.error("Repay action requires 'coin' param")
            return

        # Check Bybit blackout window (04:00-05:30 UTC)
        if self._is_repay_blackout_window():
            self._log.warning(
                "Cannot repay during Bybit blackout window (04:00-05:30 UTC)",
                LogColor.YELLOW,
            )
            return

        ts_now = self._clock.timestamp_ns()
        success = False
        result_status = "FAIL"
        message = ""

        try:
            pyo3_amount = nautilus_pyo3.Quantity.from_str(str(amount)) if amount else None
            await self._http_client.repay_spot_borrow(coin, pyo3_amount)
            success = True
            result_status = "SU"
            self._log.info(f"Successfully repaid {amount or 'all'} {coin}", LogColor.GREEN)
        except Exception as e:
            message = str(e)
            self._log.error(f"Repay failed: {e}")

        response = nautilus_pyo3.BybitMarginRepayResult(
            coin=coin,
            amount=str(amount) if amount else None,
            success=success,
            result_status=result_status,
            message=message,
            ts_event=ts_now,
            ts_init=ts_now,
        )
        self._publish_margin_data(response)

    async def _handle_get_borrow_amount_action(
        self,
        command: QueryAccount,
        params: dict[str, Any],
    ) -> None:
        coin = params.get("coin")

        if not coin:
            self._log.error("Get borrow amount action requires 'coin' param")
            return

        ts_now = self._clock.timestamp_ns()

        try:
            borrow_amount = await self._http_client.get_spot_borrow_amount(coin)

            response = nautilus_pyo3.BybitMarginStatusResult(
                coin=coin,
                borrow_amount=str(borrow_amount),
                ts_event=ts_now,
                ts_init=ts_now,
            )
            self._log.info(f"Borrow amount for {coin}: {borrow_amount}")
            self._publish_margin_data(response)
        except Exception as e:
            self._log.error(f"Get borrow amount failed: {e}")

    def _publish_margin_data(self, data) -> None:
        data_type = DataType(type(data))
        self._msgbus.publish(topic=f"data.{data_type.topic}", msg=data)

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            order.instrument_id.symbol.value,
        )

        if reason := self._check_order_validity(order, product_type):
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Parse and validate adapter-specific params BEFORE emitting order submitted,
        # so that bad values surface as order_denied (not order_rejected after submission).
        try:
            tp_sl = _parse_bybit_tp_sl_params(command.params)
        except ValueError as e:
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )
            return

        if self._is_demo and (
            tp_sl.get("take_profit")
            or tp_sl.get("stop_loss")
            or tp_sl.get("order_iv") is not None
            or tp_sl.get("mmp") is not None
        ):
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason="Native TP/SL and option params are not supported in demo mode",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Generate OrderSubmitted event
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        pyo3_trader_id = nautilus_pyo3.TraderId(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_time_in_force = (
            time_in_force_to_pyo3(order.time_in_force) if order.time_in_force else None
        )
        pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None

        pyo3_trigger_price = None

        if order.has_trigger_price:
            pyo3_trigger_price = nautilus_pyo3.Price.from_str(str(order.trigger_price))

        is_leverage = tp_sl["is_leverage"]
        is_quote_quantity = (
            order.is_quote_quantity if hasattr(order, "is_quote_quantity") else False
        )
        position_idx = self._resolve_position_idx(
            order.instrument_id,
            order.side,
            order.is_reduce_only,
            tp_sl.get("position_idx"),
        )
        try:
            if self._is_demo:
                await self._http_client.submit_order(
                    account_id=self.pyo3_account_id,
                    product_type=product_type,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    order_side=pyo3_order_side,
                    order_type=pyo3_order_type,
                    quantity=pyo3_quantity,
                    time_in_force=pyo3_time_in_force,
                    price=pyo3_price,
                    trigger_price=pyo3_trigger_price,
                    post_only=order.is_post_only,
                    reduce_only=order.is_reduce_only,
                    is_quote_quantity=is_quote_quantity,
                    is_leverage=is_leverage,
                    position_idx=position_idx,
                )
            elif (
                tp_sl.get("take_profit")
                or tp_sl.get("stop_loss")
                or tp_sl.get("order_iv") is not None
                or tp_sl.get("mmp") is not None
            ):
                # Batch path: required for native TP/SL and option-specific fields
                # (order_iv, mmp) that the simple submit_order API does not accept.
                pyo3_take_profit = (
                    nautilus_pyo3.Price.from_str(tp_sl["take_profit"])
                    if tp_sl.get("take_profit")
                    else None
                )
                pyo3_stop_loss = (
                    nautilus_pyo3.Price.from_str(tp_sl["stop_loss"])
                    if tp_sl.get("stop_loss")
                    else None
                )
                order_params = self._ws_trade_client.build_place_order_params(
                    product_type=product_type,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    order_side=pyo3_order_side,
                    order_type=pyo3_order_type,
                    quantity=pyo3_quantity,
                    is_quote_quantity=is_quote_quantity,
                    time_in_force=pyo3_time_in_force,
                    price=pyo3_price,
                    trigger_price=pyo3_trigger_price,
                    post_only=order.is_post_only,
                    reduce_only=order.is_reduce_only,
                    is_leverage=is_leverage,
                    take_profit=pyo3_take_profit,
                    stop_loss=pyo3_stop_loss,
                    position_idx=position_idx,
                )
                _apply_tp_sl_fields(order_params, tp_sl)
                await self._ws_trade_client.batch_place_orders(
                    pyo3_trader_id,
                    pyo3_strategy_id,
                    [order_params],
                )
            else:
                await self._ws_trade_client.submit_order(
                    product_type=product_type,
                    trader_id=pyo3_trader_id,
                    strategy_id=pyo3_strategy_id,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    order_side=pyo3_order_side,
                    order_type=pyo3_order_type,
                    quantity=pyo3_quantity,
                    is_quote_quantity=is_quote_quantity,
                    time_in_force=pyo3_time_in_force,
                    price=pyo3_price,
                    trigger_price=pyo3_trigger_price,
                    post_only=order.is_post_only,
                    reduce_only=order.is_reduce_only,
                    is_leverage=is_leverage,
                    position_idx=position_idx,
                )
        except Exception as e:
            self._log.error(f"Failed to submit order {order.client_order_id}: {e}")
            error_msg = str(e)
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=error_msg,
                ts_event=self._clock.timestamp_ns(),
                due_post_only="EC_PostOnlyWillTakeLiquidity" in error_msg,
            )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        if not command.order_list.orders:
            return

        # Parse and validate adapter-specific params before touching any order state.
        try:
            tp_sl = _parse_bybit_tp_sl_params(command.params)
        except ValueError as e:
            now_ns = self._clock.timestamp_ns()
            for order in command.order_list.orders:
                self.generate_order_denied(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=str(e),
                    ts_event=now_ns,
                )
            return

        if self._is_demo:
            if (
                tp_sl.get("take_profit")
                or tp_sl.get("stop_loss")
                or tp_sl.get("order_iv") is not None
                or tp_sl.get("mmp") is not None
            ):
                now_ns = self._clock.timestamp_ns()
                for order in command.order_list.orders:
                    self.generate_order_denied(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        reason="Native TP/SL and option params are not supported in demo mode",
                        ts_event=now_ns,
                    )
                return
            await self._submit_order_list_http(command, tp_sl)
            return

        await self._submit_order_list_ws(command, tp_sl)

    async def _submit_order_list_http(
        self,
        command: SubmitOrderList,
        tp_sl: dict,
    ) -> None:
        is_leverage = tp_sl["is_leverage"]
        position_idx_override = tp_sl.get("position_idx")
        now_ns = self._clock.timestamp_ns()

        for order in command.order_list.orders:
            if order.is_closed:
                self._log.warning(f"Cannot submit already closed order: {order}")
                continue

            product_type = nautilus_pyo3.bybit_product_type_from_symbol(
                order.instrument_id.symbol.value,
            )

            if reason := self._check_order_validity(order, product_type):
                self.generate_order_denied(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=reason,
                    ts_event=now_ns,
                )
                continue

            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=now_ns,
            )

            pyo3_trigger_price = None

            if order.has_trigger_price:
                pyo3_trigger_price = nautilus_pyo3.Price.from_str(str(order.trigger_price))

            is_quote_quantity = (
                order.is_quote_quantity if hasattr(order, "is_quote_quantity") else False
            )

            position_idx = self._resolve_position_idx(
                order.instrument_id,
                order.side,
                order.is_reduce_only,
                position_idx_override,
            )
            try:
                await self._http_client.submit_order(
                    account_id=self.pyo3_account_id,
                    product_type=product_type,
                    instrument_id=nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value),
                    client_order_id=nautilus_pyo3.ClientOrderId(order.client_order_id.value),
                    order_side=order_side_to_pyo3(order.side),
                    order_type=order_type_to_pyo3(order.order_type),
                    quantity=nautilus_pyo3.Quantity.from_str(str(order.quantity)),
                    time_in_force=(
                        time_in_force_to_pyo3(order.time_in_force) if order.time_in_force else None
                    ),
                    price=(
                        nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
                    ),
                    trigger_price=pyo3_trigger_price,
                    post_only=order.is_post_only,
                    reduce_only=order.is_reduce_only,
                    is_quote_quantity=is_quote_quantity,
                    is_leverage=is_leverage,
                    position_idx=position_idx,
                )
            except Exception as e:
                self._log.error(f"Failed to submit order {order.client_order_id}: {e}")
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _submit_order_list_ws(
        self,
        command: SubmitOrderList,
        tp_sl: dict,
    ) -> None:
        now_ns = self._clock.timestamp_ns()
        order_list = command.order_list
        orders = order_list.orders
        order_params = []

        is_leverage = tp_sl["is_leverage"]

        for order in orders:
            if order.is_closed:
                self._log.warning(f"Cannot submit already closed order: {order}")
                continue

            product_type = nautilus_pyo3.bybit_product_type_from_symbol(
                order.instrument_id.symbol.value,
            )

            if reason := self._check_order_validity(order, product_type):
                self.generate_order_denied(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=reason,
                    ts_event=now_ns,
                )
                continue

            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=now_ns,
            )

            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
            pyo3_order_side = order_side_to_pyo3(order.side)
            pyo3_order_type = order_type_to_pyo3(order.order_type)
            pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
            pyo3_time_in_force = (
                time_in_force_to_pyo3(order.time_in_force) if order.time_in_force else None
            )
            pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None

            pyo3_trigger_price = None

            if order.has_trigger_price:
                pyo3_trigger_price = nautilus_pyo3.Price.from_str(str(order.trigger_price))

            is_quote_quantity = (
                order.is_quote_quantity if hasattr(order, "is_quote_quantity") else False
            )

            pyo3_take_profit = (
                nautilus_pyo3.Price.from_str(tp_sl["take_profit"])
                if tp_sl.get("take_profit")
                else None
            )
            pyo3_stop_loss = (
                nautilus_pyo3.Price.from_str(tp_sl["stop_loss"]) if tp_sl.get("stop_loss") else None
            )
            position_idx = self._resolve_position_idx(
                order.instrument_id,
                order.side,
                order.is_reduce_only,
                tp_sl.get("position_idx"),
            )
            ws_params = self._ws_trade_client.build_place_order_params(
                product_type=product_type,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                quantity=pyo3_quantity,
                is_quote_quantity=is_quote_quantity,
                time_in_force=pyo3_time_in_force,
                price=pyo3_price,
                trigger_price=pyo3_trigger_price,
                post_only=order.is_post_only,
                reduce_only=order.is_reduce_only,
                is_leverage=is_leverage,
                take_profit=pyo3_take_profit,
                stop_loss=pyo3_stop_loss,
                position_idx=position_idx,
            )
            _apply_tp_sl_fields(ws_params, tp_sl)
            order_params.append(ws_params)

        if order_params:
            pyo3_trader_id = nautilus_pyo3.TraderId(command.trader_id.value)
            pyo3_strategy_id = nautilus_pyo3.StrategyId(command.strategy_id.value)

            try:
                await self._ws_trade_client.batch_place_orders(
                    pyo3_trader_id,
                    pyo3_strategy_id,
                    order_params,
                )
            except Exception as e:
                self._log.error(f"Failed to batch place orders: {e}")

                for order in orders:
                    if not order.is_closed:
                        self.generate_order_rejected(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            reason=str(e),
                            ts_event=self._clock.timestamp_ns(),
                        )

    async def _modify_order(self, command: ModifyOrder) -> None:
        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`ModifyOrder` command for {command.client_order_id!r} when order already {order.status_string()} "
                "(will not send to exchange)",
            )
            return

        pyo3_trader_id = nautilus_pyo3.TraderId(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(command.client_order_id.value)
        pyo3_venue_order_id = (
            nautilus_pyo3.VenueOrderId(command.venue_order_id.value)
            if command.venue_order_id
            else None
        )
        pyo3_quantity = (
            nautilus_pyo3.Quantity.from_str(str(command.quantity)) if command.quantity else None
        )
        pyo3_price = nautilus_pyo3.Price.from_str(str(command.price)) if command.price else None

        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            command.instrument_id.symbol.value,
        )

        order_iv = None

        if command.params:
            val = command.params.get("order_iv")
            if val is not None:
                if isinstance(val, bool) or not isinstance(val, (str, int, float)):
                    self.generate_order_modify_rejected(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                        reason=f"Invalid type for 'order_iv': {type(val).__name__}, expected str or number",
                        ts_event=self._clock.timestamp_ns(),
                    )
                    return
                order_iv = str(val)

        if self._is_demo and order_iv is not None:
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason="Option params (order_iv) are not supported in demo mode",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        try:
            if self._is_demo:
                await self._http_client.modify_order(
                    account_id=self.pyo3_account_id,
                    product_type=product_type,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    venue_order_id=pyo3_venue_order_id,
                    quantity=pyo3_quantity,
                    price=pyo3_price,
                )
            elif order_iv is not None:
                amend_params = self._ws_trade_client.build_amend_order_params(
                    product_type=product_type,
                    instrument_id=pyo3_instrument_id,
                    venue_order_id=pyo3_venue_order_id,
                    client_order_id=pyo3_client_order_id,
                    quantity=pyo3_quantity,
                    price=pyo3_price,
                )
                amend_params.order_iv = order_iv
                await self._ws_trade_client.batch_modify_orders(
                    pyo3_trader_id,
                    pyo3_strategy_id,
                    [amend_params],
                )
            else:
                await self._ws_trade_client.modify_order(
                    product_type=product_type,
                    trader_id=pyo3_trader_id,
                    strategy_id=pyo3_strategy_id,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    venue_order_id=pyo3_venue_order_id,
                    quantity=pyo3_quantity,
                    price=pyo3_price,
                )
        except Exception as e:
            self._log.error(f"Failed to modify order {command.client_order_id}: {e}")
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_order(self, command: CancelOrder) -> None:
        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`CancelOrder` command for {command.client_order_id!r} when order already {order.status_string()} "
                "(will not send to exchange)",
            )
            return

        pyo3_trader_id = nautilus_pyo3.TraderId(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(command.client_order_id.value)
        pyo3_venue_order_id = (
            nautilus_pyo3.VenueOrderId(command.venue_order_id.value)
            if command.venue_order_id
            else None
        )

        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            command.instrument_id.symbol.value,
        )

        try:
            if self._is_demo:
                await self._http_client.cancel_order(
                    account_id=self.pyo3_account_id,
                    product_type=product_type,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    venue_order_id=pyo3_venue_order_id,
                )
            else:
                await self._ws_trade_client.cancel_order(
                    product_type=product_type,
                    trader_id=pyo3_trader_id,
                    strategy_id=pyo3_strategy_id,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    venue_order_id=pyo3_venue_order_id,
                )
        except Exception as e:
            self._log.error(f"Failed to cancel order {command.client_order_id}: {e}")
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        if command.order_side != OrderSide.NO_ORDER_SIDE:
            self._log.warning(
                f"Bybit does not support order_side filtering for cancel all orders; "
                f"ignoring order_side={order_side_to_str(command.order_side)} and canceling all orders",
            )

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            command.instrument_id.symbol.value,
        )

        try:
            reports = await self._http_client.cancel_all_orders(
                account_id=self.pyo3_account_id,
                product_type=product_type,
                instrument_id=pyo3_instrument_id,
            )

            for pyo3_report in reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Cancelled order: {report}", LogColor.MAGENTA)
        except Exception as e:
            self._log.error(f"Failed to cancel all orders for {command.instrument_id}: {e}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:  # noqa: C901
        if not command.cancels:
            return

        # Derive product type from first cancel (all must be same product type for batch operation)
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            command.cancels[0].instrument_id.symbol.value,
        )

        if self._is_demo:
            # Cancel individually (batch not supported in demo)
            for cancel in command.cancels:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    cancel.instrument_id.value,
                )
                pyo3_client_order_id = (
                    nautilus_pyo3.ClientOrderId(cancel.client_order_id.value)
                    if cancel.client_order_id
                    else None
                )
                pyo3_venue_order_id = (
                    nautilus_pyo3.VenueOrderId(cancel.venue_order_id.value)
                    if cancel.venue_order_id
                    else None
                )

                try:
                    await self._http_client.cancel_order(
                        account_id=self.pyo3_account_id,
                        product_type=product_type,
                        instrument_id=pyo3_instrument_id,
                        client_order_id=pyo3_client_order_id,
                        venue_order_id=pyo3_venue_order_id,
                    )
                except Exception as e:
                    self._log.error(f"Failed to cancel order {cancel.client_order_id}: {e}")
                    order = self._cache.order(cancel.client_order_id)
                    if order and not order.is_closed:
                        self.generate_order_cancel_rejected(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            venue_order_id=order.venue_order_id,
                            reason=str(e),
                            ts_event=self._clock.timestamp_ns(),
                        )
            return

        order_params = []

        for cancel in command.cancels:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(cancel.instrument_id.value)
            pyo3_client_order_id = (
                nautilus_pyo3.ClientOrderId(cancel.client_order_id.value)
                if cancel.client_order_id
                else None
            )
            pyo3_venue_order_id = (
                nautilus_pyo3.VenueOrderId(cancel.venue_order_id.value)
                if cancel.venue_order_id
                else None
            )

            params = self._ws_trade_client.build_cancel_order_params(
                product_type=product_type,
                instrument_id=pyo3_instrument_id,
                venue_order_id=pyo3_venue_order_id,
                client_order_id=pyo3_client_order_id,
            )
            order_params.append(params)

        if order_params:
            pyo3_trader_id = nautilus_pyo3.TraderId(command.trader_id.value)
            pyo3_strategy_id = nautilus_pyo3.StrategyId(command.strategy_id.value)

            try:
                await self._ws_trade_client.batch_cancel_orders(
                    pyo3_trader_id,
                    pyo3_strategy_id,
                    order_params,
                )
            except Exception as e:
                self._log.error(f"Failed to batch cancel orders: {e}")
                for cancel in command.cancels:
                    order = self._cache.order(cancel.client_order_id)
                    if order and not order.is_closed:
                        self.generate_order_cancel_rejected(
                            strategy_id=order.strategy_id,
                            instrument_id=order.instrument_id,
                            client_order_id=order.client_order_id,
                            venue_order_id=order.venue_order_id,
                            reason=str(e),
                            ts_event=self._clock.timestamp_ns(),
                        )

    # -- MESSAGE HANDLERS -------------------------------------------------------------------------

    def _handle_msg(self, msg: Any) -> None:
        if isinstance(msg, nautilus_pyo3.BybitWebSocketError):
            self._log.error(f"WebSocket error: {msg}")
        elif isinstance(msg, nautilus_pyo3.AccountState):
            self._handle_account_state(msg)
        elif isinstance(msg, nautilus_pyo3.OrderRejected):
            self._handle_order_rejected_pyo3(msg)
        elif isinstance(msg, nautilus_pyo3.OrderCancelRejected):
            self._handle_order_cancel_rejected_pyo3(msg)
        elif isinstance(msg, nautilus_pyo3.OrderModifyRejected):
            self._handle_order_modify_rejected_pyo3(msg)
        elif isinstance(msg, nautilus_pyo3.OrderStatusReport):
            self._handle_order_status_report_pyo3(msg)
        elif isinstance(msg, nautilus_pyo3.FillReport):
            self._handle_fill_report_pyo3(msg)
        elif isinstance(msg, nautilus_pyo3.PositionStatusReport):
            self._handle_position_status_report_pyo3(msg)
        elif isinstance(msg, str):
            self._log.debug(f"Received raw message: {msg}", LogColor.MAGENTA)
        else:
            self._log.warning(f"Received unhandled message type: {type(msg)}")

    def _handle_account_state(self, msg: nautilus_pyo3.AccountState) -> None:
        account_state = AccountState.from_dict(msg.to_dict())
        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=account_state.is_reported,
            ts_event=account_state.ts_event,
        )

    def _handle_order_rejected_pyo3(self, msg: nautilus_pyo3.OrderRejected) -> None:
        event = OrderRejected.from_dict(msg.to_dict())
        self._send_order_event(event)

    def _handle_order_cancel_rejected_pyo3(self, msg: nautilus_pyo3.OrderCancelRejected) -> None:
        event = OrderCancelRejected.from_dict(msg.to_dict())
        self._send_order_event(event)

    def _handle_order_modify_rejected_pyo3(self, msg: nautilus_pyo3.OrderModifyRejected) -> None:
        event = OrderModifyRejected.from_dict(msg.to_dict())
        self._send_order_event(event)

    def _handle_order_status_report_pyo3(  # noqa: C901 (too complex)
        self,
        pyo3_report: nautilus_pyo3.OrderStatusReport,
    ) -> None:
        report = OrderStatusReport.from_pyo3(pyo3_report)

        if self._is_external_order(report.client_order_id):
            self._send_order_status_report(report)
            return

        order = self._cache.order(report.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process order status report - order for {report.client_order_id!r} not found",
            )
            return

        if order.linked_order_ids is not None:
            report.linked_order_ids = list(order.linked_order_ids)

        if report.order_status == OrderStatus.REJECTED:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                reason=report.cancel_reason or "Order rejected by exchange",
                ts_event=report.ts_last,
            )
            self._order_filled_qty.pop(report.client_order_id, None)
        elif report.order_status == OrderStatus.ACCEPTED:
            if report.is_order_updated(order):
                self.generate_order_updated(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    quantity=report.quantity,
                    price=report.price,
                    trigger_price=report.trigger_price,
                    ts_event=report.ts_last,
                )
            else:
                self.generate_order_accepted(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    ts_event=report.ts_last,
                )
        elif report.order_status == OrderStatus.PENDING_CANCEL:
            if order.status == OrderStatus.PENDING_CANCEL:
                self._log.debug(
                    f"Received PENDING_CANCEL status for {report.client_order_id!r} - "
                    "order already in pending cancel state locally",
                )
            else:
                self._log.warning(
                    f"Received PENDING_CANCEL status for {report.client_order_id!r} - "
                    f"order status {order.status_string()}",
                )
        elif report.order_status == OrderStatus.CANCELED:
            # Check if this is a post-only order rejected by the exchange
            # Bybit accepts post-only orders initially then immediately cancels them with
            # rejectReason="EC_PostOnlyWillTakeLiquidity" if they would cross the spread
            is_post_only_rejection = (
                report.cancel_reason and "EC_PostOnlyWillTakeLiquidity" in report.cancel_reason
            )

            if is_post_only_rejection:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    reason=report.cancel_reason,
                    ts_event=report.ts_last,
                    due_post_only=True,
                )
            else:
                self.generate_order_canceled(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    ts_event=report.ts_last,
                )
            self._order_filled_qty.pop(report.client_order_id, None)
        elif report.order_status == OrderStatus.EXPIRED:
            self.generate_order_expired(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
            self._order_filled_qty.pop(report.client_order_id, None)
        elif report.order_status == OrderStatus.TRIGGERED:
            if order.order_type in (
                OrderType.STOP_LIMIT,
                OrderType.TRAILING_STOP_LIMIT,
                OrderType.LIMIT_IF_TOUCHED,
            ):
                self.generate_order_triggered(
                    strategy_id=order.strategy_id,
                    instrument_id=report.instrument_id,
                    client_order_id=report.client_order_id,
                    venue_order_id=report.venue_order_id,
                    ts_event=report.ts_last,
                )
        else:
            # Fills should be handled from FillReports
            self._log.debug(f"Received unhandled OrderStatusReport: {report}")

    def _handle_fill_report_pyo3(self, pyo3_report: nautilus_pyo3.FillReport) -> None:
        report = FillReport.from_pyo3(pyo3_report)

        if self._is_external_order(report.client_order_id):
            self._send_fill_report(report)
            return

        order = self._cache.order(report.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process fill report - order for {report.client_order_id!r} not found",
            )
            return

        instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            if self._ignore_uncached_instrument_executions:
                self._log.warning(
                    f"Ignoring fill report for uncached instrument {order.instrument_id}",
                    LogColor.YELLOW,
                )
                return
            self._log.error(
                f"Cannot process fill report - instrument {order.instrument_id} not found",
            )
            return

        self.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=report.venue_order_id,
            venue_position_id=report.venue_position_id,
            trade_id=report.trade_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=report.last_qty,
            last_px=report.last_px,
            quote_currency=instrument.quote_currency,
            commission=report.commission,
            liquidity_side=report.liquidity_side,
            ts_event=report.ts_event,
        )

        if self._config.auto_repay_spot_borrows and order.side == OrderSide.BUY:
            try:
                product_type = nautilus_pyo3.bybit_product_type_from_symbol(
                    order.instrument_id.symbol.value,
                )

                if product_type != BybitProductType.SPOT:
                    return

                # Get current filled quantity (from tracking or order state)
                filled_current = self._order_filled_qty.get(order.client_order_id, Quantity.zero())
                filled_new = filled_current + report.last_qty

                if filled_new >= order.quantity:
                    # Order is now fully filled: enqueue repayment and clean up tracking
                    self._order_filled_qty.pop(order.client_order_id, None)
                    base_currency = instrument.base_currency.code

                    # Ensure queue and enqueuer exist for this currency
                    if base_currency not in self._repay_queues:
                        self._repay_queues[base_currency] = Queue(maxsize=1000)
                        self._repay_enqueuers[base_currency] = ThrottledEnqueuer(
                            qname=f"repay_queue_{base_currency}",
                            queue=self._repay_queues[base_currency],
                            loop=self._loop,
                            clock=self._clock,
                            logger=self._log,
                        )

                    # Enqueue the filled quantity for repayment
                    qty_decimal = order.quantity.as_decimal()
                    self._repay_enqueuers[base_currency].enqueue(qty_decimal)
                else:
                    # Partial fill: update tracking
                    self._order_filled_qty[order.client_order_id] = filled_new
            except Exception as e:
                self._log.warning(f"Failed to enqueue spot borrow repayment: {e}")

    def _handle_position_status_report_pyo3(self, msg: nautilus_pyo3.PositionStatusReport) -> None:
        report = PositionStatusReport.from_pyo3(msg)
        self._log.debug(f"Received {report}", LogColor.MAGENTA)
        # Do not send position reports from WebSocket stream - we use HTTP endpoint for reconciliation
        # to avoid noise from position updates every time a fill occurs

    async def _process_repayment_queues(self) -> None:
        self._log.debug("Repayment queue processor starting")
        try:
            while True:
                await asyncio.sleep(self._repay_queue_interval_secs)

                for base_currency, queue in list(self._repay_queues.items()):
                    try:
                        # Accumulate all pending quantities for this currency
                        total_qty = Decimal(0)

                        while not queue.empty():
                            try:
                                qty = queue.get_nowait()
                                total_qty += qty
                            except asyncio.QueueEmpty:
                                break

                        # If we have accumulated quantity, trigger repayment
                        if total_qty > 0:
                            qty_obj = Quantity.from_str(str(total_qty))
                            await self._repay_spot_borrow_if_needed(base_currency, qty_obj)
                    except Exception as e:
                        self._log.error(
                            f"Error processing repayment queue for {base_currency}: {e}",
                        )
        except asyncio.CancelledError:
            self._log.debug("Repayment queue processor cancelled")
        except Exception as e:
            self._log.error(f"Unexpected error in repayment queue processor: {e}")
        finally:
            self._log.debug("Repayment queue processor stopped")

    async def _repay_spot_borrow_if_needed(self, coin: str, bought_qty: Quantity) -> None:
        # Repay outstanding spot borrows for a specific coin, this method is called when
        # BUY orders are fully filled on SPOT instruments to automatically repay any outstanding
        # borrows, preventing interest accrual.
        try:
            if self._is_repay_blackout_window():
                self._log.warning(
                    f"Skipping borrow repayment for {coin} due to Bybit blackout window "
                    f"(04:00-05:30 UTC daily), will need manual repayment",
                    LogColor.YELLOW,
                )
                return

            # Check if there's an outstanding borrow first
            borrow_amount = await self._http_client.get_spot_borrow_amount(coin)

            if borrow_amount == 0:
                self._log.info(f"No outstanding borrow for {coin}", LogColor.BLUE)
                return

            # Only repay up to the amount we just bought
            bought_amount = bought_qty.as_decimal()
            repay_amount = min(borrow_amount, bought_amount)

            self._log.info(
                f"Attempting to repay spot borrow for {coin} "
                f"(outstanding: {borrow_amount}, bought: {bought_amount}, repaying: {repay_amount})",
                LogColor.BLUE,
            )

            repay_qty = nautilus_pyo3.Quantity.from_decimal_dp(repay_amount, bought_qty.precision)
            await self._http_client.repay_spot_borrow(coin, repay_qty)

            self._log.info(
                f"Successfully repaid {repay_amount} {coin} spot borrow",
                LogColor.GREEN,
            )
        except Exception as e:
            self._log.error(
                f"Failed to repay spot borrow for {coin}: {e}",
                LogColor.RED,
            )

    def _is_repay_blackout_window(self) -> bool:
        # Check if current UTC time is within Bybit's repayment blackout window (04:00-05:30 UTC daily).
        # During this window, Bybit blocks no-convert repayment operations for interest calculation.
        now_utc = self._clock.utc_now()
        hour = now_utc.hour
        minute = now_utc.minute

        return hour == 4 or (hour == 5 and minute < 30)

    def _is_external_order(self, client_order_id: ClientOrderId) -> bool:
        return not client_order_id or not self._cache.strategy_id_for_order(client_order_id)


# Bybit V5 API uses PascalCase strings for these enum fields.
_BYBIT_VALID_TRIGGER_TYPES: frozenset[str] = frozenset({"LastPrice", "IndexPrice", "MarkPrice"})
_BYBIT_VALID_ORDER_TYPES: frozenset[str] = frozenset({"Market", "Limit"})


def _validate_price_string(key: str, val: str) -> str:
    s = str(val)
    try:
        p = nautilus_pyo3.Price.from_str(s)
    except ValueError:
        raise ValueError(
            f"Invalid Bybit price for '{key}': '{s}'",
        ) from None
    if p.as_double() < 0:
        raise ValueError(
            f"Invalid Bybit price for '{key}': '{s}', expected a non-negative value",
        )
    return s


def _validate_tp_sl_cross_fields(result: dict) -> None:
    has_tp = "take_profit" in result
    has_sl = "stop_loss" in result
    tp_fields = ("tp_trigger_by", "tp_order_type", "tp_limit_price", "tp_trigger_price")
    sl_fields = ("sl_trigger_by", "sl_order_type", "sl_limit_price", "sl_trigger_price")

    if not has_tp and any(k in result for k in tp_fields):
        raise ValueError("TP override fields require 'take_profit' to be set")

    if not has_sl and any(k in result for k in sl_fields):
        raise ValueError("SL override fields require 'stop_loss' to be set")

    if result.get("tp_order_type") == "Limit" and "tp_limit_price" not in result:
        raise ValueError("'tp_order_type' is 'Limit' but 'tp_limit_price' was not provided")
    if result.get("sl_order_type") == "Limit" and "sl_limit_price" not in result:
        raise ValueError("'sl_order_type' is 'Limit' but 'sl_limit_price' was not provided")
    if "tp_limit_price" in result and result.get("tp_order_type") != "Limit":
        raise ValueError("'tp_limit_price' requires 'tp_order_type' to be 'Limit'")
    if "sl_limit_price" in result and result.get("sl_order_type") != "Limit":
        raise ValueError("'sl_limit_price' requires 'sl_order_type' to be 'Limit'")


def _parse_bybit_tp_sl_params(params: dict | None) -> dict:
    p = params or {}
    result: dict = {"is_leverage": bool(p.get("is_leverage", False))}

    for key in (
        "take_profit",
        "stop_loss",
        "tp_trigger_price",
        "sl_trigger_price",
        "tp_limit_price",
        "sl_limit_price",
    ):
        val = p.get(key)
        if val is not None:
            result[key] = _validate_price_string(key, val)

    for key, valid, label in (
        ("tp_trigger_by", _BYBIT_VALID_TRIGGER_TYPES, "trigger type"),
        ("sl_trigger_by", _BYBIT_VALID_TRIGGER_TYPES, "trigger type"),
        ("tp_order_type", _BYBIT_VALID_ORDER_TYPES, "order type"),
        ("sl_order_type", _BYBIT_VALID_ORDER_TYPES, "order type"),
    ):
        val = p.get(key)
        if val is not None:
            if val not in valid:
                raise ValueError(
                    f"Invalid Bybit {label} for '{key}': '{val}'. Expected one of {sorted(valid)}.",
                )
            result[key] = val

    _validate_tp_sl_cross_fields(result)

    val = p.get("close_on_trigger")
    if val is not None:
        result["close_on_trigger"] = bool(val)

    val = p.get("position_idx")
    if val is not None:
        if isinstance(val, bool) or not isinstance(val, int):
            raise ValueError(
                f"Invalid type for 'position_idx': {type(val).__name__}, expected int",
            )
        if val not in (0, 1, 2):
            raise ValueError(
                f"Invalid 'position_idx': {val}, expected 0, 1, or 2",
            )
        result["position_idx"] = val

    _parse_option_params(p, result)

    return result


def _parse_option_params(p: dict, result: dict) -> None:
    val = p.get("order_iv")
    if val is not None:
        if isinstance(val, bool) or not isinstance(val, (str, int, float)):
            raise ValueError(
                f"Invalid type for 'order_iv': {type(val).__name__}, expected str or number",
            )
        result["order_iv"] = str(val)

    val = p.get("mmp")
    if val is not None:
        if not isinstance(val, bool):
            raise ValueError(
                f"Invalid type for 'mmp': {type(val).__name__}, expected bool",
            )
        result["mmp"] = val


def _apply_tp_sl_fields(order_params: object, tp_sl: dict) -> None:
    for attr in (
        "tp_trigger_by",
        "sl_trigger_by",
        "tp_order_type",
        "sl_order_type",
        "tp_trigger_price",
        "sl_trigger_price",
        "tp_limit_price",
        "sl_limit_price",
        "close_on_trigger",
        "order_iv",
        "mmp",
    ):
        val = tp_sl.get(attr)
        if val is not None:
            setattr(order_params, attr, val)
