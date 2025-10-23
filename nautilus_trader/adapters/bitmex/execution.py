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

from nautilus_trader.adapters.bitmex.config import BitmexExecClientConfig
from nautilus_trader.adapters.bitmex.constants import BITMEX_VENUE
from nautilus_trader.adapters.bitmex.providers import BitmexInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.functions import contingency_type_to_pyo3
from nautilus_trader.model.functions import order_side_to_pyo3
from nautilus_trader.model.functions import order_type_to_pyo3
from nautilus_trader.model.functions import time_in_force_to_pyo3
from nautilus_trader.model.functions import trigger_type_to_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import Order


class BitmexExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the BitMEX centralized crypto exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.BitMEXHttpClient
        The BitMEX HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : BitmexInstrumentProvider
        The instrument provider.
    config : BitmexExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.BitmexHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: BitmexInstrumentProvider,
        config: BitmexExecClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or BITMEX_VENUE.value),
            venue=BITMEX_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # TBD
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._config = config

        self._log.info(f"{config.testnet=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)
        self._log.info(f"{config.recv_window_ms=}", LogColor.BLUE)
        self._log.info(f"{config.max_requests_per_second=}", LogColor.BLUE)
        self._log.info(f"{config.max_requests_per_minute=}", LogColor.BLUE)
        self._log.info(f"{config.canceller_pool_size=}", LogColor.BLUE)

        # Set initial account ID (will be updated with actual account number on connect)
        self._account_id_prefix = name or BITMEX_VENUE.value
        account_id = AccountId(f"{self._account_id_prefix}-master")  # Temporary, like OKX
        self._set_account_id(account_id)

        # Create pyo3 account ID for Rust HTTP client
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # Determine HTTP base URL for canceller
        http_url = config.base_url_http or nautilus_pyo3.get_bitmex_http_base_url(config.testnet)

        self._canceller = nautilus_pyo3.CancelBroadcaster(
            pool_size=config.canceller_pool_size,
            api_key=config.api_key,
            api_secret=config.api_secret,
            base_url=http_url,
            testnet=config.testnet,
            timeout_secs=config.http_timeout_secs,
            max_retries=config.max_retries,
            retry_delay_ms=config.retry_delay_initial_ms,
            retry_delay_max_ms=config.retry_delay_max_ms,
            recv_window_ms=config.recv_window_ms,
            max_requests_per_second=config.max_requests_per_second,
            max_requests_per_minute=config.max_requests_per_minute,
        )

        # WebSocket API
        ws_url = config.base_url_ws or nautilus_pyo3.get_bitmex_ws_url(config.testnet)

        self._ws_client = nautilus_pyo3.BitmexWebSocketClient(
            url=ws_url,
            api_key=config.api_key,
            api_secret=config.api_secret,
            account_id=self.pyo3_account_id,
            heartbeat=30,
            testnet=config.testnet,
        )
        self._ws_client_futures: set[asyncio.Future] = set()
        self._log.info(f"WebSocket URL {ws_url}", LogColor.BLUE)

    def _log_runtime_error(self, message: str) -> None:
        self._log.error(message, LogColor.RED)
        raise RuntimeError(message)

    @property
    def instrument_provider(self) -> BitmexInstrumentProvider:
        return self._instrument_provider  # type: ignore

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self._instrument_provider.instruments_pyo3()  # type: ignore

        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)
            self._canceller.add_instrument(inst)  # type: ignore[attr-defined]

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()

        await self._update_account_state()
        await self._await_account_registered()

        self._log.info("BitMEX API key authenticated", LogColor.GREEN)

        # Check BitMEX-Nautilus clock sync
        server_time: int = await self._http_client.http_get_server_time()
        self._log.info(f"BitMEX server time {server_time} UNIX (ms)")

        nautilus_time: int = self._clock.timestamp_ms()
        self._log.info(f"Nautilus clock time {nautilus_time} UNIX (ms)")

        self._ws_client.set_account_id(self.pyo3_account_id)

        instruments = self._instrument_provider.instruments_pyo3()  # type: ignore

        await self._ws_client.connect(
            instruments,
            self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_client.wait_until_active(timeout_secs=10.0)
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)

        await self._canceller.start()
        self._log.info("Started cancel broadcaster", LogColor.BLUE)

        try:
            # Subscribe to authenticated channels for execution updates
            await self._ws_client.subscribe_orders()
            await self._ws_client.subscribe_executions()
            await self._ws_client.subscribe_positions()
            await self._ws_client.subscribe_margin()
            await self._ws_client.subscribe_wallet()
        except Exception as e:
            self._log.error(f"Failed to subscribe to authenticated channels: {e}")

    async def _update_account_state(self) -> None:
        # First get the margin data to extract the actual account number
        account_number = await self._http_client.http_get_margin("XBt")

        # Update account ID with actual account number from BitMEX
        if account_number:
            actual_account_id = AccountId(f"{self._account_id_prefix}-{account_number}")
            self._set_account_id(actual_account_id)
            self.pyo3_account_id = nautilus_pyo3.AccountId(actual_account_id.value)
            self._log.info(f"Updated account ID to {actual_account_id}", LogColor.BLUE)

        # Now request the account state with the correct account ID
        pyo3_account_state = await self._http_client.request_account_state(self.pyo3_account_id)
        account_state = AccountState.from_dict(pyo3_account_state.to_dict())

        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        if account_state.balances:
            self._log.info(
                f"Generated account state with {len(account_state.balances)} balance(s)",
            )

    async def _disconnect(self) -> None:
        await self._canceller.stop()
        self._log.info("Stopped cancel broadcaster", LogColor.BLUE)

        if not self._ws_client.is_closed():
            try:
                # Unsubscribe from authenticated channels before disconnecting
                await self._ws_client.unsubscribe_orders()
                await self._ws_client.unsubscribe_executions()
                await self._ws_client.unsubscribe_positions()
                await self._ws_client.unsubscribe_margin()
                await self._ws_client.unsubscribe_wallet()
            except Exception as e:
                self._log.error(f"Failed to unsubscribe from channels: {e}")

        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)

        # Shutdown websocket
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting websocket")

            await self._ws_client.close()

            self._log.info(
                f"Disconnected from {self._ws_client.url}",
                LogColor.BLUE,
            )

        # Cancel any pending futures
        await cancel_tasks_with_timeout(
            self._ws_client_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )
        self._ws_client_futures.clear()

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        """
        Generate a list of `OrderStatusReport`s with optional query filters.
        """
        try:
            pyo3_reports = await self._http_client.request_order_status_reports(
                instrument_id=command.instrument_id,
                open_only=command.open_only,
                limit=None,
            )

            reports: list[OrderStatusReport] = []

            for pyo3_report in pyo3_reports:
                reports.append(OrderStatusReport.from_pyo3(pyo3_report))

            len_reports = len(reports)
            plural = "" if len_reports == 1 else "s"
            receipt_log = f"Received {len(reports)} OrderStatusReport{plural}"

            if command.log_receipt_level == LogLevel.INFO:
                self._log.info(receipt_log)
            else:
                self._log.debug(receipt_log)

            return reports
        except Exception as e:
            self._log.error(f"Failed to generate order status reports: {e}")
            return []

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        """
        Generate an `OrderStatusReport` for the specified order.
        """
        # TODO: Implement fetching specific order from BitMEX
        self._log.warning("Order status report generation not yet implemented")
        return None

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        """
        Generate a list of `FillReport`s with optional query filters.
        """
        try:
            pyo3_reports = await self._http_client.request_fill_reports(
                instrument_id=command.instrument_id,
                limit=None,
            )

            reports: list[FillReport] = []

            for pyo3_report in pyo3_reports:
                reports.append(FillReport.from_pyo3(pyo3_report))

            len_reports = len(reports)
            plural = "" if len_reports == 1 else "s"
            self._log.info(f"Received {len(reports)} FillReport{plural}")

            return reports
        except Exception as e:
            self._log.error(f"Failed to generate fill reports: {e}")
            return []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """
        Generate a list of `PositionStatusReport`s with optional query filters.
        """
        try:
            pyo3_reports = await self._http_client.request_position_status_reports()

            reports = []

            for pyo3_report in pyo3_reports:
                reports.append(PositionStatusReport.from_pyo3(pyo3_report))

            len_reports = len(reports)
            plural = "" if len_reports == 1 else "s"
            self._log.info(f"Received {len(reports)} PositionStatusReport{plural}")

            return reports
        except Exception as e:
            self._log.error(f"Failed to generate position status reports: {e}")
            return []

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        if order.is_quote_quantity:
            reason = "UNSUPPORTED_QUOTE_QUANTITY"
            self._log.error(
                f"Cannot submit order {order.client_order_id}: {reason}",
            )
            self.generate_order_denied(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=reason,
                ts_event=self._clock.timestamp_ns(),
            )
            return

        # Generate OrderSubmitted event here to ensure correct event sequencing
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_order_side = order_side_to_pyo3(order.side)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_time_in_force = time_in_force_to_pyo3(order.time_in_force)
        pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None
        pyo3_trigger_price = (
            nautilus_pyo3.Price.from_str(str(order.trigger_price))
            if order.has_trigger_price
            else None
        )
        pyo3_trigger_type = (
            trigger_type_to_pyo3(order.trigger_type)
            if order.has_trigger_price and hasattr(order, "trigger_type")
            else None
        )
        pyo3_display_qty = (
            nautilus_pyo3.Quantity.from_str(str(order.display_qty))
            if hasattr(order, "display_qty") and order.display_qty
            else None
        )

        pyo3_contingency_type = None
        pyo3_order_list_id = None

        if order.order_list_id is not None:
            pyo3_order_list_id = nautilus_pyo3.OrderListId(order.order_list_id.value)

        if order.contingency_type in (ContingencyType.OCO, ContingencyType.OTO):
            pyo3_contingency_type = contingency_type_to_pyo3(order.contingency_type)

        try:
            await self._http_client.submit_order(
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                order_side=pyo3_order_side,
                order_type=pyo3_order_type,
                quantity=pyo3_quantity,
                time_in_force=pyo3_time_in_force,
                price=pyo3_price,
                trigger_price=pyo3_trigger_price,
                trigger_type=pyo3_trigger_type,
                display_qty=pyo3_display_qty,
                post_only=order.is_post_only,
                reduce_only=order.is_reduce_only,
                order_list_id=pyo3_order_list_id,
                contingency_type=pyo3_contingency_type,
            )
        except Exception as e:
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        for order in command.order_list.orders:
            submit_command = SubmitOrder(
                trader_id=command.trader_id,
                strategy_id=command.strategy_id,
                order=order,
                command_id=UUID4(),
                ts_init=self._clock.timestamp_ns(),
                position_id=command.position_id,
                client_id=command.client_id,
                params=command.params,
            )
            await self._submit_order(submit_command)

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

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
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
        pyo3_trigger_price = (
            nautilus_pyo3.Price.from_str(str(command.trigger_price))
            if command.trigger_price
            else None
        )

        try:
            await self._http_client.modify_order(
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
                quantity=pyo3_quantity,
                price=pyo3_price,
                trigger_price=pyo3_trigger_price,
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
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)

        try:
            await self._canceller.broadcast_cancel(
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
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        pyo3_order_side = order_side_to_pyo3(command.order_side) if command.order_side else None

        try:
            await self._canceller.broadcast_cancel_all(
                instrument_id=pyo3_instrument_id,
                order_side=pyo3_order_side,
            )
        except Exception as e:
            # Generate cancel rejected for all open orders
            orders_open: list[Order] = self._cache.orders_open(
                instrument_id=command.instrument_id,
            )
            for open_order in orders_open:
                if open_order.is_closed:
                    continue
                self.generate_order_cancel_rejected(
                    strategy_id=open_order.strategy_id,
                    instrument_id=open_order.instrument_id,
                    client_order_id=open_order.client_order_id,
                    venue_order_id=open_order.venue_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        valid_cancels: list[CancelOrder] = []
        client_order_ids: list[ClientOrderId] = []

        for cancel in command.cancels:
            order = self._cache.order(cancel.client_order_id)
            if order is None:
                self._log.error(f"{cancel.client_order_id!r} not found to cancel")
                continue

            if order.is_closed:
                self._log.warning(
                    f"BatchCancelOrders command for {cancel.client_order_id!r} when order already {order.status_string()} "
                    "(will not send to exchange)",
                )
                continue

            valid_cancels.append(cancel)
            client_order_ids.append(cancel.client_order_id)

        if not valid_cancels:
            self._log.info("No valid orders to cancel in batch")
            return

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)
        pyo3_client_order_ids = [
            nautilus_pyo3.ClientOrderId.from_str(cid.value) for cid in client_order_ids
        ]

        try:
            await self._canceller.broadcast_batch_cancel(
                instrument_id=pyo3_instrument_id,
                client_order_ids=pyo3_client_order_ids,
                venue_order_ids=None,
            )
        except Exception as e:
            self._log.error(f"Failed to batch cancel orders: {e}")
            for cancel in valid_cancels:
                self.generate_order_cancel_rejected(
                    strategy_id=cancel.strategy_id,
                    instrument_id=cancel.instrument_id,
                    client_order_id=cancel.client_order_id,
                    venue_order_id=cancel.venue_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _query_order(self, command: QueryOrder) -> None:
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
            pyo3_report = await self._http_client.query_order(
                instrument_id=pyo3_instrument_id,
                client_order_id=pyo3_client_order_id,
                venue_order_id=pyo3_venue_order_id,
            )

            if pyo3_report is None:
                self._log.warning(
                    f"Order not found: client_order_id={command.client_order_id}, "
                    f"venue_order_id={command.venue_order_id}",
                )
                return

            report = OrderStatusReport.from_pyo3(pyo3_report)
            self._send_order_status_report(report)
            self._log.info(f"Queried order {command.client_order_id}")
        except Exception as e:
            self._log.error(f"Failed to query order {command.client_order_id}: {e}")

    def _handle_account_state(self, msg: nautilus_pyo3.AccountState) -> None:
        account_state = AccountState.from_dict(msg.to_dict())
        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=account_state.is_reported,
            ts_event=account_state.ts_event,
        )

    def _handle_msg(self, msg: Any) -> None:
        try:
            if nautilus_pyo3.is_pycapsule(msg):
                pass  # PyCapsules are market data we ignore for the execution client
            elif isinstance(msg, nautilus_pyo3.AccountState):
                self._handle_account_state(msg)
            elif isinstance(msg, nautilus_pyo3.OrderStatusReport):
                self._handle_order_status_report_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.OrderUpdated):
                self._handle_order_updated_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.FillReport):
                self._handle_fill_report_pyo3(msg)
            elif isinstance(msg, nautilus_pyo3.PositionStatusReport):
                self._handle_position_status_report_pyo3(msg)
            else:
                self._log.warning(f"Received unhandled message type: {type(msg)}")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

    def _handle_fill_reports_list(self, reports: list) -> None:
        for fill_report in reports:
            self._handle_fill_report_pyo3(fill_report)

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

    def _handle_order_updated_pyo3(
        self,
        pyo3_event: nautilus_pyo3.OrderUpdated,
    ) -> None:
        client_order_id = ClientOrderId(pyo3_event.client_order_id.value)

        order = self._cache.order(client_order_id)
        if not order:
            self._log.warning(
                f"Cannot find order for client_order_id {client_order_id} with "
                f"venue_order_id {pyo3_event.venue_order_id}, ignoring update",
            )
            return

        event_dict = pyo3_event.to_dict()
        event_dict["trader_id"] = order.trader_id.value
        event_dict["strategy_id"] = order.strategy_id.value

        # We use zero as a sentinel indicating no quantity change
        event_qty = Quantity.from_str(event_dict["quantity"])
        if event_qty == 0:
            event_dict["quantity"] = str(order.quantity)

        event = OrderUpdated.from_dict(event_dict)
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
            pass  # Handled by submit_order
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
            # Check if this is a post-only order that was canceled (BitMEX specific behavior)
            # BitMEX cancels post-only orders instead of rejecting them when they would cross the spread
            # The specific message is "Order had execInst of ParticipateDoNotInitiate"
            is_post_only_rejection = (
                report.cancel_reason and "ParticipateDoNotInitiate" in report.cancel_reason
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
        pyo3_report: nautilus_pyo3.PositionStatusReport,
    ) -> None:
        _report = PositionStatusReport.from_pyo3(pyo3_report)

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
