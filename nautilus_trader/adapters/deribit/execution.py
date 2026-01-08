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

from nautilus_trader.adapters.deribit.config import DeribitExecClientConfig
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
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId


class DeribitExecutionClient(LiveExecutionClient):
    """
    Provides an execution client for the Deribit cryptocurrency exchange.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : nautilus_pyo3.DeribitHttpClient
        The Deribit HTTP client.
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
        client: nautilus_pyo3.DeribitHttpClient,
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
        self._http_client = client
        if config.api_key:
            masked_key = mask_api_key(config.api_key)
            self._log.info(f"REST API key {masked_key}", LogColor.BLUE)

    async def _connect(self) -> None:
        self._log.info("Connecting...")
        await self._instrument_provider.initialize()

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
        self._log.info("Disconnected", LogColor.GREEN)

    def _handle_account_state(self, msg: nautilus_pyo3.AccountState) -> None:
        account_state = AccountState.from_dict(msg.to_dict())
        self.generate_account_state(
            balances=account_state.balances,
            margins=account_state.margins,
            reported=account_state.is_reported,
            ts_event=account_state.ts_event,
        )

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
        self._log.warning(
            f"submit_order not yet implemented (client_order_id={command.order.client_order_id})",
        )

    async def _submit_order_list(self, command: SubmitOrder) -> None:
        self._log.warning("submit_order_list not yet implemented")

    async def _modify_order(self, command: ModifyOrder) -> None:
        self._log.warning(
            f"modify_order not yet implemented (client_order_id={command.client_order_id})",
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        self._log.warning(
            f"cancel_order not yet implemented (client_order_id={command.client_order_id})",
        )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        self._log.warning("cancel_all_orders not yet implemented")

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        self._log.warning("batch_cancel_orders not yet implemented")

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
        self._log.warning(
            f"generate_order_status_reports not yet implemented (instrument_id={command.instrument_id})",
        )
        return []

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        self._log.warning(
            f"generate_fill_reports not yet implemented (instrument_id={command.instrument_id})",
        )
        return []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        self._log.warning(
            f"generate_position_status_reports not yet implemented (instrument_id={command.instrument_id})",
        )
        return []
