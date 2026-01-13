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
from typing import Any

from nautilus_trader.adapters.deribit.config import DeribitExecClientConfig
from nautilus_trader.adapters.deribit.constants import DERIBIT_EXECUTION_SESSION_NAME
from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.adapters.deribit.providers import DeribitInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.secure import mask_api_key
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderCanceled
from nautilus_trader.model.events import OrderCancelRejected
from nautilus_trader.model.events import OrderExpired
from nautilus_trader.model.events import OrderModifyRejected
from nautilus_trader.model.events import OrderUpdated
from nautilus_trader.model.functions import order_type_to_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId


class DeribitExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Deribit cryptocurrency exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : nautilus_pyo3.DeribitHttpClient
        The Deribit HTTP client for REST API operations.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : DeribitInstrumentProvider
        The instrument provider.
    config : DeribitExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: nautilus_pyo3.DeribitHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: DeribitInstrumentProvider,
        config: DeribitExecClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or DERIBIT_VENUE.value),
            venue=DERIBIT_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._instrument_provider: DeribitInstrumentProvider = instrument_provider

        # Configuration
        self._config = config
        instrument_kinds = (
            [i.name.upper() for i in config.instrument_kinds] if config.instrument_kinds else None
        )
        self._log.info(f"config.instrument_kinds={instrument_kinds}", LogColor.BLUE)
        self._log.info(f"{config.is_testnet=}", LogColor.BLUE)
        self._log.info(f"{config.http_timeout_secs=}", LogColor.BLUE)
        self._log.info(f"{config.max_retries=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_initial_ms=}", LogColor.BLUE)
        self._log.info(f"{config.retry_delay_max_ms=}", LogColor.BLUE)

        # Set account ID
        account_id = AccountId(f"{name or DERIBIT_VENUE.value}-master")
        self._set_account_id(account_id)

        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)
        self._http_client = http_client
        self._ws_client = nautilus_pyo3.DeribitWebSocketClient.with_credentials(
            is_testnet=config.is_testnet,
            account_id=self.pyo3_account_id,
        )

        if config.api_key:
            masked_key = mask_api_key(config.api_key)
            self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

    async def _connect(self) -> None:
        self._log.info("Connecting...")
        await self._instrument_provider.initialize()

        # Get PyO3 instruments for WebSocket cache (needed for order routing)
        # Must use instruments_pyo3() as the WebSocket client expects PyO3 types with type_str
        instruments = self._instrument_provider.instruments_pyo3()
        self._log.info(f"Caching {len(instruments)} instruments for WebSocket")

        # Connect WebSocket with instruments and callback dispatch
        self._log.info("Connecting WebSocket for execution...")
        await self._ws_client.connect(
            instruments=instruments,
            callback=self._handle_ws_message,
        )

        # Authenticate for private operations (buy/sell/edit/cancel)
        self._log.info("Authenticating WebSocket session...")
        await self._ws_client.authenticate_session(DERIBIT_EXECUTION_SESSION_NAME)

        # Wait for authentication to complete (30 second timeout)
        await self._ws_client.wait_until_active(timeout_secs=30.0)
        self._log.info("WebSocket authenticated", LogColor.GREEN)

        # Fetch initial account state
        try:
            account_state = await self._http_client.request_account_state(
                self.pyo3_account_id,
            )
            self._handle_account_state(account_state)
            self._log.info("Received initial account state", LogColor.GREEN)
        except Exception as e:
            self._log.error(f"Failed to fetch initial account state: {e}")

        self._log.info("Connected", LogColor.GREEN)

    async def _disconnect(self) -> None:
        self._log.info("Disconnecting...")
        if self._ws_client:
            await self._ws_client.close()
        self._log.info("Disconnected", LogColor.GREEN)

    def _handle_ws_message(self, msg: Any) -> None:
        """
        Handle incoming WebSocket messages (order events, fills, data, etc.).
        """
        try:
            msg_type = type(msg).__name__
            handler = self._get_message_handler(msg_type)
            if handler:
                handler(msg)
        except Exception as e:
            self._log.error(f"Error handling WebSocket message: {e}")

    def _get_message_handler(self, msg_type: str):
        """
        Return the appropriate handler for a message type.
        """
        handlers = {
            "OrderAccepted": self._handle_order_accepted,
            "OrderCanceled": self._handle_order_canceled,
            "OrderExpired": self._handle_order_expired,
            "OrderUpdated": self._handle_order_updated,
            "OrderCancelRejected": self._handle_order_cancel_rejected,
            "OrderModifyRejected": self._handle_order_modify_rejected,
            "OrderStatusReport": self._handle_order_status_report,
            "FillReport": self._handle_fill_report,
            "OrderRejected": self._handle_order_rejected,
            "AccountState": self._handle_account_state,
        }
        return handlers.get(msg_type)

    def _handle_order_status_report(self, msg: Any) -> None:
        """
        Handle OrderStatusReport message.
        """
        report = OrderStatusReport.from_pyo3(msg)
        self._send_order_status_report(report)
        self._log.debug(f"Received OrderStatusReport: {report.client_order_id}")

    def _handle_fill_report(self, msg: Any) -> None:
        report = FillReport.from_pyo3(msg)
        self._send_fill_report(report)
        self._log.debug(f"Received FillReport: {report.trade_id}")

    def _handle_order_rejected(self, msg: Any) -> None:
        self._log.warning(f"Order rejected: {msg}")

    def _handle_account_state(self, msg: nautilus_pyo3.AccountState) -> None:
        account_state = AccountState.from_dict(msg.to_dict())
        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=account_state.is_reported,
            ts_event=account_state.ts_event,
        )

    def _handle_order_accepted(self, pyo3_event: nautilus_pyo3.OrderAccepted) -> None:
        event = OrderAccepted.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._log.debug(f"OrderAccepted: {event.client_order_id}")

    def _handle_order_canceled(self, pyo3_event: nautilus_pyo3.OrderCanceled) -> None:
        event = OrderCanceled.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._log.debug(f"OrderCanceled: {event.client_order_id}")

    def _handle_order_expired(self, pyo3_event: nautilus_pyo3.OrderExpired) -> None:
        event = OrderExpired.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._log.debug(f"OrderExpired: {event.client_order_id}")

    def _handle_order_updated(self, pyo3_event: nautilus_pyo3.OrderUpdated) -> None:
        event = OrderUpdated.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._log.debug(f"OrderUpdated: {event.client_order_id}")

    def _handle_order_cancel_rejected(self, pyo3_event: nautilus_pyo3.OrderCancelRejected) -> None:
        event = OrderCancelRejected.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._log.warning(f"OrderCancelRejected: {event.client_order_id} - {event.reason}")

    def _handle_order_modify_rejected(self, pyo3_event: nautilus_pyo3.OrderModifyRejected) -> None:
        event = OrderModifyRejected.from_dict(pyo3_event.to_dict())
        self._send_order_event(event)
        self._log.warning(f"OrderModifyRejected: {event.client_order_id} - {event.reason}")

    async def _query_account(self, command: QueryAccount) -> None:
        self._log.debug(f"Querying account state: {command}")
        try:
            account_state = await self._http_client.request_account_state(
                self.pyo3_account_id,
            )
            self._handle_account_state(account_state)
        except Exception as e:
            self._log.error(f"Failed to query account state: {e}")

    async def _submit_order(self, command: SubmitOrder) -> None:
        order = command.order

        if order.is_closed:
            self._log.warning(f"Cannot submit already closed order: {order}")
            return

        # Convert Python types to PyO3 types
        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
        pyo3_order_type = order_type_to_pyo3(order.order_type)
        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
        pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None

        time_in_force_str = (
            self._map_time_in_force(order.time_in_force) if order.time_in_force else None
        )

        try:
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

            pyo3_order_side = nautilus_pyo3.OrderSide.from_str(order.side.name)
            await self._ws_client.submit_order(
                order_side=pyo3_order_side,
                quantity=pyo3_quantity,
                order_type=pyo3_order_type,
                client_order_id=pyo3_client_order_id,
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
                price=pyo3_price,
                time_in_force=time_in_force_str,
                post_only=order.is_post_only,
                reduce_only=order.is_reduce_only,
            )
        except Exception as e:
            self._log.error(f"Failed to submit order: {e}")
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    def _map_time_in_force(self, tif: TimeInForce) -> str:
        mapping = {
            TimeInForce.GTC: "good_til_cancelled",
            TimeInForce.IOC: "immediate_or_cancel",
            TimeInForce.FOK: "fill_or_kill",
            TimeInForce.GTD: "good_til_date",
        }
        return mapping.get(tif, "good_til_cancelled")

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        order_list = command.order_list

        if not order_list.orders:
            self._log.debug("submit_order_list called with empty order list")
            return

        self._log.info(
            f"Submitting order list {order_list.id} with {len(order_list.orders)} orders "
            f"for instrument={command.instrument_id}",
        )

        # Deribit doesn't have native batch order submission
        # Loop through and submit each order individually
        for order in order_list.orders:
            if order.is_closed:
                self._log.warning(f"Skipping closed order: {order.client_order_id}")
                continue

            # Convert Python types to PyO3 types
            pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
            pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)
            pyo3_order_type = order_type_to_pyo3(order.order_type)
            pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(order.quantity))
            pyo3_price = nautilus_pyo3.Price.from_str(str(order.price)) if order.has_price else None

            time_in_force_str = (
                self._map_time_in_force(order.time_in_force) if order.time_in_force else None
            )

            try:
                # Generate OrderSubmitted event first
                self.generate_order_submitted(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    ts_event=self._clock.timestamp_ns(),
                )

                self._log.info(
                    f"Submitting order from list: {order.client_order_id} "
                    f"({order.side.name} {order.quantity} @ {order.price})",
                )

                pyo3_order_side = nautilus_pyo3.OrderSide.from_str(order.side.name)
                await self._ws_client.submit_order(
                    order_side=pyo3_order_side,
                    quantity=pyo3_quantity,
                    order_type=pyo3_order_type,
                    client_order_id=pyo3_client_order_id,
                    trader_id=pyo3_trader_id,
                    strategy_id=pyo3_strategy_id,
                    instrument_id=pyo3_instrument_id,
                    price=pyo3_price,
                    time_in_force=time_in_force_str,
                    post_only=order.is_post_only,
                    reduce_only=order.is_reduce_only,
                )
            except Exception as e:
                self._log.error(f"Failed to submit order from list: {e}")
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _modify_order(self, command: ModifyOrder) -> None:
        order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"Order not found: {command.client_order_id}")
            self.generate_order_modify_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"Order not found: {command.client_order_id}",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        if order.venue_order_id is None:
            self._log.error(
                f"Cannot modify order without venue_order_id: {command.client_order_id}",
            )
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=None,
                reason=f"Cannot modify order without venue_order_id: {command.client_order_id}",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)

        # Use command values if provided, otherwise fall back to existing order values
        price = command.price if command.price else order.price
        quantity = command.quantity if command.quantity else order.quantity

        pyo3_quantity = nautilus_pyo3.Quantity.from_str(str(quantity))
        pyo3_price = nautilus_pyo3.Price.from_str(str(price))

        try:
            self._log.info(
                f"Modifying order {order.client_order_id} (venue: {order.venue_order_id}) "
                f"to price={price} qty={quantity}",
            )

            await self._ws_client.modify_order(
                order_id=order.venue_order_id.value,
                quantity=pyo3_quantity,
                price=pyo3_price,
                client_order_id=pyo3_client_order_id,
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
            )
        except Exception as e:
            self._log.error(f"Failed to modify order: {e}")
            self.generate_order_modify_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_order(self, command: CancelOrder) -> None:
        order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"Order not found: {command.client_order_id}")
            self.generate_order_cancel_rejected(
                strategy_id=command.strategy_id,
                instrument_id=command.instrument_id,
                client_order_id=command.client_order_id,
                venue_order_id=command.venue_order_id,
                reason=f"Order not found: {command.client_order_id}",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        if order.venue_order_id is None:
            self._log.error(
                f"Cannot cancel order without venue_order_id: {command.client_order_id}",
            )
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=None,
                reason=f"Cannot cancel order without venue_order_id: {command.client_order_id}",
                ts_event=self._clock.timestamp_ns(),
            )
            return

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)

        try:
            self._log.info(
                f"Canceling order {order.client_order_id} (venue: {order.venue_order_id})",
            )

            await self._ws_client.cancel_order(
                order_id=order.venue_order_id.value,
                client_order_id=pyo3_client_order_id,
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
            )
        except Exception as e:
            self._log.error(f"Failed to cancel order: {e}")
            self.generate_order_cancel_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=order.venue_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        # Deribit doesn't support side filtering - log warning if specified
        if command.order_side != OrderSide.NO_ORDER_SIDE:
            self._log.warning(
                "Deribit cancel_all_by_instrument doesn't support order_side filtering. "
                "Cancelling all orders for instrument regardless of side.",
            )

        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(command.instrument_id.value)

        try:
            self._log.info(
                f"Cancelling all orders for instrument {command.instrument_id}",
            )

            await self._ws_client.cancel_all_orders(
                instrument_id=pyo3_instrument_id,
                order_type=None,  # Cancel all order types
            )
        except Exception as e:
            self._log.error(f"Failed to cancel all orders: {e}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        if not command.cancels:
            self._log.debug("batch_cancel_orders called with empty cancels list")
            return

        self._log.info(
            f"Batch cancelling {len(command.cancels)} orders for instrument={command.instrument_id}",
        )

        # Deribit doesn't have native batch cancel by order ID
        # Loop through and cancel each order individually
        for cancel in command.cancels:
            order = self._cache.order(cancel.client_order_id)
            if order is None:
                self._log.warning(f"Skipping cancel - order not found: {cancel.client_order_id}")
                self.generate_order_cancel_rejected(
                    strategy_id=cancel.strategy_id,
                    instrument_id=cancel.instrument_id,
                    client_order_id=cancel.client_order_id,
                    venue_order_id=cancel.venue_order_id,
                    reason=f"Order not found: {cancel.client_order_id}",
                    ts_event=self._clock.timestamp_ns(),
                )
                continue

            if order.venue_order_id is None:
                self._log.warning(
                    f"Skipping cancel for {cancel.client_order_id} - no venue_order_id",
                )
                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=None,
                    reason=f"Cannot cancel order without venue_order_id: {cancel.client_order_id}",
                    ts_event=self._clock.timestamp_ns(),
                )
                continue

            # Convert Python types to PyO3 types
            pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
            pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
            pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
            pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)

            try:
                self._log.info(
                    f"Batch cancel: {order.client_order_id} (venue: {order.venue_order_id})",
                )

                await self._ws_client.cancel_order(
                    order_id=order.venue_order_id.value,
                    client_order_id=pyo3_client_order_id,
                    trader_id=pyo3_trader_id,
                    strategy_id=pyo3_strategy_id,
                    instrument_id=pyo3_instrument_id,
                )
            except Exception as e:
                self._log.error(
                    f"Batch cancel failed: order_id={order.venue_order_id}, "
                    f"client_order_id={order.client_order_id}, error={e}",
                )
                self.generate_order_cancel_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    venue_order_id=order.venue_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _query_order(self, command: QueryOrder) -> None:
        """
        Query order state via WebSocket get_order_state.
        """
        order = self._cache.order(command.client_order_id)
        if order is None:
            self._log.error(f"Order not found: {command.client_order_id}")
            return

        if order.venue_order_id is None:
            self._log.error(f"Cannot query order without venue_order_id: {command.client_order_id}")
            return

        pyo3_trader_id = nautilus_pyo3.TraderId.from_str(order.trader_id.value)
        pyo3_strategy_id = nautilus_pyo3.StrategyId.from_str(order.strategy_id.value)
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(order.instrument_id.value)
        pyo3_client_order_id = nautilus_pyo3.ClientOrderId(order.client_order_id.value)

        try:
            self._log.info(
                f"Querying order {order.client_order_id} (venue: {order.venue_order_id})",
            )

            await self._ws_client.query_order(
                order_id=order.venue_order_id.value,
                client_order_id=pyo3_client_order_id,
                trader_id=pyo3_trader_id,
                strategy_id=pyo3_strategy_id,
                instrument_id=pyo3_instrument_id,
            )
        except Exception as e:
            self._log.error(f"Failed to query order: {e}")

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        self._log.warning(
            f"generate_order_status_report not yet implemented (instrument_id={command.instrument_id})",
        )
        return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        reports: list[OrderStatusReport] = []
        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            # command.start/end are UnixNanos (has as_u64 method)
            start = command.start.as_u64() if command.start else None
            end = command.end.as_u64() if command.end else None

            pyo3_reports = await self._http_client.request_order_status_reports(
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
                start=start,
                end=end,
                open_only=command.open_only,
            )

            for pyo3_report in pyo3_reports:
                report = OrderStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except Exception as e:
            self._log.exception("Failed to generate OrderStatusReports", e)

        return reports

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        reports: list[FillReport] = []
        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            start = command.start.as_u64() if command.start else None
            end = command.end.as_u64() if command.end else None

            pyo3_reports = await self._http_client.request_fill_reports(
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
                start=start,
                end=end,
            )

            for pyo3_report in pyo3_reports:
                report = FillReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except Exception as e:
            self._log.exception("Failed to generate FillReports", e)

        return reports

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        reports: list[PositionStatusReport] = []
        try:
            pyo3_instrument_id = None
            if command.instrument_id:
                pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(
                    command.instrument_id.value,
                )

            pyo3_reports = await self._http_client.request_position_status_reports(
                account_id=self.pyo3_account_id,
                instrument_id=pyo3_instrument_id,
            )

            for pyo3_report in pyo3_reports:
                report = PositionStatusReport.from_pyo3(pyo3_report)
                self._log.debug(f"Received {report}", LogColor.MAGENTA)
                reports.append(report)
        except Exception as e:
            self._log.exception("Failed to generate PositionStatusReports", e)

        return reports
