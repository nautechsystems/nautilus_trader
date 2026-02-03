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
"""
AX Exchange execution client implementation.

This module provides a LiveExecutionClient that interfaces with Architect's REST and
WebSocket APIs for order management and execution. The client uses Rust-based HTTP and
WebSocket clients exposed via PyO3 for performance.

"""

import asyncio

from nautilus_trader.adapters.architect_ax.config import AxExecClientConfig
from nautilus_trader.adapters.architect_ax.constants import AX_VENUE
from nautilus_trader.adapters.architect_ax.providers import AxInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.execution.messages import CancelAllOrders
from nautilus_trader.execution.messages import CancelOrder
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryAccount
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


class AxExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the AX Exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.AxHttpClient
        The AX Exchange HTTP client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : AxInstrumentProvider
        The instrument provider.
    config : AxExecClientConfig
        The configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: nautilus_pyo3.AxHttpClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: AxInstrumentProvider,
        config: AxExecClientConfig,
        name: str | None,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or AX_VENUE.value),
            venue=AX_VENUE,
            oms_type=OmsType.NETTING,
            instrument_provider=instrument_provider,
            account_type=AccountType.MARGIN,
            base_currency=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self._instrument_provider: AxInstrumentProvider = instrument_provider
        self._config = config

        account_id = AccountId(f"{name or AX_VENUE.value}-001")
        self._set_account_id(account_id)
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        self._log.info(f"{config.environment=}", LogColor.BLUE)
        self._log.info(f"{config.http_proxy_url=}", LogColor.BLUE)

        self._http_client = client

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()

        try:
            await self._http_client.authenticate_auto()
            self._log.info("Authenticated with AX Exchange", LogColor.BLUE)
        except ValueError as e:
            err_str = str(e)
            if "Missing credentials" in err_str or "MissingCredentials" in err_str:
                self._log.warning("No API credentials configured, execution features unavailable")
            else:
                raise

        self._log.info("Connected to AX Exchange execution API", LogColor.BLUE)

    def _cache_instruments(self) -> None:
        for inst in self._instrument_provider.instruments_pyo3():
            self._http_client.cache_instrument(inst)

    async def _disconnect(self) -> None:
        self._http_client.cancel_all_requests()
        self._log.info("Disconnected from AX Exchange execution API", LogColor.BLUE)

    async def _submit_order(self, command: SubmitOrder) -> None:
        self._log.warning("Order submission not yet implemented for AX Exchange")

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        self._log.warning("Order list submission not yet implemented for AX Exchange")

    async def _modify_order(self, command: ModifyOrder) -> None:
        self._log.warning("Order modification not yet implemented for AX Exchange")

    async def _cancel_order(self, command: CancelOrder) -> None:
        self._log.warning("Order cancellation not yet implemented for AX Exchange")

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        self._log.warning("Cancel all orders not yet implemented for AX Exchange")

    async def _query_account(self, command: QueryAccount) -> None:
        self._log.warning("Account query not yet implemented for AX Exchange")

    async def generate_order_status_report(
        self,
        instrument_id,
        client_order_id=None,
        venue_order_id=None,
    ) -> OrderStatusReport | None:
        self._log.warning("Order status report generation not yet implemented for AX Exchange")
        return None

    async def generate_order_status_reports(
        self,
        instrument_id=None,
        start=None,
        end=None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        reports = await self._http_client.request_order_status_reports(self.pyo3_account_id)
        return [OrderStatusReport.from_pyo3(r) for r in reports]

    async def generate_fill_reports(
        self,
        instrument_id=None,
        venue_order_id=None,
        start=None,
        end=None,
    ) -> list[FillReport]:
        reports = await self._http_client.request_fill_reports(self.pyo3_account_id)
        return [FillReport.from_pyo3(r) for r in reports]

    async def generate_position_status_reports(
        self,
        instrument_id=None,
        start=None,
        end=None,
    ) -> list[PositionStatusReport]:
        reports = await self._http_client.request_position_reports(self.pyo3_account_id)
        return [PositionStatusReport.from_pyo3(r) for r in reports]
