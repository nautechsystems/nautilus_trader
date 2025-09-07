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
from typing import Any

from nautilus_trader.adapters.okx.config import OKXExecClientConfig
from nautilus_trader.adapters.okx.constants import OKX_VENUE
from nautilus_trader.adapters.okx.providers import OKXInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import ensure_pydatetime_utc
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.core.nautilus_pyo3 import OKXTradeMode
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import SubmitOrder
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


class OKXExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the OKX centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.OKXHttpClient
        The OKX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : OKXInstrumentProvider
        The instrument provider.
    config : OKXExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.OKXHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: OKXInstrumentProvider,
        config: OKXExecClientConfig,
        name: str | None,
    ) -> None:
        PyCondition.not_empty(config.instrument_types, "config.instrument_types")

        # Determine account type based on instrument types
        if instrument_provider.instrument_types == (OKXInstrumentType.SPOT,):
            account_type = AccountType.CASH
        else:
            account_type = AccountType.MARGIN

        super().__init__(
            loop=loop,
            client_id=ClientId(name or OKX_VENUE.value),
            venue=OKX_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=account_type,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._instrument_provider: OKXInstrumentProvider = instrument_provider

        instrument_types = [i.name.upper() for i in config.instrument_types]
        contract_types = (
            [c.name.upper() for c in config.contract_types] if config.contract_types else None
        )

        # Configuration
        self._config = config
        self._log.info(f"config.instrument_types={instrument_types}", LogColor.BLUE)
        self._log.info(f"config.contract_types={contract_types}", LogColor.BLUE)
        self._log.info(f"{config.margin_mode=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.use_fills_channel=}", LogColor.BLUE)
        self._log.info(f"{config.use_mm_mass_cancel=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)

        # Set account ID
        account_id = AccountId(f"{name or OKX_VENUE.value}-master")
        self._set_account_id(account_id)

        # Create pyo3 account ID for Rust HTTP client
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # WebSocket API
        self._ws_client = nautilus_pyo3.OKXWebSocketClient.with_credentials(
            url=config.base_url_ws or nautilus_pyo3.get_okx_ws_url_private(config.is_demo),
            account_id=self.pyo3_account_id,
        )
        self._ws_client_futures: set[asyncio.Future] = set()

        # Set trade mode
        if account_type == AccountType.CASH:
            self._trade_mode = OKXTradeMode.CASH
        else:
            # TODO: Initially support isolated margin only
            self._trade_mode = OKXTradeMode.ISOLATED

    @property
    def okx_instrument_provider(self) -> OKXInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._cache_instruments()
        await self._update_account_state()

        await self._ws_client.connect(
            instruments=self.okx_instrument_provider.instruments_pyo3(),
            callback=self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_client.wait_until_active(timeout_secs=10.0)
        self._log.info(f"Connected to {self._ws_client.url}", LogColor.BLUE)
        self._log.info(f"Private websocket API key {self._ws_client.api_key}", LogColor.BLUE)
        self._log.info("OKX API key authenticated", LogColor.GREEN)

        # Subscribe to orders and account updates
        for instrument_type in self._instrument_provider._instrument_types:
            await self._ws_client.subscribe_orders(instrument_type)

            # Only subscribe to fills channel if VIP5+ (configurable)
            if self._config.use_fills_channel:
                self._log.info("Subscribing to fills channel", LogColor.BLUE)
                await self._ws_client.subscribe_fills(instrument_type)
            else:
                self._log.info(
                    "Using order status reports for fill information (standard for all users)",
                    LogColor.BLUE,
                )

        await self._ws_client.subscribe_account()

    async def _disconnect(self) -> None:
        # Shutdown websocket
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting websocket")

            await self._ws_client.close()

            self._log.info(f"Disconnected from {self._ws_client.url}", LogColor.BLUE)

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
        # price and size precisions when parsing responses.
        instruments_pyo3 = self.okx_instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _update_account_state(self) -> None:
        try:
            pyo3_account_state = await self._http_client.request_account_state(self.pyo3_account_id)
            account_state = AccountState.from_dict(pyo3_account_state.to_dict())

            self.generate_account_state(
                balances=account_state.balances,
                margins=[],  # TBD
                reported=True,
                ts_event=self._clock.timestamp_ns(),
            )
        except Exception as e:
            self._log.error(f"Failed to update account state: {e}")

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        # Wait for instruments to be cached
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.debug(
            f"Requesting OrderStatusReports "
            f"{repr(command.instrument_id) if command.instrument_id else ''}"
            "...",
        )

        pyo3_reports: list[nautilus_pyo3.OrderStatusReport] = []
        reports: list[OrderStatusReport] = []

        try:
            if command.instrument_id:
                # Request for specific instrument
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                response = await self._http_client.request_order_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    start=ensure_pydatetime_utc(command.start),
                    end=ensure_pydatetime_utc(command.end),
                    open_only=command.open_only,
                )
                pyo3_reports.extend(response)
            else:
                for instrument_type in self._config.instrument_types:
                    response = await self._http_client.request_order_status_reports(
                        account_id=self.pyo3_account_id,
                        instrument_type=instrument_type,
                        start=ensure_pydatetime_utc(command.start),
                        end=ensure_pydatetime_utc(command.end),
                        open_only=command.open_only,
                    )
                    pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
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
        # Check instruments are cached
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.debug(
            "Requesting OrderStatusReport "
            + ", ".join(
                repr(x)
                for x in [
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                ]
                if x
            )
            + " ...",
        )

        try:
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
            pyo3_reports: list[nautilus_pyo3.OrderStatusReport] = (
                await self._http_client.request_order_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                )
            )

            if not pyo3_reports:
                return None

            # Filter for the specific order we're looking for
            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                if (
                    command.client_order_id
                    and report.client_order_id is not None
                    and report.client_order_id == command.client_order_id
                ) or (command.venue_order_id and report.venue_order_id == command.venue_order_id):
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    return report

        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReport", e)

        return None

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        # Check instruments cache first
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.debug(
            "Requesting FillReports "
            + ", ".join(
                repr(x)
                for x in [
                    command.instrument_id,
                    command.venue_order_id,
                ]
                if x
            )
            + " ...",
        )

        pyo3_reports: list[nautilus_pyo3.FillReport] = []
        reports: list[FillReport] = []

        try:
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                response = await self._http_client.request_fill_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    start=ensure_pydatetime_utc(command.start),
                    end=ensure_pydatetime_utc(command.end),
                )
                pyo3_reports.extend(response)
            else:
                for instrument_type in self._config.instrument_types:
                    response = await self._http_client.request_fill_reports(
                        account_id=self.pyo3_account_id,
                        instrument_type=instrument_type,
                        start=ensure_pydatetime_utc(command.start),
                        end=ensure_pydatetime_utc(command.end),
                    )
                    pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)
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
        # Check instruments are cached
        if not self._http_client.is_initialized():
            await self._cache_instruments()

        self._log.debug(
            f"Requesting PositionStatusReports"
            f" {repr(command.instrument_id) if command.instrument_id else ''}"
            " ...",
        )

        pyo3_reports: list[nautilus_pyo3.PositionStatusReport] = []
        reports: list[PositionStatusReport] = []

        try:
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                response = await self._http_client.request_position_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                )

                if not response:
                    instrument = self._cache.instrument(command.instrument_id)
                    if instrument is None:
                        raise RuntimeError(
                            f"Cannot create FLAT position report - instrument {command.instrument_id} not found",
                        )

                    report = PositionStatusReport.create_flat(
                        account_id=self.account_id,
                        instrument_id=command.instrument_id,
                        size_precision=instrument.size_precision,
                        ts_init=self._clock.timestamp_ns(),
                    )
                    reports.append(report)
                else:
                    pyo3_reports.extend(response)
            else:
                for instrument_type in self._config.instrument_types:
                    response = await self._http_client.request_position_status_reports(
                        account_id=self.pyo3_account_id,
                        instrument_type=instrument_type,
                    )
                    pyo3_reports.extend(response)

            for pyo3_report in pyo3_reports:
                report = PositionStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except Exception as e:
            self._log.exception("Failed to generate PositionReports", e)

        len_reports = len(reports)
        plural = "" if len_reports == 1 else "s"
        self._log.info(f"Generated {len(reports)} PositionReport{plural}")

        return reports

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _query_account(self, _command: QueryAccount) -> None:
        # Specific account ID (sub account) not yet supported
        await self._update_account_state()

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

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        pyo3_client_order_id = (
            nautilus_pyo3.ClientOrderId(command.client_order_id.value)
            if command.client_order_id is not None
            else None
        )
        pyo3_venue_order_id = (
            nautilus_pyo3.VenueOrderId(command.venue_order_id.value)
            if command.venue_order_id
            else None
        )

        try:
            await self._ws_client.cancel_order(
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
            )
        except Exception as e:
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        if self._config.use_mm_mass_cancel:
            await self._cancel_all_orders_mass_cancel(command)
        else:
            await self._cancel_all_orders_individually(command)

    async def _cancel_all_orders_mass_cancel(self, command: CancelAllOrders) -> None:
        # Use OKX's mass-cancel WebSocket endpoint for market makers
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        try:
            await self._ws_client.mass_cancel_orders(
                instrument_id=pyo3_instrument_id,
            )
        except Exception as e:
            # If mass cancel fails, generate cancel rejected events for all open orders
            orders_open = self._cache.orders_open(instrument_id=command.instrument_id)
            for order in orders_open:
                if not order.is_closed:
                    self.generate_order_cancel_rejected(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=order.venue_order_id,
                        reason=str(e),
                        ts_event=self._clock.timestamp_ns(),
                    )

    async def _cancel_all_orders_individually(self, command: CancelAllOrders) -> None:
        # Cancel orders one by one (each with retry logic) - works for all users
        orders_open: list[Order] = self._cache.orders_open(instrument_id=command.instrument_id)
        for order in orders_open:
            if order.is_closed:
                continue

            cancel_command = CancelOrder(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                command_id=command.id,
                ts_init=command.ts_init,
            )
            await self._cancel_order(cancel_command)

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

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        pyo3_client_order_id = (
            nautilus_pyo3.ClientOrderId(command.client_order_id.value)
            if command.client_order_id is not None
            else None
        )
        pyo3_venue_order_id = (
            nautilus_pyo3.VenueOrderId(command.venue_order_id.value)
            if command.venue_order_id
            else None
        )
        pyo3_price = nautilus_pyo3.Price.from_str(str(command.price)) if command.price else None
        pyo3_quantity = (
            nautilus_pyo3.Quantity.from_str(str(command.quantity)) if command.quantity else None
        )

        try:
            await self._ws_client.modify_order(
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                price=pyo3_price,
                quantity=pyo3_quantity,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
            )
        except Exception as e:
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        # Generate OrderSubmitted event here to ensure correct event sequencing
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
        pyo3_trigger_price = (
            nautilus_pyo3.Price.from_str(str(order.trigger_price))
            if order.has_trigger_price
            else None
        )

        pyo3_time_in_force = (
            time_in_force_to_pyo3(order.time_in_force) if order.time_in_force else None
        )

        try:
            await self._ws_client.submit_order(
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                td_mode=self._trade_mode,
                client_order_id=pyo3_client_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                quantity=pyo3_quantity,
                price=pyo3_price,
                trigger_price=pyo3_trigger_price,
                time_in_force=pyo3_time_in_force,
                post_only=order.is_post_only,
                reduce_only=order.is_reduce_only,
                quote_quantity=order.is_quote_quantity,
            )
        except Exception as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    # -- WEBSOCKET HANDLERS -----------------------------------------------------------------------

    def _is_external_order(self, client_order_id: ClientOrderId) -> bool:
        return not client_order_id or not self._cache.strategy_id_for_order(client_order_id)

    def _handle_msg(self, msg: Any) -> None:
        if isinstance(msg, nautilus_pyo3.OKXWebSocketError):
            self._log.error(repr(msg))
            return

        try:
            self._log.debug(f"Received websocket message: {type(msg)}")

            if isinstance(msg, dict) and msg.get("type") == "AccountState":
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
            else:
                self._log.debug(f"Received unhandled message type: {type(msg)}")

        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    def _handle_account_state(self, msg: dict) -> None:
        try:
            account_state = AccountState.from_dict(msg)
            self._log.debug(f"Received account update: {account_state}")

            # Generate account state update event
            self.generate_account_state(
                balances=account_state.balances,
                margins=account_state.margins,
                reported=account_state.is_reported,
                ts_event=account_state.ts_event,
            )
        except Exception as e:
            self._log.error(f"Failed to process account state update: {e}")

    def _handle_order_rejected_pyo3(self, pyo3_event: nautilus_pyo3.OrderRejected) -> None:
        event = OrderRejected.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)

    def _handle_order_cancel_rejected_pyo3(
        self,
        pyo3_event: nautilus_pyo3.OrderCancelRejected,
    ) -> None:
        event = OrderCancelRejected.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)

    def _handle_order_modify_rejected_pyo3(
        self,
        pyo3_event: nautilus_pyo3.OrderModifyRejected,
    ) -> None:
        event = OrderModifyRejected.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)

    def _handle_order_status_report_pyo3(
        self,
        pyo3_report: nautilus_pyo3.OrderStatusReport,
    ) -> None:
        # Discard order status reports until account is properly initialized
        # Reconciliation will handle getting the current state of open orders
        if not self.is_connected or not self.account_id or not self._cache.account(self.account_id):
            self._log.debug(
                f"Discarding order status report during connection sequence: {pyo3_report.client_order_id!r}",
            )
            return

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

        if report.order_status == OrderStatus.REJECTED:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=report.instrument_id,
                client_order_id=report.client_order_id,
                reason=report.reason,
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
        elif report.order_status == OrderStatus.CANCELED:
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
            self._log.warning(f"Received unhandled OrderStatusReport: {report}")

    def _handle_order_update(self, order: Any, report: OrderStatusReport) -> None:
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

    def _handle_fill_report_pyo3(self, pyo3_report: nautilus_pyo3.FillReport) -> None:
        """
        Handle PyO3 FillReport (both fills channel and order status derived).
        """
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
