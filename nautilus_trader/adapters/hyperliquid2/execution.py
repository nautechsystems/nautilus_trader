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
Provides an execution client for Hyperliquid.

This module provides the `HyperliquidExecutionClient` class which connects to the
Hyperliquid HTTP and WebSocket APIs for order management and trade execution.
"""

import asyncio

from nautilus_trader.adapters.hyperliquid2.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid2.providers import HyperliquidInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId


class HyperliquidExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the `Hyperliquid` DEX.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.HyperliquidHttpClient
        The Hyperliquid HTTP client.
    ws_client : nautilus_pyo3.HyperliquidWebSocketClient
        The Hyperliquid WebSocket client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : HyperliquidInstrumentProvider
        The instrument provider.
    base_url_http : str, optional
        The base HTTP URL.
    base_url_ws : str, optional
        The base WebSocket URL.
    name : str, optional
        The custom client name.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.HyperliquidHttpClient,
        ws_client: nautilus_pyo3.HyperliquidWebSocketClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: HyperliquidInstrumentProvider,
        base_url_http: str | None = None,
        base_url_ws: str | None = None,
        name: str | None = None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or "HYPERLIQUID"),
            venue=HYPERLIQUID_VENUE,
            oms_type=None,
            instrument_provider=instrument_provider,
            account_type=None,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Clients
        self._http_client = client
        self._ws_client = ws_client

        # Configuration
        self._base_url_http = base_url_http
        self._base_url_ws = base_url_ws

        # Account
        self._account_id = AccountId("HYPERLIQUID-001")

    @property
    def hyperliquid_instrument_provider(self) -> HyperliquidInstrumentProvider:
        return self._instrument_provider

    async def _connect(self) -> None:
        # Load instruments
        self._log.info("Loading Hyperliquid instruments...")
        await self._instrument_provider.load_all_async()

        # Connect WebSocket for execution updates
        if self._ws_client:
            self._log.info("Connecting to Hyperliquid WebSocket for execution...")
            await self._ws_client.connect()
            
            # Subscribe to execution channels
            await self._ws_client.subscribe_order_updates()
            await self._ws_client.subscribe_user_events()
            
            self._log.info("Connected to Hyperliquid WebSocket for execution", LogColor.GREEN)

    async def _disconnect(self) -> None:
        # Disconnect WebSocket
        if self._ws_client:
            self._log.info("Disconnecting from Hyperliquid WebSocket...")
            await self._ws_client.disconnect()
            self._log.info("Disconnected from Hyperliquid WebSocket", LogColor.BLUE)

    # -- EXECUTION COMMANDS -----------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        self._log.info(f"Submitting order: {command.order}")
        self._log.error("Order submission not yet implemented")

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        self._log.info(f"Submitting order list: {len(command.order_list.orders)} orders")
        self._log.error("Order list submission not yet implemented")

    async def _modify_order(self, command: ModifyOrder) -> None:
        self._log.info(f"Modifying order: {command.client_order_id}")
        self._log.error("Order modification not yet implemented")

    async def _cancel_order(self, command: CancelOrder) -> None:
        self._log.info(f"Canceling order: {command.client_order_id}")
        self._log.error("Order cancellation not yet implemented")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        self._log.info("Canceling all orders")
        self._log.error("Cancel all orders not yet implemented")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        self._log.info(f"Batch canceling {len(command.cancels)} orders")
        self._log.error("Batch cancel orders not yet implemented")

    async def _query_order(self, command: QueryOrder) -> None:
        self._log.info(f"Querying order: {command.client_order_id}")
        self._log.error("Order query not yet implemented")
