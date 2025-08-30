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
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.messages import ModifyOrder
from nautilus_trader.execution.messages import QueryOrder
from nautilus_trader.execution.messages import SubmitOrder
from nautilus_trader.execution.messages import SubmitOrderList
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.cancellation import DEFAULT_FUTURE_CANCELLATION_TIMEOUT
from nautilus_trader.live.cancellation import cancel_tasks_with_timeout
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
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
        self._symbol_status = config.symbol_status
        self._log.info(f"config.symbol_status={config.symbol_status}", LogColor.BLUE)
        self._log.info(f"config.testnet={config.testnet}", LogColor.BLUE)
        self._log.info(f"config.http_timeout_secs={config.http_timeout_secs}", LogColor.BLUE)

        # Set initial account ID (will be updated with actual account number on connect)
        self._account_id_prefix = name or BITMEX_VENUE.value
        account_id = AccountId(f"{self._account_id_prefix}-master")  # Temporary, like OKX
        self._set_account_id(account_id)

        # Create pyo3 account ID for Rust HTTP client
        self.pyo3_account_id = nautilus_pyo3.AccountId(account_id.value)

        # HTTP API
        self._http_client = client
        self._log.info(f"REST API key {self._http_client.api_key}", LogColor.BLUE)

        # WebSocket API
        ws_url = self._determine_ws_url(config)

        self._ws_client = nautilus_pyo3.BitmexWebSocketClient(
            url=ws_url,  # TODO: Move this to Rust
            api_key=config.api_key,
            api_secret=config.api_secret,
            account_id=self.pyo3_account_id,
            heartbeat=30,
        )
        self._ws_client_futures: set[asyncio.Future] = set()
        self._log.info(f"WebSocket URL {ws_url}", LogColor.BLUE)

        # Hot caches
        self._venue_order_ids: dict[ClientOrderId, VenueOrderId] = {}
        self._client_order_ids: dict[VenueOrderId, ClientOrderId] = {}

    def _log_runtime_error(self, message: str) -> None:
        self._log.error(message, LogColor.RED)
        raise RuntimeError(message)

    @property
    def instrument_provider(self) -> BitmexInstrumentProvider:
        return self._instrument_provider  # type: ignore

    def _determine_ws_url(self, config: BitmexExecClientConfig) -> str:
        if config.base_url_ws:
            return config.base_url_ws
        elif config.testnet:
            return "wss://testnet.bitmex.com/realtime"
        else:
            return "wss://ws.bitmex.com/realtime"

    def _cache_instruments(self) -> None:
        # Ensures instrument definitions are available for correct
        # price and size precisions when parsing responses
        instruments_pyo3 = self._instrument_provider.instruments_pyo3()  # type: ignore

        for inst in instruments_pyo3:
            self._http_client.add_instrument(inst)

        self._log.debug("Cached instruments", LogColor.MAGENTA)

    async def _connect(self) -> None:
        await self._instrument_provider.initialize()
        self._cache_instruments()

        instruments = self._instrument_provider.instruments_pyo3()  # type: ignore

        await self._ws_client.connect(
            instruments,
            self._handle_msg,
        )

        # Wait for connection to be established
        await self._ws_client.wait_until_active(timeout_secs=10.0)
        self._log.info(f"Connected to WebSocket {self._ws_client.url}", LogColor.BLUE)

        # Update account state on connection
        await self._update_account_state()

    async def _update_account_state(self) -> None:
        try:
            # First get the margin data to extract the actual account number
            account_number = await self._http_client.http_get_margin("XBt")  # type: ignore[attr-defined]

            # Update account ID with actual account number from BitMEX
            if account_number:
                actual_account_id = AccountId(f"{self._account_id_prefix}-{account_number}")
                self._set_account_id(actual_account_id)
                self.pyo3_account_id = nautilus_pyo3.AccountId(actual_account_id.value)
                self._log.info(f"Updated account ID to {actual_account_id}", LogColor.BLUE)

            # Now request the account state with the correct account ID
            pyo3_account_state = await self._http_client.request_account_state(self.pyo3_account_id)
            account_state = AccountState.from_dict(pyo3_account_state.to_dict())

            self.generate_account_state(
                balances=account_state.balances,
                margins=[],  # TBD
                reported=True,
                ts_event=self._clock.timestamp_ns(),
            )
        except Exception as e:
            self._log.error(f"Failed to update account state: {e}")

    async def _disconnect(self) -> None:
        # Delay to allow websocket to send any unsubscribe messages
        await asyncio.sleep(1.0)
        # Shutdown websocket
        if not self._ws_client.is_closed():
            self._log.info("Disconnecting websocket")

            await self._ws_client.close()

            self._log.info(
                f"Disconnected from {self._ws_client.url}",
                LogColor.BLUE,
            )

        # Cancel any pending futures
        await cancel_tasks_with_timeout(
            self._ws_client_futures,
            self._log,
            timeout_secs=DEFAULT_FUTURE_CANCELLATION_TIMEOUT,
        )
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

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        """
        Generate a list of `OrderStatusReport`s with optional query filters.
        """
        try:
            # Get the symbol filter if an instrument_id is provided
            symbol = None
            if command.instrument_id:
                symbol = command.instrument_id.symbol.value

            # Fetch order reports from BitMEX
            reports = await self._http_client.get_order_reports(symbol)

            # Convert from pyo3 reports to Python reports
            result = []
            for report in reports:
                # Convert pyo3 report to Python OrderStatusReport
                result.append(OrderStatusReport.from_pyo3(report))

            self._log.info(f"Generated {len(result)} order status reports")
            return result
        except Exception as e:
            self._log.error(f"Failed to generate order status reports: {e}")
            return []

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        """
        Generate an `OrderStatusReport` for the specified order.
        """
        # TODO: Implement fetching specific order from BitMEX
        self._log.warning("Order status report generation not yet implemented")
        return None

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """
        Generate a list of `PositionStatusReport`s with optional query filters.
        """
        try:
            # Fetch position reports from BitMEX
            reports = await self._http_client.get_position_reports()

            # Convert from pyo3 reports to Python reports if needed
            result = []
            for report in reports:
                # Convert pyo3 report to Python PositionStatusReport
                result.append(PositionStatusReport.from_pyo3(report))

            self._log.info(f"Generated {len(result)} position status reports")
            return result
        except Exception as e:
            self._log.error(f"Failed to generate position status reports: {e}")
            return []

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        """
        Generate a list of `FillReport`s with optional query filters.
        """
        try:
            # Get the symbol filter if an instrument_id is provided
            symbol = None
            if command.instrument_id:
                symbol = command.instrument_id.symbol.value

            # Fetch fill reports from BitMEX
            reports = await self._http_client.get_fill_reports(symbol)

            # Convert from pyo3 reports to Python reports if needed
            result = []
            for report in reports:
                # Convert pyo3 report to Python FillReport
                result.append(FillReport.from_pyo3(report))

            self._log.info(f"Generated {len(result)} fill reports")
            return result
        except Exception as e:
            self._log.error(f"Failed to generate fill reports: {e}")
            return []

    def _handle_msg(self, msg: Any) -> None:
        try:
            if isinstance(msg, nautilus_pyo3.AccountState):
                account_state = AccountState.from_dict(msg.to_dict())

                self.generate_account_state(
                    balances=account_state.balances,
                    margins=account_state.margins,
                    reported=account_state.is_reported,
                    ts_event=account_state.ts_event,
                )
            else:
                # TODO: Implement other message handling for execution messages
                self._log.debug(f"Received message: {msg}")
        except Exception as e:
            self._log.exception("Error handling websocket message", e)

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
