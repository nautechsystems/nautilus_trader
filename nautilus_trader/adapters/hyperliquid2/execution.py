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
Live execution client for Hyperliquid.
"""

from nautilus_hyperliquid2 import Hyperliquid2HttpClient
from nautilus_hyperliquid2 import Hyperliquid2WebSocketClient

from nautilus_trader.adapters.hyperliquid2.providers import Hyperliquid2InstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId


class Hyperliquid2LiveExecClient(LiveExecutionClient):
    """
    Live execution client for the Hyperliquid exchange.

    Parameters
    ----------
    http_client : Hyperliquid2HttpClient
        The Hyperliquid HTTP client.
    ws_client : Hyperliquid2WebSocketClient
        The Hyperliquid WebSocket client.
    instrument_provider : Hyperliquid2InstrumentProvider
        The instrument provider.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    account_id : AccountId, optional
        The account ID for the client.

    """

    def __init__(
        self,
        http_client: Hyperliquid2HttpClient,
        ws_client: Hyperliquid2WebSocketClient,
        instrument_provider: Hyperliquid2InstrumentProvider,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        account_id: AccountId | None = None,
    ) -> None:
        super().__init__(
            client_id=ClientId("HYPERLIQUID"),
            venue=instrument_provider.venue,
            account_id=account_id or AccountId("HYPERLIQUID-001"),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._http_client = http_client
        self._ws_client = ws_client
        self._instrument_provider = instrument_provider

    async def _connect(self) -> None:
        """Connect to Hyperliquid."""
        self._log.info("Connecting to Hyperliquid")

        # Load instruments
        await self._instrument_provider.load_all_async()

        # Connect WebSocket
        await self._ws_client.connect()

        self._log.info("Connected to Hyperliquid")

    async def _disconnect(self) -> None:
        """Disconnect from Hyperliquid."""
        self._log.info("Disconnecting from Hyperliquid")
        # WebSocket disconnect logic would go here
        self._log.info("Disconnected from Hyperliquid")

    # Account methods

    async def generate_account_report(self, **kwargs) -> None:
        """Generate account report."""
        self._log.warning("Account report generation not yet implemented")

    async def generate_order_status_report(self, **kwargs) -> None:
        """Generate order status report."""
        self._log.warning("Order status report generation not yet implemented")

    async def generate_fill_reports(self, **kwargs) -> None:
        """Generate fill reports."""
        self._log.warning("Fill report generation not yet implemented")

    async def generate_position_status_reports(self, **kwargs) -> None:
        """Generate position status reports."""
        self._log.warning("Position status report generation not yet implemented")

    # Order submission methods (stubs for now)

    async def _submit_order(self, command: SubmitOrder) -> None:
        """Submit an order."""
        PyCondition.not_none(command, "command")

        self._log.warning(
            f"Order submission not yet implemented: {command.order.client_order_id}"
        )

        # In a full implementation, this would:
        # 1. Convert Nautilus order to Hyperliquid format
        # 2. Submit order via HTTP client
        # 3. Handle response and generate appropriate events
        # 4. Subscribe to order updates via WebSocket

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        """Submit a list of orders."""
        PyCondition.not_none(command, "command")

        self._log.warning(
            f"Order list submission not yet implemented: {command.order_list.id}"
        )

    async def _modify_order(self, command: ModifyOrder) -> None:
        """Modify an order."""
        PyCondition.not_none(command, "command")

        self._log.warning(
            f"Order modification not yet implemented: {command.client_order_id}"
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        """Cancel an order."""
        PyCondition.not_none(command, "command")

        self._log.warning(
            f"Order cancellation not yet implemented: {command.client_order_id}"
        )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        """Cancel all orders."""
        PyCondition.not_none(command, "command")

        instrument_id = command.instrument_id
        if instrument_id:
            self._log.warning(
                f"Cancel all orders not yet implemented for {instrument_id}"
            )
        else:
            self._log.warning("Cancel all orders not yet implemented")

    async def _query_order(self, command: QueryOrder) -> None:
        """Query an order."""
        PyCondition.not_none(command, "command")

        self._log.warning(
            f"Order query not yet implemented: {command.client_order_id}"
        )
