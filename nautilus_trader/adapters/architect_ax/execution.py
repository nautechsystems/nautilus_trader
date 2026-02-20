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
AX Exchange execution client implementation.

This module provides a LiveExecutionClient that interfaces with Architect's REST and
WebSocket APIs for order management and execution. The client uses Rust-based HTTP and
WebSocket clients exposed via PyO3 for performance.

"""

import asyncio
from typing import Any

from nautilus_trader.adapters.architect_ax.config import AxExecClientConfig
from nautilus_trader.adapters.architect_ax.constants import AX_SUPPORTED_ORDER_TYPES
from nautilus_trader.adapters.architect_ax.constants import AX_VENUE
from nautilus_trader.adapters.architect_ax.constants import AX_WS_ORDERS_PRODUCTION_URL
from nautilus_trader.adapters.architect_ax.constants import AX_WS_ORDERS_SANDBOX_URL
from nautilus_trader.adapters.architect_ax.providers import AxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import AxEnvironment
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
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import position_side_to_str
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.functions import order_side_to_pyo3
from nautilus_trader.model.functions import order_type_to_pyo3
from nautilus_trader.model.functions import time_in_force_to_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import Order


class AxExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the AX Exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.AxHttpClient
        The AX Exchange HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : AxInstrumentProvider
        The instrument provider.
    config : AxExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.AxHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: AxInstrumentProvider,
        config: AxExecClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or AX_VENUE.value),
            venue=AX_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._instrument_provider: AxInstrumentProvider = instrument_provider
        self._config = config

        account_id = AccountId(f"{name or AX_VENUE.value}-001")
        self._set_account_id(account_id)
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)
        self.pyo3_trader_id = nautilus_pyo3.TraderId(self.trader_id.value)

        self._log.info(f"environment={config.environment.name}", LogColor.BLUE)
        self._log.info(f"API key {client.api_key_masked}", LogColor.BLUE)

        self._http_client = client
        self._ws_orders_client: nautilus_pyo3.AxOrdersWebSocketClient | None = None
        self._has_credentials = False

        if config.base_url_ws:
            self._ws_orders_url = config.base_url_ws
        elif config.environment == AxEnvironment.SANDBOX:
            self._ws_orders_url = AX_WS_ORDERS_SANDBOX_URL
        else:
            self._ws_orders_url = AX_WS_ORDERS_PRODUCTION_URL

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()

        try:
            bearer_token = await self._http_client.authenticate_auto()
            self._log.info("Authenticated with AX Exchange", LogColor.BLUE)
            self._has_credentials = True

            await self._update_account_state()
            await self._await_account_registered()

            self._ws_orders_client = nautilus_pyo3.AxOrdersWebSocketClient(
                url=self._ws_orders_url,
                account_id=self.pyo3_account_id,
                trader_id=self.pyo3_trader_id,
                heartbeat=30,
            )

            # Cache instruments for proper precision handling
            for inst in self._instrument_provider.instruments_pyo3():
                self._ws_orders_client.cache_instrument(inst)

            await self._ws_orders_client.connect(
                callback=self._handle_msg,
                bearer_token=bearer_token,
            )
            self._log.info("Connected to AX orders WebSocket", LogColor.BLUE)
        except ValueError as e:
            err_str = str(e)
            if "Missing credentials" in err_str or "MissingCredentials" in err_str:
                self._log.warning("No API credentials configured, execution features unavailable")
            else:
                raise

        self._log.info("Connected to AX Exchange execution API", LogColor.BLUE)

    async def _update_account_state(self) -> None:
        pyo3_account_state = await self._http_client.request_account_state(
            self.pyo3_account_id,
        )
        account_state = AccountState.from_dict(pyo3_account_state.to_dict())

        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

    def _cache_instruments(self) -> None:
        for inst in self._instrument_provider.instruments_pyo3():
            self._http_client.cache_instrument(inst)

    async def _disconnect(self) -> None:
        self._http_client.cancel_all_requests()

        if self._ws_orders_client and not self._ws_orders_client.is_closed():
            self._log.info("Disconnecting orders WebSocket")
            await self._ws_orders_client.close()

        self._log.info("Disconnected from AX Exchange execution API", LogColor.BLUE)

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        self._log.debug(f"Generating OrderStatusReport for {command}")

        # Read immutable order fields from cache
        order = self._cache.order(command.client_order_id) if command.client_order_id else None
        if order is not None:
            pyo3_side = order_side_to_pyo3(order.side)
            pyo3_type = order_type_to_pyo3(order.order_type)
            pyo3_tif = time_in_force_to_pyo3(order.time_in_force)
        else:
            pyo3_side = nautilus_pyo3.OrderSide.NO_ORDER_SIDE
            pyo3_type = nautilus_pyo3.OrderType.LIMIT
            pyo3_tif = nautilus_pyo3.TimeInForce.GTC

        instrument_id = command.instrument_id or (order.instrument_id if order else None)
        if instrument_id is None:
            self._log.error(
                "Cannot generate OrderStatusReport: no instrument_id on command or cached order",
            )
            return None

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(instrument_id.value)
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

        try:
            pyo3_report = await self._http_client.request_order_status(
                self.pyo3_account_id,
                pyo3_instrument_id,
                pyo3_side,
                pyo3_type,
                pyo3_tif,
                pyo3_client_order_id,
                pyo3_venue_order_id,
            )
            return OrderStatusReport.from_pyo3(pyo3_report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "OrderStatusReport")

        return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        self._log.debug("Requesting OrderStatusReports...")
        reports: list[OrderStatusReport] = []

        try:
            pyo3_reports = await self._http_client.request_order_status_reports(
                self.pyo3_account_id,
            )
            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "OrderStatusReports")

        self._log_report_receipt(
            len(reports),
            "OrderStatusReport",
            command.log_receipt_level,
        )

        return reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        self._log.debug("Requesting FillReports...")
        reports: list[FillReport] = []

        try:
            pyo3_reports = await self._http_client.request_fill_reports(
                self.pyo3_account_id,
            )
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
        reports: list[PositionStatusReport] = []

        try:
            pyo3_reports = await self._http_client.request_position_reports(
                self.pyo3_account_id,
            )
            for pyo3_report in pyo3_reports:
                report = PositionStatusReport.from_pyo3(pyo3_report)
                self._log.info(
                    f"Position: {report.instrument_id} side={position_side_to_str(report.position_side)} "
                    f"qty={report.quantity} avg_px={report.avg_px_open}",
                    LogColor.MAGENTA,
                )
                reports.append(report)
        except (asyncio.CancelledError, Exception) as e:
            self._log_report_error(e, "PositionStatusReports")

        self._log_report_receipt(
            len(reports),
            "PositionStatusReport",
            command.log_receipt_level,
        )

        return reports

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit closed order: {order}")
            return

        if not self._has_credentials or self._ws_orders_client is None:
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason="No API credentials configured",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        if order.order_type not in AX_SUPPORTED_ORDER_TYPES:
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"Unsupported order type: {order.order_type.name}. "
                "AX supports MARKET, LIMIT and STOP_LIMIT.",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        pyo3_price = None
        if order.has_price:
            pyo3_price = nautilus_pyo3.Price.from_str(str(order.price))
        elif order.order_type == OrderType.MARKET:
            # AX does not support native market orders, use preview endpoint
            # to get the take-through price for an aggressive IOC limit order
            try:
                pyo3_price = await self._get_market_order_price(order)
            except Exception as e:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=f"Market order preview failed: {e}",
                    ts_event=self._clock.timestamp_ns(),
                )
                return

            if pyo3_price is None:
                self.generate_order_denied(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason="No liquidity available for market order",
                    ts_event=self._clock.timestamp_ns(),
                )
                return

        pyo3_trigger_price = None
        if order.has_trigger_price:
            pyo3_trigger_price = nautilus_pyo3.Price.from_str(str(order.trigger_price))

        pyo3_trader_id = nautilus_pyo3.TraderId(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_time_in_force = time_in_force_to_pyo3(order.time_in_force)

        try:
            await self._ws_orders_client.submit_order(
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                quantity=pyo3_quantity,
                time_in_force=pyo3_time_in_force,
                price=pyo3_price,
                trigger_price=pyo3_trigger_price,
                post_only=order.is_post_only,
            )
        except Exception as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )
            return

        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

    async def _get_market_order_price(self, order: Order) -> nautilus_pyo3.Price | None:
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_side = order_side_to_pyo3(order.side)

        price = await self._http_client.preview_aggressive_limit_order(
            instrument_id=pyo3_instrument_id,
            quantity=pyo3_quantity,
            side=pyo3_side,
        )

        if price is None:
            return None

        self._log.info(
            f"Market order take-through price: {price} for {order.instrument_id}",
        )
        return price

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        for order in command.order_list.orders:
            submit_cmd = SubmitOrder(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                order=order,
                command_id=command.id,
                ts_init=command.ts_init,
                position_id=command.position_id,
                client_id=command.client_id,
            )
            await self._submit_order(submit_cmd)

    async def _modify_order(self, command: ModifyOrder) -> None:
        self.generate_order_modify_rejected(
            strategy_id=command.strategy_id,
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            reason="AX does not support order modification. Use cancel and resubmit instead.",
            ts_event=self._clock.timestamp_ns(),
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        if order.is_closed:
            self._log.warning(
                f"`CancelOrder` command for {command.client_order_id!r} when order already "
                f"{order.status_string()} (will not send to exchange)",
            )
            return

        if not self._has_credentials or self._ws_orders_client is None:
            self._log.error("Cannot cancel order: no API credentials configured")
            return

        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(command.client_order_id.value)
        pyo3_venue_order_id = (
            nautilus_pyo3.VenueOrderId(command.venue_order_id.value)
            if command.venue_order_id
            else None
        )

        try:
            await self._ws_orders_client.cancel_order(
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
        if not self._has_credentials or self._ws_orders_client is None:
            self._log.error("Cannot cancel orders: no API credentials configured")
            return

        open_orders = self._cache.orders_open(
            venue=self.venue,
            instrument_id=command.instrument_id,
            strategy_id=command.strategy_id,
            side=command.order_side,
        )

        for order in open_orders:
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
            pyo3_venue_order_id = (
                nautilus_pyo3.VenueOrderId(order.venue_order_id.value)
                if order.venue_order_id
                else None
            )

            try:
                await self._ws_orders_client.cancel_order(
                    client_order_id=pyo3_client_order_id,
                    venue_order_id=pyo3_venue_order_id,
                )
            except Exception as e:
                self._log.error(f"Failed to cancel order {order.client_order_id}: {e}")

    async def _query_account(self, command: QueryAccount) -> None:
        try:
            await self._update_account_state()
        except Exception as e:
            self._log.error(f"Failed to query account state: {e}")

    def _handle_msg(self, msg: Any) -> None:
        if isinstance(msg, nautilus_pyo3.OrderAccepted):
            self._handle_order_accepted(msg)
        elif isinstance(msg, nautilus_pyo3.OrderFilled):
            self._handle_order_filled(msg)
        elif isinstance(msg, nautilus_pyo3.OrderCanceled):
            self._handle_order_canceled(msg)
        elif isinstance(msg, nautilus_pyo3.OrderExpired):
            self._handle_order_expired(msg)
        elif isinstance(msg, nautilus_pyo3.OrderRejected):
            self._handle_order_rejected(msg)
        elif isinstance(msg, nautilus_pyo3.OrderCancelRejected):
            self._handle_order_cancel_rejected(msg)
        elif isinstance(msg, nautilus_pyo3.OrderStatusReport):
            self._handle_order_status_report(msg)
        elif isinstance(msg, nautilus_pyo3.FillReport):
            self._handle_fill_report(msg)
        else:
            self._log.warning(f"Received unhandled message type: {type(msg)}")

    def _handle_order_accepted(self, msg: nautilus_pyo3.OrderAccepted) -> None:
        event = OrderAccepted.from_dict(msg.to_dict())
        order = self._cache.order(event.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process OrderAccepted: order not found for {event.client_order_id!r}",
            )
            return

        self.generate_order_accepted(
            strategy_id=order.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            venue_order_id=event.venue_order_id,
            ts_event=event.ts_event,
        )

    def _handle_order_filled(self, msg: nautilus_pyo3.OrderFilled) -> None:
        client_order_id = ClientOrderId(msg.client_order_id.value)
        instrument_id = InstrumentId.from_str(str(msg.instrument_id))

        order = self._cache.order(client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process OrderFilled: order not found for {client_order_id!r}",
            )
            return

        instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot process OrderFilled: instrument not found for {instrument_id}",
            )
            return

        # Convert to dict and handle None commission (AX doesn't provide it)
        event_dict = msg.to_dict()
        if event_dict.get("commission") is None:
            event_dict["commission"] = f"0 {instrument.quote_currency.code}"

        event = OrderFilled.from_dict(event_dict)

        self.generate_order_filled(
            strategy_id=order.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            venue_order_id=event.venue_order_id,
            venue_position_id=event.position_id,
            trade_id=event.trade_id,
            order_side=order.side,
            order_type=order.order_type,
            last_qty=event.last_qty,
            last_px=event.last_px,
            quote_currency=instrument.quote_currency,
            commission=event.commission,
            liquidity_side=event.liquidity_side,
            ts_event=event.ts_event,
        )

    def _handle_order_canceled(self, msg: nautilus_pyo3.OrderCanceled) -> None:
        event = OrderCanceled.from_dict(msg.to_dict())
        order = self._cache.order(event.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process OrderCanceled: order not found for {event.client_order_id!r}",
            )
            return

        self.generate_order_canceled(
            strategy_id=order.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            venue_order_id=event.venue_order_id,
            ts_event=event.ts_event,
        )

    def _handle_order_expired(self, msg: nautilus_pyo3.OrderExpired) -> None:
        event = OrderExpired.from_dict(msg.to_dict())
        order = self._cache.order(event.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process OrderExpired: order not found for {event.client_order_id!r}",
            )
            return

        self.generate_order_expired(
            strategy_id=order.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            venue_order_id=event.venue_order_id,
            ts_event=event.ts_event,
        )

    def _handle_order_rejected(self, msg: nautilus_pyo3.OrderRejected) -> None:
        event = OrderRejected.from_dict(msg.to_dict())
        order = self._cache.order(event.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process OrderRejected: order not found for {event.client_order_id!r}",
            )
            return

        self.generate_order_rejected(
            strategy_id=order.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            reason=event.reason,
            ts_event=event.ts_event,
            due_post_only=event.due_post_only,
        )

    def _handle_order_cancel_rejected(self, msg: nautilus_pyo3.OrderCancelRejected) -> None:
        event = OrderCancelRejected.from_dict(msg.to_dict())
        order = self._cache.order(event.client_order_id)
        if order is None:
            self._log.error(
                f"Cannot process OrderCancelRejected: order not found for {event.client_order_id!r}",
            )
            return

        self.generate_order_cancel_rejected(
            strategy_id=order.strategy_id,
            instrument_id=event.instrument_id,
            client_order_id=event.client_order_id,
            venue_order_id=event.venue_order_id,
            reason=event.reason,
            ts_event=event.ts_event,
        )

    def _handle_order_status_report(self, msg: nautilus_pyo3.OrderStatusReport) -> None:
        report = OrderStatusReport.from_pyo3(msg)

        # Check for external order (no client_order_id or not in cache)
        if report.client_order_id is None:
            self._send_order_status_report(report)
            return

        order = self._cache.order(report.client_order_id)
        if order is None:
            # External order - send report for reconciliation
            self._send_order_status_report(report)
            return

        if report.order_status == OrderStatus.ACCEPTED:
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

    def _handle_fill_report(self, msg: nautilus_pyo3.FillReport) -> None:
        report = FillReport.from_pyo3(msg)

        # Check for external fill (no client_order_id or not in cache)
        if report.client_order_id is None:
            self._send_fill_report(report)
            return

        order = self._cache.order(report.client_order_id)
        if order is None:
            # External fill - send report for reconciliation
            self._send_fill_report(report)
            return

        instrument = self._cache.instrument(order.instrument_id)
        if instrument is None:
            self._log.error(
                f"Cannot process FillReport: instrument not found for {order.instrument_id}",
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
