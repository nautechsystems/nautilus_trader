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
Execution client for Asterdex adapters.
"""

import asyncio
from collections.abc import Callable

import pandas as pd

from nautilus_trader.adapters.asterdex2.config import AsterdexExecClientConfig
from nautilus_trader.adapters.asterdex2.http.client import AsterdexHttpClient
from nautilus_trader.adapters.asterdex2.providers import AsterdexInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
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
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId


class AsterdexLiveExecClient(LiveExecutionClient):
    """
    Provides an execution client for the Asterdex exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    http_client : AsterdexHttpClient
        The Asterdex HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : AsterdexInstrumentProvider
        The instrument provider for the client.
    config : AsterdexExecClientConfig
        The configuration for the client.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        http_client: AsterdexHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: AsterdexInstrumentProvider,
        config: AsterdexExecClientConfig,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(config.name or "ASTERDEX"),
            venue=Venue("ASTERDEX"),
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,  # Asterdex uses margin accounts
            base_currency=None,  # Multi-currency
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        # Configuration
        self._config = config
        self._http_client = http_client

        # Account ID
        self._account_id = AccountId(f"{self.venue}-001")

        self._log.info("Asterdex execution client initialized", LogColor.BLUE)

    async def _connect(self) -> None:
        """Connect the client."""
        self._log.info("Connecting to Asterdex execution...", LogColor.BLUE)

        # Load instruments
        await self._instrument_provider.load_all_async()

        # Generate account state
        self.generate_account_state(
            balances=[],
            margins=[],
            reported=True,
            ts_event=self._clock.timestamp_ns(),
        )

        self._log.info("Connected to Asterdex execution", LogColor.GREEN)

    async def _disconnect(self) -> None:
        """Disconnect the client."""
        self._log.info("Disconnecting from Asterdex execution...", LogColor.BLUE)
        self._log.info("Disconnected from Asterdex execution", LogColor.GREEN)

    # -- EXECUTION REPORTS ------------------------------------------------------------------------

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> OrderStatusReport | None:
        """
        Generate an order status report.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the order.
        client_order_id : ClientOrderId, optional
            The client order ID.
        venue_order_id : VenueOrderId, optional
            The venue order ID.

        Returns
        -------
        OrderStatusReport | None

        """
        self._log.warning("Order status reports not yet implemented")
        return None

    async def generate_order_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        """
        Generate order status reports.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID to filter by.
        start : pd.Timestamp, optional
            The start time to filter by.
        end : pd.Timestamp, optional
            The end time to filter by.
        open_only : bool, default False
            Whether to return only open orders.

        Returns
        -------
        list[OrderStatusReport]

        """
        self._log.warning("Order status reports not yet implemented")
        return []

    async def generate_fill_reports(
        self,
        instrument_id: InstrumentId | None = None,
        venue_order_id: VenueOrderId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[FillReport]:
        """
        Generate fill reports.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID to filter by.
        venue_order_id : VenueOrderId, optional
            The venue order ID to filter by.
        start : pd.Timestamp, optional
            The start time to filter by.
        end : pd.Timestamp, optional
            The end time to filter by.

        Returns
        -------
        list[FillReport]

        """
        self._log.warning("Fill reports not yet implemented")
        return []

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[PositionStatusReport]:
        """
        Generate position status reports.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID to filter by.
        start : pd.Timestamp, optional
            The start time to filter by.
        end : pd.Timestamp, optional
            The end time to filter by.

        Returns
        -------
        list[PositionStatusReport]

        """
        self._log.warning("Position status reports not yet implemented")
        return []

    # -- COMMAND HANDLERS -------------------------------------------------------------------------

    async def _submit_order(self, command: SubmitOrder) -> None:
        """Submit an order."""
        self._log.warning("Order submission not yet implemented")

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        """Submit an order list."""
        self._log.warning("Order list submission not yet implemented")

    async def _modify_order(self, command: ModifyOrder) -> None:
        """Modify an order."""
        self._log.warning("Order modification not yet implemented")

    async def _cancel_order(self, command: CancelOrder) -> None:
        """Cancel an order."""
        self._log.warning("Order cancellation not yet implemented")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        """Cancel all orders."""
        self._log.warning("Cancel all orders not yet implemented")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        """Batch cancel orders."""
        self._log.warning("Batch cancel orders not yet implemented")

    async def _query_order(self, command: QueryOrder) -> None:
        """Query an order."""
        self._log.warning("Order query not yet implemented")
