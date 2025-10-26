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

from __future__ import annotations

import asyncio
from typing import Any

from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
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
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId


class HyperliquidExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Hyperliquid decentralized exchange (DEX).

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : Any
        The Hyperliquid HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : HyperliquidInstrumentProvider
        The instrument provider.
    config : HyperliquidExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: Any,  # TODO: Replace with actual HyperliquidHttpClient when available
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: HyperliquidInstrumentProvider,
        config: HyperliquidExecClientConfig,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or HYPERLIQUID_VENUE.value),
            venue=HYPERLIQUID_VENUE,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # Multi-currency account
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Configuration
        self._config = config
        self._client = client
        self._instrument_provider: HyperliquidInstrumentProvider = instrument_provider

        # Log configuration details
        self._log.info(f"config.testnet={config.testnet}", LogColor.BLUE)
        self._log.info(f"config.http_timeout_secs={config.http_timeout_secs}", LogColor.BLUE)

        # Set initial account ID (following OKX/BitMEX pattern)
        account_id = AccountId(f"{name or HYPERLIQUID_VENUE.value}-master")
        self._set_account_id(account_id)

        # Placeholder for WebSocket connections
        self._ws_connection = None

        self._log.info("Hyperliquid execution client initialized")

    @property
    def hyperliquid_instrument_provider(self) -> HyperliquidInstrumentProvider:
        return self._instrument_provider

    def _cache_instruments(self) -> None:
        """
        Cache instruments into HTTP client following OKX/BitMEX pattern.
        """
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self._instrument_provider.instruments_pyo3()
        for inst in instruments_pyo3:
            self._client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    # -- CONNECTION HANDLERS -----------------------------------------------------------------------

    async def _connect(self) -> None:
        """
        Connect the client.
        """
        self._log.info("Connecting to Hyperliquid execution...", LogColor.BLUE)

        # Initialize instruments (required for the execution client to work)
        self._log.info("Loading instruments...", LogColor.BLUE)
        await self._instrument_provider.initialize()
        self._cache_instruments()

        # Set account ID on HTTP client for report generation
        self._client.set_account_id(str(self.account_id))

        self._log.info(
            f"Loaded {len(self._instrument_provider.list_all())} instruments",
            LogColor.GREEN,
        )

        # TODO: Implement account state updates when API is available
        # await self._update_account_state()

        # TODO: Implement WebSocket connection when available
        # Placeholder for WebSocket connection setup
        # self._ws_client.set_account_id(self.pyo3_account_id)
        # await self._ws_client.connect(instruments, self._handle_msg)
        # await self._ws_client.wait_until_active(timeout_secs=10.0)
        # await self._ws_client.subscribe_orders()
        # await self._ws_client.subscribe_executions()
        # await self._ws_client.subscribe_positions()
        # await self._ws_client.subscribe_wallet()

        self._log.info("Hyperliquid execution client connected", LogColor.GREEN)

    async def _disconnect(self) -> None:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting from Hyperliquid execution...", LogColor.BLUE)

        # TODO: Implement actual disconnection logic
        if self._ws_connection:
            # Close WebSocket connections
            pass

        await asyncio.sleep(0.1)  # Placeholder
        self._log.info("Hyperliquid execution disconnection completed", LogColor.GREEN)

    # -- COMMANDS -----------------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        """
        Submit an order.

        Parameters
        ----------
        command : SubmitOrder
            The command to submit the order.

        """
        order = command.order

        if order.is_closed:
            self._log.warning(f"Order {order} is already closed")
            return

        # Generate order submitted event, to ensure correct ordering of events
        self.generate_order_submitted(
            strategy_id=order.strategy_id,
            instrument_id=order.instrument_id,
            client_order_id=order.client_order_id,
            ts_event=self._clock.timestamp_ns(),
        )

        try:
            self._log.info(f"Submitting order to Hyperliquid: {order}")

            # Submit via HTTP client with domain types (Rust handles all serialization and parsing)
            # Following BitMEX/Bybit pattern - pass domain types, receive OrderStatusReport
            report = await self._client.submit_order(
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                order_side=order.side,
                order_type=order.order_type,
                quantity=order.quantity,
                time_in_force=order.time_in_force,
                price=order.price,
                trigger_price=order.trigger_price,
                post_only=order.is_post_only,
                reduce_only=order.is_reduce_only,
            )

            self._log.debug(f"Received order status report: {report}")

            # Generate order accepted event based on the report
            self.generate_order_accepted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                venue_order_id=report.venue_order_id,
                ts_event=self._clock.timestamp_ns(),
            )

            self._log.info(
                f"Order {order.client_order_id} accepted, venue_order_id={report.venue_order_id}",
            )

        except Exception as e:
            self._log.error(f"Error submitting order {order.client_order_id}: {e}")
            self.generate_order_rejected(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                reason=str(e),
                ts_event=self._clock.timestamp_ns(),
            )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        """
        Submit an order list.

        Parameters
        ----------
        command : SubmitOrderList
            The command to submit the order list.

        """
        order_list = command.order_list
        orders = order_list.orders

        if not orders:
            self._log.warning("Order list is empty, nothing to submit")
            return

        # Check if all orders are open
        closed_orders = [order for order in orders if order.is_closed]
        if closed_orders:
            self._log.warning(f"Skipping {len(closed_orders)} closed orders in batch")
            orders = [order for order in orders if not order.is_closed]

        if not orders:
            return

        # Generate submitted events for all orders
        now_ns = self._clock.timestamp_ns()
        for order in orders:
            self.generate_order_submitted(
                strategy_id=order.strategy_id,
                instrument_id=order.instrument_id,
                client_order_id=order.client_order_id,
                ts_event=now_ns,
            )

        try:
            self._log.info(f"Submitting {len(orders)} orders to Hyperliquid as batch")

            # Pass Order objects directly to Rust layer
            # This pushes the complexity to the Rust layer for pure Rust execution
            reports = await self._client.submit_orders(orders)

            self._log.debug(f"Received {len(reports)} order status reports")

            # Generate acceptance events for all successfully submitted orders
            for report in reports:
                # Find the corresponding order
                order = next(
                    (o for o in orders if o.client_order_id == report.client_order_id),
                    None,
                )
                if order:
                    self.generate_order_accepted(
                        strategy_id=order.strategy_id,
                        instrument_id=order.instrument_id,
                        client_order_id=order.client_order_id,
                        venue_order_id=report.venue_order_id,
                        ts_event=self._clock.timestamp_ns(),
                    )
                    self._log.info(
                        f"Order {order.client_order_id} accepted, venue_order_id={report.venue_order_id}",
                    )

        except Exception as e:
            self._log.error(f"Error submitting order batch: {e}")
            # Generate rejection events for all orders
            for order in orders:
                self.generate_order_rejected(
                    strategy_id=order.strategy_id,
                    instrument_id=order.instrument_id,
                    client_order_id=order.client_order_id,
                    reason=str(e),
                    ts_event=self._clock.timestamp_ns(),
                )

    async def _modify_order(self, command: ModifyOrder) -> None:
        """
        Modify an order.

        Parameters
        ----------
        command : ModifyOrder
            The command to modify the order.

        """
        self._log.warning(f"Order modification not yet implemented for {command.client_order_id}")

    async def _cancel_order(self, command: CancelOrder) -> None:
        """
        Cancel an order.

        Parameters
        ----------
        command : CancelOrder
            The command to cancel the order.

        """
        self._log.warning(f"Order cancellation not yet implemented for {command.client_order_id}")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        """
        Cancel all orders for an instrument.

        Parameters
        ----------
        command : CancelAllOrders
            The command to cancel all orders.

        """
        instrument_str = (
            f" for {command.instrument_id}" if command.instrument_id is not None else ""
        )
        self._log.warning(f"Cancel all orders not yet implemented{instrument_str}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        """
        Batch cancel orders.

        Parameters
        ----------
        command : BatchCancelOrders
            The command to batch cancel orders.

        """
        self._log.warning(
            f"Batch cancel orders not yet implemented for {len(command.cancels)} orders",
        )

    # -- REPORTS ------------------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        """
        Generate an order status report.

        Parameters
        ----------
        command : GenerateOrderStatusReport
            The command to generate the report.

        Returns
        -------
        OrderStatusReport | None

        """
        self._log.warning(
            f"Order status report generation not yet implemented for {command.client_order_id}",
        )
        return None

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        """
        Generate order status reports.

        Parameters
        ----------
        command : GenerateOrderStatusReports
            The command to generate the reports.

        Returns
        -------
        list[OrderStatusReport]

        """
        try:
            instrument_id = command.instrument_id.value if command.instrument_id else None
            reports = await self._client.request_order_status_reports(instrument_id=instrument_id)

            self._log.info(f"Generated {len(reports)} order status report(s)", LogColor.GREEN)
            return reports
        except Exception as e:
            self._log.error(f"Failed to generate order status reports: {e}")
            return []

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        """
        Generate fill reports.

        Parameters
        ----------
        command : GenerateFillReports
            The command to generate the reports.

        Returns
        -------
        list[FillReport]

        """
        try:
            instrument_id = command.instrument_id.value if command.instrument_id else None
            reports = await self._client.request_fill_reports(instrument_id=instrument_id)

            self._log.info(f"Generated {len(reports)} fill report(s)", LogColor.GREEN)
            return reports
        except Exception as e:
            self._log.error(f"Failed to generate fill reports: {e}")
            return []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """
        Generate position status reports.

        Parameters
        ----------
        command : GeneratePositionStatusReports
            The command to generate the reports.

        Returns
        -------
        list[PositionStatusReport]

        """
        try:
            instrument_id = command.instrument_id.value if command.instrument_id else None
            reports = await self._client.request_position_status_reports(
                instrument_id=instrument_id,
            )

            self._log.info(f"Generated {len(reports)} position status report(s)", LogColor.GREEN)
            return reports
        except Exception as e:
            self._log.error(f"Failed to generate position status reports: {e}")
            return []

    # -- QUERIES ------------------------------------------------------------------------------------

    async def _query_order(self, command: QueryOrder) -> None:
        """
        Query order status.

        Parameters
        ----------
        command : QueryOrder
            The command to query the order.

        """
        self._log.warning(f"Order query not yet implemented for {command.client_order_id}")

    async def _query_account(self, command: QueryAccount) -> None:
        """
        Query account information.

        Parameters
        ----------
        command : QueryAccount
            The command to query the account.

        """
        self._log.warning("Account query not yet implemented")
