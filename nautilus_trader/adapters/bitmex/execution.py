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
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import BatchCancelOrders
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId


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
        self._base_url_ws = config.base_url_ws
        self._http_client = client

        # WebSocket API
        ws_url = self._determine_ws_url(config)
        self._ws_client = nautilus_pyo3.BitmexWebSocketClient(
            url=ws_url,
            api_key=config.api_key,
            api_secret=config.api_secret,
            heartbeat=30,
        )
        self._ws_client_futures: set[asyncio.Future] = set()
        self._log.info(f"WebSocket URL {ws_url}", LogColor.BLUE)

        self._symbol_status = config.symbol_status

        # Hot caches
        self._venue_order_ids: dict[ClientOrderId, VenueOrderId] = {}
        self._client_order_ids: dict[VenueOrderId, ClientOrderId] = {}

    def _log_runtime_error(self, message: str) -> None:
        self._log.error(message, LogColor.RED)
        raise RuntimeError(message)

    @property
    def instrument_provider(self) -> BitmexInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self) -> None:
        pass  # TODO: Implement

    async def _disconnect(self) -> None:
        # Shutdown websocket
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting websocket")

            await self._ws_client.close()

            self._log.info(
                f"Disconnected from {self._ws_client.url}",
                LogColor.BLUE,
            )

        # Cancel any pending futures
        for future in self._ws_client_futures:
            if not future.done():
                future.cancel()

        if self._ws_client_futures:
            try:
                await asyncio.wait_for(
                    asyncio.gather(*self._ws_client_futures, return_exceptions=True),
                    timeout=2.0,
                )
            except TimeoutError:
                self._log.warning("Timeout while waiting for websockets shutdown to complete")

        self._ws_client_futures.clear()

    async def _submit_order(self, command: SubmitOrder) -> None:
        self._log.warning("Order submission not yet implemented")

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        self._log.warning("Order list submission not yet implemented")

    async def _modify_order(self, command: ModifyOrder) -> None:
        self._log.warning("Order modification not yet implemented")

    async def _cancel_order(self, command: CancelOrder) -> None:
        self._log.warning("Order cancellation not yet implemented")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        self._log.warning("Cancel all orders not yet implemented")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        self._log.warning("Batch cancel orders not yet implemented")

    async def _query_order(self, command: QueryOrder) -> None:
        self._log.warning("Query order not yet implemented")

    def _handle_order_status_report(self, report: Any) -> None:
        """
        Handle an order status report from the exchange.
        """
        # TODO: Implement

    def _handle_trade_report(self, report: Any) -> None:
        """
        Handle a trade report from the exchange.
        """
        # TODO: Implement

    def _handle_position_report(self, report: Any) -> None:
        """
        Handle a position report from the exchange.
        """
        # TODO: Implement
