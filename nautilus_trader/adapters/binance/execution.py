# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import datetime
from typing import List

from nautilus_trader.adapters.binance.common import BINANCE_VENUE
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.execution.messages import ExecutionReport
from nautilus_trader.execution.messages import OrderStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.c_enums.account_type import AccountType
from nautilus_trader.model.c_enums.venue_type import VenueType
from nautilus_trader.model.commands.trading import CancelOrder
from nautilus_trader.model.commands.trading import ModifyOrder
from nautilus_trader.model.commands.trading import SubmitOrder
from nautilus_trader.model.commands.trading import SubmitOrderList
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.orders.base import Order
from nautilus_trader.msgbus.bus import MessageBus


class BinanceSpotExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for Binance SPOT markets.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: BinanceHttpClient,
        account_id: AccountId,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: BinanceInstrumentProvider,
    ):
        """
        Initialize a new instance of the ``BinanceSpotExecutionClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        client : BinanceHttpClient
            The binance HTTP client.
        account_id : AccountId
            The account ID for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.
        instrument_provider : BinanceInstrumentProvider
            The instrument provider.

        """
        super().__init__(
            loop=loop,
            client_id=ClientId(BINANCE_VENUE.value),
            venue_type=VenueType.EXCHANGE,
            account_id=account_id,
            account_type=AccountType.CASH,
            base_currency=None,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config={"name": "BinanceExecClient"},
        )

        self._client = client
        self._instrument_provider = instrument_provider

    def connect(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def disconnect(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def reset(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def dispose(self) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- COMMAND HANDLERS --------------------------------------------------------------------------

    def submit_order(self, command: SubmitOrder) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def submit_order_list(self, command: SubmitOrderList) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def modify_order(self, command: ModifyOrder) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    def cancel_order(self, command: CancelOrder) -> None:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    # -- RECONCILIATION ----------------------------------------------------------------------------

    async def generate_order_status_report(self, order: Order) -> OrderStatusReport:
        """
        Generate an order status report for the given order.

        If an error occurs then logs and returns ``None``.

        Parameters
        ----------
        order : Order
            The order for the report.

        Returns
        -------
        OrderStatusReport or ``None``

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def generate_exec_reports(
        self,
        venue_order_id: VenueOrderId,
        symbol: Symbol,
        since: datetime = None,
    ) -> List[ExecutionReport]:
        """
        Generate a list of execution reports.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        venue_order_id : VenueOrderId
            The venue order ID for the trades.
        symbol : Symbol
            The symbol for the trades.
        since : datetime, optional
            The timestamp to filter trades on.

        Returns
        -------
        list[ExecutionReport]

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover
