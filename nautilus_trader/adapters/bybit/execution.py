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
"""
Bybit execution client implementation.

This module provides a LiveExecutionClient that interfaces with Bybit's REST and
WebSocket APIs for order management and execution. The client uses Rust-based HTTP and
WebSocket clients exposed via PyO3 for performance.

"""

import asyncio
from typing import Any

from nautilus_trader.accounting.factory import AccountFactory
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
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
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderStatus
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
            # Bybit SPOT accounts support margin trading (borrowing)
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
        self._log.info(f"{config.testnet=}", LogColor.BLUE)
        self._log.info(f"{config.use_gtd=}", LogColor.BLUE)
        self._log.info(f"{config.use_ws_execution_fast=}", LogColor.BLUE)
        self._log.info(f"{config.use_http_batch_api=}", LogColor.BLUE)
        self._log.info(f"{config.use_spot_position_reports=}", LogColor.BLUE)
        self._log.info(f"{config.ignore_uncached_instrument_executions=}", LogColor.BLUE)
        self._log.info(f"{config.ws_trade_timeout_secs=}", LogColor.BLUE)

        # Set account ID
        account_id = AccountId(f"{name or BYBIT_VENUE.value}-UNIFIED")
        self._set_account_id(account_id)

        # Create pyo3 account ID for Rust HTTP client
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        # HTTP API
        self._http_client = client
        masked_key = self._http_client.masked_api_key()
        self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

        # Configure HTTP client settings
        self._http_client.set_use_spot_position_reports(self._use_spot_position_reports)

        # WebSocket API - environment setup
        # Priority: demo > testnet > mainnet
        if config.demo:
            environment = nautilus_pyo3.BybitEnvironment.DEMO
        elif config.testnet:
            environment = nautilus_pyo3.BybitEnvironment.TESTNET
        else:
            environment = nautilus_pyo3.BybitEnvironment.MAINNET

        # WebSocket API - Private channel
        self._ws_private_client = nautilus_pyo3.BybitWebSocketClient.new_private(
            environment=environment,
            api_key=config.api_key,
            api_secret=config.api_secret,
            url=config.base_url_ws_private,
            heartbeat=20,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

        # WebSocket API - Trade channel (always enabled)
        self._ws_trade_client: nautilus_pyo3.BybitWebSocketClient = (
            nautilus_pyo3.BybitWebSocketClient.new_trade(
                environment=environment,
                api_key=config.api_key,
                api_secret=config.api_secret,
                url=config.base_url_ws_trade,
                heartbeat=20,
            )
        )

    @property
    def bybit_instrument_provider(self) -> BybitInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._cache_instruments()
        await self._update_account_state()
        await self._await_account_registered()

        # Set account_id on WebSocket clients so they can parse account messages
        self._ws_private_client.set_account_id(self.pyo3_account_id)
        self._ws_trade_client.set_account_id(self.pyo3_account_id)

        # Connect to private WebSocket
        await self._ws_private_client.connect(callback=self._handle_msg)

        # Wait for connection to be established
        await self._ws_private_client.wait_until_active(timeout_secs=10.0)
        self._log.info("Connected to private WebSocket", LogColor.BLUE)

        await self._ws_private_client.subscribe_orders()
        await self._ws_private_client.subscribe_executions()
        await self._ws_private_client.subscribe_positions()
        await self._ws_private_client.subscribe_wallet()

        # Connect to trade WebSocket
        await self._ws_trade_client.connect(callback=self._handle_msg)
        await self._ws_trade_client.wait_until_active(timeout_secs=10.0)
        self._log.info("Connected to trade WebSocket", LogColor.BLUE)

    async def _disconnect(self) -> None:
        self._http_client.cancel_all_requests()

        # Close private WebSocket
        if await self._ws_private_client.is_active():
            self._log.info("Disconnecting private websocket")
            await self._ws_private_client.close()

        # Close trade WebSocket
        if await self._ws_trade_client.is_active():
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

    async def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self.bybit_instrument_provider.instruments_pyo3()

        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)
            self._ws_private_client.add_instrument(inst)
            self._ws_trade_client.add_instrument(inst)

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
            await self._http_client.set_margin_mode(self._margin_mode)  # type: ignore[attr-defined]
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

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        receipt_log = f"Received {len(reports)} OrderStatusReport{plural}"

        if command.log_receipt_level == LogLevel.INFO:
            self._log.info(receipt_log)
        else:
            self._log.debug(receipt_log)

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
                LogColor.CYAN,
            )

            pyo3_report = await self._http_client.query_order(
                account_id=self.pyo3_account_id,
                product_type=product_type,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
            )

            self._log.debug(f"query_order returned: {pyo3_report}", LogColor.CYAN)

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
        except Exception as e:
            self._log.exception("Failed to generate FillReports", e)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} FillReport{plural}")

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
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            for product_type in self._product_types:
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
        except ValueError as e:
            if "request canceled" in str(e).lower():
                self._log.debug("PositionStatusReports request cancelled during shutdown")
            else:
                self._log.exception("Failed to generate PositionStatusReports", e)
        except Exception as e:
            self._log.exception("Failed to generate PositionStatusReports", e)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Received {len(reports)} PositionStatusReport{plural}")

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

            await self._http_client.set_leverage(  # type: ignore[attr-defined]
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

            await self._http_client.switch_mode(  # type: ignore[attr-defined]
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

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _query_account(self, _command: QueryAccount) -> None:
        await self._update_account_state()

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        # Generate OrderSubmitted event
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
        pyo3_time_in_force = (
            time_in_force_to_pyo3(order.time_in_force) if order.time_in_force else None
        )

        # Extract trigger price for conditional orders
        pyo3_trigger_price = None
        if hasattr(order, "trigger_price") and order.trigger_price is not None:
            pyo3_trigger_price = nautilus_pyo3.Price.from_str(str(order.trigger_price))

        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            order.instrument_id.symbol.value,
        )

        try:
            # Submit via WebSocket
            await self._ws_trade_client.submit_order(
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

        now_ns = self._clock.timestamp_ns()
        order_params = []

        for order in command.order_list.orders:
            if order.is_closed:
                self._log.warning(f"Cannot submit already closed order: {order}")
                continue

            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=now_ns,
            )

            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
            pyo3_order_side = nautilus_pyo3.OrderSide(order.side.value)
            pyo3_order_type = nautilus_pyo3.OrderType(order.order_type.value)
            pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
            pyo3_time_in_force = (
                nautilus_pyo3.TimeInForce(order.time_in_force.value)
                if order.time_in_force
                else None
            )
            pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
            pyo3_trigger_price = (
                nautilus_pyo3.Price.from_str(str(order.trigger_price))
                if order.has_trigger_price
                else None
            )
            post_only = order.is_post_only
            reduce_only = order.is_reduce_only

            product_type = nautilus_pyo3.bybit_product_type_from_symbol(
                order.instrument_id.symbol.value,
            )

            params = self._ws_trade_client.build_place_order_params(
                product_type=product_type,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                quantity=pyo3_quantity,
                time_in_force=pyo3_time_in_force,
                price=pyo3_price,
                trigger_price=pyo3_trigger_price,
                post_only=post_only,
                reduce_only=reduce_only,
            )
            order_params.append(params)

        if order_params:
            await self._ws_trade_client.batch_place_orders(order_params)

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

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
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
        pyo3_quantity = (
            nautilus_pyo3.Quantity.from_str(str(command.quantity)) if command.quantity else None
        )
        pyo3_price = nautilus_pyo3.Price.from_str(str(command.price)) if command.price else None

        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            command.instrument_id.symbol.value,
        )

        try:
            # Modify via WebSocket
            await self._ws_trade_client.modify_order(
                product_type=product_type,
                instrument_id=pyo3_instrument_id,
                venue_order_id=pyo3_venue_order_id,
                client_order_id=pyo3_client_order_id,
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

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
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

        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            command.instrument_id.symbol.value,
        )

        try:
            # Cancel via WebSocket
            await self._ws_trade_client.cancel_order(
                product_type=product_type,
                instrument_id=pyo3_instrument_id,
                venue_order_id=pyo3_venue_order_id,
                client_order_id=pyo3_client_order_id,
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

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        if not command.cancels:
            return

        # Extract data from cancel commands
        instrument_ids = []
        client_order_ids = []
        venue_order_ids = []

        for cancel in command.cancels:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(cancel.instrument_id.value)
            instrument_ids.append(pyo3_instrument_id)

            pyo3_client_order_id = (
                nautilus_pyo3.ClientOrderId(cancel.client_order_id.value)
                if cancel.client_order_id
                else None
            )
            client_order_ids.append(pyo3_client_order_id)

            pyo3_venue_order_id = (
                nautilus_pyo3.VenueOrderId(cancel.venue_order_id.value)
                if cancel.venue_order_id
                else None
            )
            venue_order_ids.append(pyo3_venue_order_id)

        # Derive product type from first cancel (all must be same product type for batch operation)
        product_type = nautilus_pyo3.bybit_product_type_from_symbol(
            command.cancels[0].instrument_id.symbol.value,
        )

        try:
            # Batch cancel via WebSocket
            await self._ws_trade_client.batch_cancel_orders(
                product_type=product_type,
                instrument_ids=instrument_ids,
                venue_order_ids=venue_order_ids,
                client_order_ids=client_order_ids,
            )
        except Exception as e:
            self._log.error(f"Failed to batch cancel orders: {e}")

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
        event = OrderRejected.from_pyo3(msg)
        self._send_order_event(event)

    def _handle_order_cancel_rejected_pyo3(self, msg: nautilus_pyo3.OrderCancelRejected) -> None:
        event = OrderCancelRejected.from_pyo3(msg)
        self._send_order_event(event)

    def _handle_order_modify_rejected_pyo3(self, msg: nautilus_pyo3.OrderModifyRejected) -> None:
        event = OrderModifyRejected.from_pyo3(msg)
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
        elif report.order_status == OrderStatus.ACCEPTED:
            if is_order_updated(order, report):
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
        elif report.order_status == OrderStatus.EXPIRED:
            self.generate_order_expired(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
            )
        elif report.order_status == OrderStatus.TRIGGERED:
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

    def _handle_position_status_report_pyo3(self, msg: nautilus_pyo3.PositionStatusReport) -> None:
        report = PositionStatusReport.from_pyo3(msg)
        self._log.debug(f"Received {report}", LogColor.MAGENTA)
        # Do not send position reports from WebSocket stream - we use HTTP endpoint for reconciliation
        # to avoid noise from position updates every time a fill occurs

    def _is_external_order(self, client_order_id: ClientOrderId) -> bool:
        return not client_order_id or not self._cache.strategy_id_for_order(client_order_id)


def is_order_updated(order: Order, report: OrderStatusReport) -> bool:
    if order.has_price and report.price and order.price != report.price:
        return True

    if (
        order.has_trigger_price
        and report.trigger_price
        and order.trigger_price != report.trigger_price
    ):
        return True

    return order.quantity != report.quantity
