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
from typing import TYPE_CHECKING, Any

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
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId


if TYPE_CHECKING:
    pass  # TODO: Add imports when HyperliquidHttpClient is available


class HyperliquidExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Hyperliquid decentralized exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : Any
        The Hyperliquid HTTP client (to be implemented).
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
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or HYPERLIQUID_VENUE.value),
            venue=HYPERLIQUID_VENUE,
            oms_type=OmsType.NETTING,  # Set to NETTING like BitMEX
            account_type=AccountType.MARGIN,  # Set to MARGIN like BitMEX
            base_currency=None,  # TODO: Set base currency when available
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
        )

        # Configuration
        self._config = config
        self._client = client

        # Log configuration details
        self._log.info(f"config.testnet={config.testnet}", LogColor.BLUE)
        self._log.info(f"config.http_timeout_secs={config.http_timeout_secs}", LogColor.BLUE)

        # Set initial account ID
        account_id = AccountId(f"{HYPERLIQUID_VENUE.value}-001")  # Placeholder account ID
        self._set_account_id(account_id)

        # Placeholder for WebSocket connections
        self._ws_connection = None

        self._log.info("Hyperliquid execution client initialized")

    # -- COMMANDS -----------------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        """
        Submit an order.

        Parameters
        ----------
        command : SubmitOrder
            The command to submit the order.

        """
        self._log.warning(f"Order submission not yet implemented for {command.order}")

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        """
        Submit an order list.

        Parameters
        ----------
        command : SubmitOrderList
            The command to submit the order list.

        """
        self._log.warning(f"Order list submission not yet implemented for {command.order_list}")

    async def _modify_order(self, command: ModifyOrder) -> None:
        """
        Modify an order.

        Parameters
        ----------
        command : ModifyOrder
            The command to modify the order.

        """
        self._log.warning(f"Order modification not yet implemented for {command.order}")

    # -- REPORTS ------------------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        """
        Generate an `OrderStatusReport` for the given order.

        Parameters
        ----------
        command : GenerateOrderStatusReport
            The command to generate the order status report.

        Returns
        -------
        OrderStatusReport or None

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
        Generate a list of `OrderStatusReport`s with optional query filters.

        Parameters
        ----------
        command : GenerateOrderStatusReports
            The command to generate the order status reports.

        Returns
        -------
        list[OrderStatusReport]

        """
        self._log.warning("Order status reports generation not yet implemented")
        return []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """
        Generate a list of `PositionStatusReport`s with optional query filters.

        Parameters
        ----------
        command : GeneratePositionStatusReports
            The command to generate the position status reports.

        Returns
        -------
        list[PositionStatusReport]

        """
        self._log.warning("Position status reports generation not yet implemented")
        return []

    # -- QUERIES ------------------------------------------------------------------------------------

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
        Cancel all orders.

        Parameters
        ----------
        command : CancelAllOrders
            The command to cancel all orders.

        """
        instrument_id_str = command.instrument_id or "all instruments"
        self._log.warning(f"Cancel all orders not yet implemented for {instrument_id_str}")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        """
        Batch cancel orders.

        Parameters
        ----------
        command : BatchCancelOrders
            The command to batch cancel orders.

        """
        self._log.warning(
            f"Batch cancel orders not yet implemented for {len(command.cancel_list)} orders",
        )

    # -- QUERIES ------------------------------------------------------------------------------------

    async def _query_order(self, command: QueryOrder) -> None:
        """
        Query an order.

        Parameters
        ----------
        command : QueryOrder
            The command to query the order.

        """
        self._log.warning(f"Order query not yet implemented for {command.client_order_id}")

    # -- TASKS --------------------------------------------------------------------------------------

    async def _connect(self) -> None:
        """
        Connect the client.
        """
        self._log.info("Connecting to Hyperliquid execution...", LogColor.BLUE)

        # TODO: Implement actual connection logic when HyperliquidHttpClient is available
        self._log.warning("Hyperliquid execution connection logic not yet implemented")

        # Simulate connection for now
        await asyncio.sleep(0.1)
        self._log.info("Hyperliquid execution connection established (placeholder)", LogColor.GREEN)

    async def _disconnect(self) -> None:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting from Hyperliquid execution...", LogColor.BLUE)

        # TODO: Implement actual disconnection logic
        if self._ws_connection:
            # Close WebSocket connections
            pass

        await asyncio.sleep(0.1)
        self._log.info("Hyperliquid execution disconnection completed", LogColor.GREEN)
