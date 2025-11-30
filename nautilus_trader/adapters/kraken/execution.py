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
Kraken execution client implementation.

This module provides a LiveExecutionClient that interfaces with Kraken's REST and
WebSocket APIs for order management and execution. The client uses Rust-based HTTP and
WebSocket clients exposed via PyO3 for performance.

"""

import asyncio
from typing import Any
from typing import cast

from nautilus_trader.adapters.kraken.config import KrakenExecClientConfig
from nautilus_trader.adapters.kraken.constants import KRAKEN_VENUE
from nautilus_trader.adapters.kraken.providers import KrakenInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import KrakenEnvironment
from nautilus_trader.core.nautilus_pyo3 import KrakenProductType
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
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
from nautilus_trader.model.orders import Order


class KrakenExecutionClient(LiveExecutionClient):
    """
    Execution client for Kraken exchange.

    Provides order management and execution via Kraken's REST and WebSocket APIs.
    Supports both Spot and Futures markets through separate HTTP clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client_spot : nautilus_pyo3.KrakenSpotHttpClient, optional
        The Kraken Spot HTTP client.
    http_client_futures : nautilus_pyo3.KrakenFuturesHttpClient, optional
        The Kraken Futures HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : KrakenInstrumentProvider
        The instrument provider.
    config : KrakenExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client_spot: nautilus_pyo3.KrakenSpotHttpClient | None,
        http_client_futures: nautilus_pyo3.KrakenFuturesHttpClient | None,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: KrakenInstrumentProvider,
        config: KrakenExecClientConfig,
        name: str | None,
    ) -> None:
        product_types = list(config.product_types or (KrakenProductType.SPOT,))

        # Determine account type based on product types
        if set(product_types) == {KrakenProductType.SPOT}:
            self._account_type = AccountType.CASH
        else:
            self._account_type = AccountType.MARGIN

        super().__init__(
            loop=loop,
            client_id=ClientId(name or KRAKEN_VENUE.value),
            venue=KRAKEN_VENUE,
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
        self._product_types = product_types

        self._log.info(f"Account type: {self._account_type.name}", LogColor.BLUE)
        self._log.info(f"Product types: {[str(p) for p in self._product_types]}", LogColor.BLUE)
        self._log.info(f"{config.environment=}", LogColor.BLUE)
        self._log.info(f"{config.http_proxy_url=}", LogColor.BLUE)
        self._log.info(f"{config.ws_proxy_url=}", LogColor.BLUE)

        # Set account ID
        account_id = AccountId(f"{name or KRAKEN_VENUE.value}-UNIFIED")
        self._set_account_id(account_id)

        # Create pyo3 account ID for Rust HTTP client
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        # HTTP API clients
        self._http_client_spot = http_client_spot
        self._http_client_futures = http_client_futures

        # Log API keys for configured clients
        if http_client_spot is not None:
            masked_key = http_client_spot.api_key_masked
            self._log.info(f"Spot REST API key {masked_key}", LogColor.BLUE)
        if http_client_futures is not None:
            masked_key = http_client_futures.api_key_masked
            self._log.info(f"Futures REST API key {masked_key}", LogColor.BLUE)

        # Determine environment
        environment = config.environment or KrakenEnvironment.MAINNET

        # WebSocket API - Spot (Kraken v2 API)
        self._ws_client_spot: nautilus_pyo3.KrakenSpotWebSocketClient | None = None
        if KrakenProductType.SPOT in product_types:
            self._ws_client_spot = nautilus_pyo3.KrakenSpotWebSocketClient(
                environment=environment,
                base_url=config.base_url_ws_spot,
                heartbeat_secs=config.ws_heartbeat_secs,
            )
            self._log.info(f"Spot WebSocket URL {self._ws_client_spot.url}", LogColor.BLUE)

        # WebSocket API - Futures
        self._ws_client_futures: nautilus_pyo3.KrakenFuturesWebSocketClient | None = None
        if KrakenProductType.FUTURES in product_types:
            self._ws_client_futures = nautilus_pyo3.KrakenFuturesWebSocketClient(
                environment=environment,
                heartbeat_secs=config.ws_heartbeat_secs,
            )
            self._log.info(f"Futures WebSocket URL {self._ws_client_futures.url}", LogColor.BLUE)

        self._ws_client_futures_set: set[asyncio.Future] = set()

    @property
    def kraken_instrument_provider(self) -> KrakenInstrumentProvider:
        return self._instrument_provider  # type: ignore

    def _get_http_client_for_symbol(
        self,
        symbol: str,
    ) -> nautilus_pyo3.KrakenSpotHttpClient | nautilus_pyo3.KrakenFuturesHttpClient | None:
        product_type = nautilus_pyo3.kraken_product_type_from_symbol(symbol)
        if product_type == KrakenProductType.SPOT:
            return self._http_client_spot
        elif product_type == KrakenProductType.FUTURES:
            return self._http_client_futures
        return None

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        await self._cache_instruments()

        # Connect to spot WebSocket if configured
        if self._ws_client_spot is not None:
            instruments_pyo3 = self.kraken_instrument_provider.instruments_pyo3()
            await self._ws_client_spot.connect(instruments_pyo3, self._handle_msg)
            await self._ws_client_spot.wait_until_active(timeout_secs=10.0)
            self._log.info("Connected to spot WebSocket", LogColor.BLUE)

        # Connect to futures WebSocket if configured
        if self._ws_client_futures is not None:
            instruments_pyo3 = self.kraken_instrument_provider.instruments_pyo3()
            await self._ws_client_futures.connect(instruments_pyo3, self._handle_msg)
            self._log.info("Connected to futures WebSocket", LogColor.BLUE)

    async def _disconnect(self) -> None:
        if self._http_client_spot is not None:
            self._http_client_spot.cancel_all_requests()
        if self._http_client_futures is not None:
            self._http_client_futures.cancel_all_requests()

        # Close spot WebSocket
        if self._ws_client_spot is not None and not self._ws_client_spot.is_closed():
            self._log.info("Disconnecting spot websocket")
            await self._ws_client_spot.close()

        # Close futures WebSocket
        if self._ws_client_futures is not None and not self._ws_client_futures.is_closed():
            self._log.info("Disconnecting futures websocket")
            await self._ws_client_futures.close()

        # Cancel any pending futures
        for future in self._ws_client_futures_set:
            if not future.done():
                future.cancel()

        if self._ws_client_futures_set:
            try:
                await asyncio.wait_for(
                    asyncio.gather(*self._ws_client_futures_set, return_exceptions=True),
                    timeout=2.0,
                )
            except TimeoutError:
                self._log.warning("Timeout while waiting for websockets shutdown to complete")

        self._ws_client_futures_set.clear()

    async def _cache_instruments(self) -> None:
        instruments_pyo3 = self.kraken_instrument_provider.instruments_pyo3()

        for inst in instruments_pyo3:
            client = self._get_http_client_for_symbol(str(inst.raw_symbol))
            if client:
                client.cache_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        self._log.debug(
            f"Requesting OrderStatusReports "
            f"{repr(command.instrument_id) if command.instrument_id else ''}"
            "...",
        )

        reports: list[OrderStatusReport] = []

        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            start_ns = command.start.value if command.start else None
            end_ns = command.end.value if command.end else None

            # Request from spot client
            if self._http_client_spot is not None:
                pyo3_reports = await self._http_client_spot.request_order_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    start=start_ns,
                    end=end_ns,
                    open_only=command.open_only,
                )
                for pyo3_report in pyo3_reports:
                    report = OrderStatusReport.from_pyo3(pyo3_report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    reports.append(report)

            # Request from futures client
            if self._http_client_futures is not None:
                pyo3_reports = await self._http_client_futures.request_order_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    start=start_ns,
                    end=end_ns,
                    open_only=command.open_only,
                )
                for pyo3_report in pyo3_reports:
                    report = OrderStatusReport.from_pyo3(pyo3_report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    reports.append(report)

        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReports", e)

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
        self._log.debug(
            f"Requesting FillReports "
            f"{repr(command.instrument_id) if command.instrument_id else ''}"
            "...",
        )

        reports: list[FillReport] = []

        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            start_ns = command.start.value if command.start else None
            end_ns = command.end.value if command.end else None

            # Request from spot client
            if self._http_client_spot is not None:
                pyo3_reports = await self._http_client_spot.request_fill_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    start=start_ns,
                    end=end_ns,
                )
                for pyo3_report in pyo3_reports:
                    report = FillReport.from_pyo3(pyo3_report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    reports.append(report)

            # Request from futures client
            if self._http_client_futures is not None:
                pyo3_reports = await self._http_client_futures.request_fill_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    start=start_ns,
                    end=end_ns,
                )
                for pyo3_report in pyo3_reports:
                    report = FillReport.from_pyo3(pyo3_report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    reports.append(report)

        except Exception as e:
            self._log.exception("Failed to generate FillReports", e)

        self._log_report_receipt(len(reports), "FillReport", LogLevel.INFO)

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        self._log.debug("Requesting PositionStatusReports...")

        reports: list[PositionStatusReport] = []

        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            # Only futures has position reports
            if self._http_client_futures is not None:
                pyo3_reports = await self._http_client_futures.request_position_status_reports(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                )
                for pyo3_report in pyo3_reports:
                    report = PositionStatusReport.from_pyo3(pyo3_report)
                    self._log.debug(f"Received {report}", LogColor.MAGENTA)
                    reports.append(report)

        except Exception as e:
            self._log.exception("Failed to generate PositionStatusReports", e)

        self._log_report_receipt(
            len(reports),
            "PositionStatusReport",
            command.log_receipt_level,
        )

        return reports

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        symbol = order.instrument_id.symbol.value
        product_type = nautilus_pyo3.kraken_product_type_from_symbol(symbol)
        client = self._get_http_client_for_symbol(symbol)

        if client is None:
            self._log.error(f"No HTTP client available for symbol {symbol}")
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=f"No HTTP client for product type {product_type}",
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

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_time_in_force = (
            time_in_force_to_pyo3(order.time_in_force)
            if order.time_in_force
            else nautilus_pyo3.TimeInForce.GTC
        )
        pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
        pyo3_trigger_price = (
            nautilus_pyo3.Price.from_str(str(order.trigger_price))
            if hasattr(order, "trigger_price") and order.trigger_price
            else None
        )

        try:
            # Spot and futures clients have different signatures
            if product_type == nautilus_pyo3.KrakenProductType.FUTURES:
                futures_client = cast(nautilus_pyo3.KrakenFuturesHttpClient, client)
                pyo3_report = await futures_client.submit_order(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    order_side=pyo3_order_side,
                    order_type=pyo3_order_type,
                    quantity=pyo3_quantity,
                    time_in_force=pyo3_time_in_force,
                    price=pyo3_price,
                    trigger_price=pyo3_trigger_price,
                    reduce_only=order.is_reduce_only,
                    post_only=order.is_post_only,
                )
            else:
                spot_client = cast(nautilus_pyo3.KrakenSpotHttpClient, client)
                pyo3_report = await spot_client.submit_order(
                    account_id=self.pyo3_account_id,
                    instrument_id=pyo3_instrument_id,
                    client_order_id=pyo3_client_order_id,
                    order_side=pyo3_order_side,
                    order_type=pyo3_order_type,
                    quantity=pyo3_quantity,
                    time_in_force=pyo3_time_in_force,
                    price=pyo3_price,
                    reduce_only=order.is_reduce_only,
                    post_only=order.is_post_only,
                )

            report = OrderStatusReport.from_pyo3(pyo3_report)

            self.generate_order_accepted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
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

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        # Submit orders individually since Kraken doesn't have a batch order API
        for order in command.order_list.orders:
            await self._submit_order(
                SubmitOrder(
                    trader_id=command.trader_id,
                    strategy_id=command.strategy_id,
                    order=order,
                    command_id=command.id,
                    ts_init=command.ts_init,
                ),
            )

    async def _modify_order(self, command: ModifyOrder) -> None:
        # Kraken doesn't support order modification directly
        # Would need to cancel and resubmit
        order: Order | None = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"{command.client_order_id!r} not found in cache")
            return

        self._log.warning(
            "Kraken does not support order modification. Please cancel and resubmit the order.",
        )

        self.generate_order_modify_rejected(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            venue_order_id=order.venue_order_id,
            reason="Order modification not supported by Kraken",
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

        symbol = command.instrument_id.symbol.value
        client = self._get_http_client_for_symbol(symbol)

        if client is None:
            self._log.error(f"No HTTP client available for symbol {symbol}")
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

        try:
            pyo3_report = await client.cancel_order(
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
            )

            report = OrderStatusReport.from_pyo3(pyo3_report)

            self.generate_order_canceled(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=report.ts_last,
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
                f"Kraken does not support order_side filtering for cancel all orders; "
                f"ignoring order_side={order_side_to_str(command.order_side)} and canceling all orders",
            )

        symbol = command.instrument_id.symbol.value
        client = self._get_http_client_for_symbol(symbol)

        if client is None:
            self._log.error(f"No HTTP client available for symbol {symbol}")
            return

        try:
            # Futures client requires instrument_id parameter, spot does not
            if isinstance(client, nautilus_pyo3.KrakenFuturesHttpClient):
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )
                count = await client.cancel_all_orders(pyo3_instrument_id)
            else:
                count = await client.cancel_all_orders()
            self._log.info(f"Cancelled {count} orders for {command.instrument_id}")
        except Exception as e:
            self._log.error(f"Failed to cancel all orders for {command.instrument_id}: {e}")

    def _handle_msg(self, msg: Any) -> None:  # noqa: C901 (too complex)
        try:
            if isinstance(msg, nautilus_pyo3.AccountState):
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
            elif isinstance(msg, bytes):
                self._log.debug(f"Received bytes message: {msg.decode('utf-8')}", LogColor.MAGENTA)
            else:
                self._log.debug(f"Received unhandled message type: {type(msg)}")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

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
        elif report.order_status == OrderStatus.ACCEPTED:
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

    def _handle_position_status_report_pyo3(
        self,
        msg: nautilus_pyo3.PositionStatusReport,
    ) -> None:
        report = PositionStatusReport.from_pyo3(msg)
        self._log.debug(f"Received {report}", LogColor.MAGENTA)
