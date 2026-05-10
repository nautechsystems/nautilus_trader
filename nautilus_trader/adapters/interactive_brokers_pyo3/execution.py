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
Provides a PyO3-based execution client for Interactive Brokers.

This adapter uses PyO3 bindings to call the Rust implementation of the Interactive
Brokers adapter, providing the same API as the Python adapter but with Rust performance.

"""

from __future__ import annotations

import asyncio

from nautilus_trader.adapters.interactive_brokers_pyo3.config import (
    InteractiveBrokersExecClientConfig,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.transformers import transform_order_event_from_pyo3
from nautilus_trader.cache.transformers import transform_order_to_pyo3
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.messages import GeneratePositionStatusReports
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.live.execution_client import LiveExecutionClient
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue


try:
    from nautilus_trader.core.nautilus_pyo3.interactive_brokers import (
        InteractiveBrokersExecutionClient as RustInteractiveBrokersExecutionClient,
    )
except ImportError:
    RustInteractiveBrokersExecutionClient = None


class InteractiveBrokersExecutionClient(LiveExecutionClient):
    """
    Provides a PyO3-based execution client for Interactive Brokers.

    This class wraps the Rust implementation via PyO3 bindings, providing
    the same API as the Python adapter but using the Rust implementation.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : InteractiveBrokersInstrumentProvider
        The instrument provider.
    config : InteractiveBrokersExecClientConfig
        Configuration for the client.
    name : str, optional
        The custom client ID.

    Raises
    ------
    ImportError
        If the PyO3 bindings are not available.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider,  # InteractiveBrokersInstrumentProvider
        config: InteractiveBrokersExecClientConfig,
        name: str | None = None,
    ) -> None:
        if RustInteractiveBrokersExecutionClient is None:
            raise ImportError(
                "PyO3 bindings for Interactive Brokers are not available. "
                "Please ensure the extension module is built with the 'extension-module' feature.",
            )

        # Initialize the Rust client via PyO3
        self._rust_client = RustInteractiveBrokersExecutionClient(
            msgbus,
            cache,
            clock,
            instrument_provider._rust_provider,
            config,
        )
        self._ib_config = config

        account_id = getattr(config, "account_id", None)
        normalized_account_id = None
        client_id_value = name or self._rust_client.client_id.value

        if account_id:
            normalized_account_id = (
                account_id if account_id.startswith("IB-") else f"IB-{account_id}"
            )
            client_id_value = normalized_account_id.split("-", maxsplit=1)[0]

        # Initialize the Python base class
        super().__init__(
            loop=loop,
            client_id=ClientId(client_id_value),
            venue=None,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=None,  # IB accounts are multi-currency
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=None,
        )

        if normalized_account_id:
            self._set_account_id(AccountId(normalized_account_id))

        if hasattr(self._rust_client, "set_event_callback"):
            self._rust_client.set_event_callback(self._on_rust_event)

    async def _connect(self):
        """
        Connect the client.
        """
        self._rust_client.connect()
        self._instrument_provider._sync_from_rust()

    async def _disconnect(self):
        """
        Disconnect the client.
        """
        self._rust_client.disconnect()

    def _on_rust_event(self, kind: str, payload) -> None:
        self._loop.call_soon_threadsafe(self._handle_rust_event, kind, payload)

    def _handle_rust_event(self, kind: str, payload) -> None:
        if kind == "order_event":
            self._send_order_event(transform_order_event_from_pyo3(payload))
            return
        if kind == "order_report":
            self._send_order_status_report(OrderStatusReport.from_pyo3(payload))
            return
        if kind == "fill_report":
            self._send_fill_report(FillReport.from_pyo3(payload))
            return
        if kind == "position_report":
            self._send_position_status_report(PositionStatusReport.from_pyo3(payload))
            return
        if kind == "mass_status_report":
            self._send_mass_status_report(self._mass_status_from_pyo3(payload))
            return
        if kind == "account_state":
            if not payload.balances:
                return
            account_state = AccountState.from_dict(payload.to_dict())
            self.generate_account_state(
                balances=account_state.balances,
                margins=account_state.margins,
                reported=account_state.is_reported,
                ts_event=account_state.ts_event,
            )
            return

        self._log.warning(f"Unhandled IB PyO3 execution callback kind={kind}")

    # -- COMMAND HANDLERS ----------------------------------------------------------------------------

    async def _submit_order(self, command):
        """
        Submit a single order.

        Parameters
        ----------
        command : SubmitOrder
            The submit order command.

        """
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId
        from nautilus_trader.core.nautilus_pyo3 import StrategyId
        from nautilus_trader.core.nautilus_pyo3 import TraderId

        order = command.order
        if order.is_closed:
            self._log.warning(f"Order {order} is already closed")
            return

        pyo3_order = transform_order_to_pyo3(order)
        pyo3_instrument_id = InstrumentId.from_str(order.instrument_id.value)
        pyo3_strategy_id = StrategyId.from_str(command.strategy_id.value)
        pyo3_trader_id = TraderId.from_str(command.trader_id.value)

        # Extract optional fields
        from nautilus_trader.core.nautilus_pyo3 import ExecAlgorithmId
        from nautilus_trader.core.nautilus_pyo3 import PositionId

        pyo3_exec_algorithm_id = (
            ExecAlgorithmId.from_str(command.exec_algorithm_id.value)
            if command.exec_algorithm_id
            else None
        )
        pyo3_position_id = (
            PositionId.from_str(command.position_id.value) if command.position_id else None
        )
        # Convert params dict to Option[HashMap] - pass as dict, Rust will handle conversion
        pyo3_params = command.params

        self._rust_client.submit_order(
            pyo3_trader_id,
            pyo3_order,
            pyo3_instrument_id,
            pyo3_strategy_id,
            pyo3_exec_algorithm_id,
            pyo3_position_id,
            pyo3_params,
        )

    async def _submit_order_list(self, command):
        """
        Submit a list of orders (OCA group).

        Parameters
        ----------
        command : SubmitOrderList
            The submit order list command.

        """
        from nautilus_trader.core.nautilus_pyo3 import StrategyId
        from nautilus_trader.core.nautilus_pyo3 import TraderId

        # Extract fields from command
        pyo3_trader_id = TraderId.from_str(command.trader_id.value)
        pyo3_strategy_id = StrategyId.from_str(command.strategy_id.value)

        pyo3_orders = [transform_order_to_pyo3(order) for order in command.order_list.orders]

        # Extract optional fields
        from nautilus_trader.core.nautilus_pyo3 import ExecAlgorithmId
        from nautilus_trader.core.nautilus_pyo3 import PositionId

        pyo3_exec_algorithm_id = (
            ExecAlgorithmId.from_str(command.exec_algorithm_id.value)
            if command.exec_algorithm_id
            else None
        )
        pyo3_position_id = (
            PositionId.from_str(command.position_id.value) if command.position_id else None
        )
        # Convert params dict to Option[HashMap] - pass as dict, Rust will handle conversion
        pyo3_params = command.params

        self._rust_client.submit_order_list(
            pyo3_trader_id,
            pyo3_strategy_id,
            pyo3_orders,
            pyo3_exec_algorithm_id,
            pyo3_position_id,
            pyo3_params,
        )

    async def _modify_order(self, command):
        """
        Modify an existing order.

        Parameters
        ----------
        command : ModifyOrder
            The modify order command.

        """
        from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId
        from nautilus_trader.core.nautilus_pyo3 import Price
        from nautilus_trader.core.nautilus_pyo3 import Quantity
        from nautilus_trader.core.nautilus_pyo3 import StrategyId
        from nautilus_trader.core.nautilus_pyo3 import TraderId
        from nautilus_trader.core.nautilus_pyo3 import VenueOrderId

        # Extract fields from command
        pyo3_trader_id = TraderId.from_str(command.trader_id.value)
        pyo3_strategy_id = StrategyId.from_str(command.strategy_id.value)
        pyo3_client_order_id = ClientOrderId(command.client_order_id.value)
        pyo3_venue_order_id = (
            VenueOrderId(command.venue_order_id.value) if command.venue_order_id else None
        )
        pyo3_instrument_id = InstrumentId.from_str(command.instrument_id.value)
        pyo3_quantity = Quantity.from_str(str(command.quantity)) if command.quantity else None
        pyo3_price = Price.from_str(str(command.price)) if command.price else None
        pyo3_trigger_price = (
            Price.from_str(str(command.trigger_price)) if command.trigger_price else None
        )

        self._rust_client.modify_order(
            pyo3_trader_id,
            pyo3_strategy_id,
            pyo3_client_order_id,
            pyo3_venue_order_id,
            pyo3_instrument_id,
            pyo3_quantity,
            pyo3_price,
            pyo3_trigger_price,
        )

    async def _cancel_order(self, command):
        """
        Cancel a specific order.

        Parameters
        ----------
        command : CancelOrder
            The cancel order command.

        """
        from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId
        from nautilus_trader.core.nautilus_pyo3 import StrategyId
        from nautilus_trader.core.nautilus_pyo3 import TraderId
        from nautilus_trader.core.nautilus_pyo3 import VenueOrderId

        # Extract fields from command
        pyo3_trader_id = TraderId.from_str(command.trader_id.value)
        pyo3_strategy_id = StrategyId.from_str(command.strategy_id.value)
        pyo3_client_order_id = ClientOrderId(command.client_order_id.value)
        pyo3_venue_order_id = (
            VenueOrderId(command.venue_order_id.value) if command.venue_order_id else None
        )
        pyo3_instrument_id = InstrumentId.from_str(command.instrument_id.value)

        self._rust_client.cancel_order(
            pyo3_trader_id,
            pyo3_strategy_id,
            pyo3_client_order_id,
            pyo3_venue_order_id,
            pyo3_instrument_id,
        )

    async def _cancel_all_orders(self, command):
        """
        Cancel all orders for an instrument.

        Parameters
        ----------
        command : CancelAllOrders
            The cancel all orders command.

        """
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId
        from nautilus_trader.core.nautilus_pyo3 import OrderSide
        from nautilus_trader.core.nautilus_pyo3 import StrategyId
        from nautilus_trader.core.nautilus_pyo3 import TraderId

        # Extract fields from command
        pyo3_trader_id = TraderId.from_str(command.trader_id.value)
        pyo3_strategy_id = StrategyId.from_str(command.strategy_id.value)
        pyo3_instrument_id = InstrumentId.from_str(command.instrument_id.value)
        pyo3_order_side = OrderSide.from_str(command.order_side.name)

        self._rust_client.cancel_all_orders(
            pyo3_trader_id,
            pyo3_strategy_id,
            pyo3_instrument_id,
            pyo3_order_side,
            None,
        )

    async def _batch_cancel_orders(self, command):
        """
        Batch cancel multiple orders.

        Parameters
        ----------
        command : BatchCancelOrders
            The batch cancel orders command.

        """
        from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId
        from nautilus_trader.core.nautilus_pyo3 import StrategyId
        from nautilus_trader.core.nautilus_pyo3 import TraderId

        # Extract fields from command
        pyo3_trader_id = TraderId.from_str(command.trader_id.value)
        pyo3_strategy_id = StrategyId.from_str(command.strategy_id.value)
        pyo3_instrument_id = InstrumentId.from_str(command.instrument_id.value)

        # Extract cancel command details - we need to pass all cancel info
        # For now, extract client_order_ids and let Rust construct cancel commands
        # Ideally, we'd pass full cancel commands, but that requires more complex conversion
        pyo3_client_order_ids = [
            ClientOrderId(cancel_cmd.client_order_id.value) for cancel_cmd in command.cancels
        ]

        self._rust_client.batch_cancel_orders(
            pyo3_trader_id,
            pyo3_strategy_id,
            pyo3_instrument_id,
            pyo3_client_order_ids,
        )

    async def _query_account(self, command):
        """
        Query account state (forwards to Rust client).
        """
        from nautilus_trader.core.nautilus_pyo3 import TraderId

        pyo3_trader_id = TraderId.from_str(command.trader_id.value)
        self._rust_client.query_account(pyo3_trader_id)

    async def _query_order(self, command):
        """
        Query order status (forwards to Rust client).
        """
        from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId
        from nautilus_trader.core.nautilus_pyo3 import StrategyId
        from nautilus_trader.core.nautilus_pyo3 import TraderId
        from nautilus_trader.core.nautilus_pyo3 import VenueOrderId

        pyo3_trader_id = TraderId.from_str(command.trader_id.value)
        pyo3_strategy_id = StrategyId.from_str(command.strategy_id.value)
        pyo3_instrument_id = InstrumentId.from_str(command.instrument_id.value)
        pyo3_client_order_id = ClientOrderId(command.client_order_id.value)
        pyo3_venue_order_id = (
            VenueOrderId(command.venue_order_id.value) if command.venue_order_id else None
        )
        self._rust_client.query_order(
            pyo3_trader_id,
            pyo3_strategy_id,
            pyo3_instrument_id,
            pyo3_client_order_id,
            pyo3_venue_order_id,
        )

    async def generate_mass_status(
        self,
        lookback_mins: int | None = None,
    ) -> ExecutionMassStatus | None:
        """
        Generate an ExecutionMassStatus report (order, fill, position reports).

        Parameters
        ----------
        lookback_mins : int, optional
            The maximum lookback in minutes for querying closed orders, trades and positions.

        Returns
        -------
        ExecutionMassStatus or None

        """
        result = self._rust_client.generate_mass_status(lookback_mins)
        return self._mass_status_from_pyo3(result) if result is not None else None

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        """
        Generate a single order status report.

        Parameters
        ----------
        command : GenerateOrderStatusReport
            The command for generating the report.

        Returns
        -------
        OrderStatusReport | None
            The order status report if found, None otherwise.

        """
        from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId
        from nautilus_trader.core.nautilus_pyo3 import VenueOrderId

        # Extract fields from command
        pyo3_instrument_id = (
            InstrumentId.from_str(command.instrument_id.value) if command.instrument_id else None
        )
        pyo3_client_order_id = (
            ClientOrderId(command.client_order_id.value) if command.client_order_id else None
        )
        pyo3_venue_order_id = (
            VenueOrderId(command.venue_order_id.value) if command.venue_order_id else None
        )

        result = self._rust_client.generate_order_status_report(
            pyo3_instrument_id,
            pyo3_client_order_id,
            pyo3_venue_order_id,
        )

        return OrderStatusReport.from_pyo3(result) if result is not None else None

    def _mass_status_from_pyo3(self, pyo3_mass_status) -> ExecutionMassStatus:
        mass_status = ExecutionMassStatus(
            client_id=self.id,
            account_id=AccountId(pyo3_mass_status.account_id.value),
            venue=Venue(pyo3_mass_status.venue.value) if pyo3_mass_status.venue else None,
            report_id=UUID4.from_str(pyo3_mass_status.report_id.value),
            ts_init=pyo3_mass_status.ts_init,
        )

        if pyo3_mass_status.order_reports:
            order_reports = [
                OrderStatusReport.from_pyo3(report)
                for report in pyo3_mass_status.order_reports.values()
            ]
            mass_status.add_order_reports(order_reports)

        if pyo3_mass_status.fill_reports:
            fill_reports = [
                FillReport.from_pyo3(report)
                for reports in pyo3_mass_status.fill_reports.values()
                for report in reports
            ]
            mass_status.add_fill_reports(fill_reports)

        if pyo3_mass_status.position_reports:
            position_reports = [
                PositionStatusReport.from_pyo3(report)
                for reports in pyo3_mass_status.position_reports.values()
                for report in reports
            ]
            mass_status.add_position_reports(position_reports)

        return mass_status

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        """
        Generate multiple order status reports.

        Parameters
        ----------
        command : GenerateOrderStatusReports
            The command for generating the reports.

        Returns
        -------
        list[OrderStatusReport]
            List of order status reports.

        """
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId

        # Extract fields from command
        open_only = command.open_only
        pyo3_instrument_id = (
            InstrumentId.from_str(command.instrument_id.value) if command.instrument_id else None
        )
        start_ns = command.start.as_u64() if command.start is not None else None
        end_ns = command.end.as_u64() if command.end is not None else None

        result = self._rust_client.generate_order_status_reports(
            open_only,
            pyo3_instrument_id,
            start_ns,
            end_ns,
        )

        return [OrderStatusReport.from_pyo3(report) for report in result] if result else []

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        """
        Generate fill reports.

        Parameters
        ----------
        command : GenerateFillReports
            The command for generating the reports.

        Returns
        -------
        list[FillReport]
            List of fill reports.

        """
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId
        from nautilus_trader.core.nautilus_pyo3 import VenueOrderId

        # Extract fields from command
        pyo3_instrument_id = (
            InstrumentId.from_str(command.instrument_id.value) if command.instrument_id else None
        )
        pyo3_venue_order_id = (
            VenueOrderId(command.venue_order_id.value) if command.venue_order_id else None
        )
        start_ns = command.start.as_u64() if command.start is not None else None
        end_ns = command.end.as_u64() if command.end is not None else None

        result = self._rust_client.generate_fill_reports(
            pyo3_instrument_id,
            pyo3_venue_order_id,
            start_ns,
            end_ns,
        )

        return [FillReport.from_pyo3(report) for report in result] if result else []

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """
        Generate position status reports.

        Parameters
        ----------
        command : GeneratePositionStatusReports
            The command for generating the reports.

        Returns
        -------
        list[PositionStatusReport]
            List of position status reports.

        """
        from nautilus_trader.core.nautilus_pyo3 import InstrumentId

        # Extract fields from command
        pyo3_instrument_id = (
            InstrumentId.from_str(command.instrument_id.value) if command.instrument_id else None
        )
        start_ns = command.start.as_u64() if command.start is not None else None
        end_ns = command.end.as_u64() if command.end is not None else None

        result = self._rust_client.generate_position_status_reports(
            pyo3_instrument_id,
            start_ns,
            end_ns,
        )

        return [PositionStatusReport.from_pyo3(report) for report in result] if result else []
