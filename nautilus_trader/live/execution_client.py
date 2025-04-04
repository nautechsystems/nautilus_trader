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
The `LiveExecutionClient` class is responsible for interfacing with a particular API
which may be presented directly by a venue, or through a broker intermediary.
"""

import asyncio
import functools
import traceback
from asyncio import Task
from collections.abc import Callable
from collections.abc import Coroutine
from datetime import timedelta

import pandas as pd

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.client import ExecutionClient
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
from nautilus_trader.execution.reports import ExecutionMassStatus
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import order_side_to_str
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Currency


class LiveExecutionClient(ExecutionClient):
    """
    The base class for all live execution clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    venue : Venue or ``None``
        The client venue. If multi-venue then can be ``None``.
    instrument_provider : InstrumentProvider
        The instrument provider for the client.
    account_type : AccountType
        The account type for the client.
    base_currency : Currency, optional
        The account base currency for the client. Use ``None`` for multi-currency accounts.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    config : NautilusConfig, optional
        The configuration for the instance.

    Raises
    ------
    ValueError
        If `oms_type` is ``UNSPECIFIED`` (must be specified).

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client_id: ClientId,
        venue: Venue | None,
        oms_type: OmsType,
        account_type: AccountType,
        base_currency: Currency | None,
        instrument_provider: InstrumentProvider,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        config: NautilusConfig | None = None,
    ) -> None:
        PyCondition.type(instrument_provider, InstrumentProvider, "instrument_provider")

        super().__init__(
            client_id=client_id,
            venue=venue,
            oms_type=oms_type,
            account_type=account_type,
            base_currency=base_currency,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self._loop = loop
        self._instrument_provider = instrument_provider

        self.reconciliation_active = False

    async def run_after_delay(
        self,
        delay: float,
        coro: Coroutine,
    ) -> None:
        """
        Run the given coroutine after a delay.

        Parameters
        ----------
        delay : float
            The delay (seconds) before running the coroutine.
        coro : Coroutine
            The coroutine to run after the initial delay.

        """
        await asyncio.sleep(delay)
        return await coro

    def create_task(
        self,
        coro: Coroutine,
        log_msg: str | None = None,
        actions: Callable | None = None,
        success_msg: str | None = None,
        success_color: LogColor = LogColor.NORMAL,
    ) -> asyncio.Task:
        """
        Run the given coroutine with error handling and optional callback actions when
        done.

        Parameters
        ----------
        coro : Coroutine
            The coroutine to run.
        log_msg : str, optional
            The log message for the task.
        actions : Callable, optional
            The actions callback to run when the coroutine is done.
        success_msg : str, optional
            The log message to write on `actions` success.
        success_color : str, default ``NORMAL``
            The log message color for `actions` success.

        Returns
        -------
        asyncio.Task

        """
        log_msg = log_msg or coro.__name__
        self._log.debug(f"Creating task '{log_msg}'")
        task = self._loop.create_task(
            coro,
            name=coro.__name__,
        )
        task.add_done_callback(
            functools.partial(
                self._on_task_completed,
                actions,
                success_msg,
                success_color,
            ),
        )
        return task

    def _on_task_completed(
        self,
        actions: Callable | None,
        success_msg: str | None,
        success_color: LogColor,
        task: Task,
    ) -> None:
        e: BaseException | None = task.exception()
        if e:
            tb_str = "".join(traceback.format_exception(type(e), e, e.__traceback__))
            self._log.error(
                f"Error on '{task.get_name()}': {task.exception()!r}\n{tb_str}",
            )
        else:
            if actions:
                try:
                    actions()
                except Exception as e:
                    self._log.exception(
                        f"Failed triggering action {actions.__name__} on '{task.get_name()}'",
                        e,
                    )
            if success_msg:
                self._log.info(success_msg, success_color)

    def connect(self) -> None:
        """
        Connect the client.
        """
        self._log.info("Connecting...")
        self.create_task(
            self._connect(),
            actions=lambda: self._set_connected(True),
            success_msg="Connected",
            success_color=LogColor.GREEN,
        )

    def disconnect(self) -> None:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")
        self.create_task(
            self._disconnect(),
            actions=lambda: self._set_connected(False),
            success_msg="Disconnected",
            success_color=LogColor.GREEN,
        )

    def submit_order(self, command: SubmitOrder) -> None:
        self._log.info(f"Submit {command.order}", LogColor.BLUE)
        self.create_task(
            self._submit_order(command),
            log_msg=f"submit_order: {command}",
        )

    def submit_order_list(self, command: SubmitOrderList) -> None:
        self._log.info(f"Submit {command.order_list}", LogColor.BLUE)
        self.create_task(
            self._submit_order_list(command),
            log_msg=f"submit_order_list: {command}",
        )

    def modify_order(self, command: ModifyOrder) -> None:
        venue_order_id_str = (
            " " + repr(command.venue_order_id) if command.venue_order_id is not None else ""
        )
        self._log.info(f"Modify {command.client_order_id!r}{venue_order_id_str}", LogColor.BLUE)
        self.create_task(
            self._modify_order(command),
            log_msg=f"modify_order: {command}",
        )

    def cancel_order(self, command: CancelOrder) -> None:
        venue_order_id_str = (
            " " + repr(command.venue_order_id) if command.venue_order_id is not None else ""
        )
        self._log.info(f"Cancel {command.client_order_id!r}{venue_order_id_str}", LogColor.BLUE)
        self.create_task(
            self._cancel_order(command),
            log_msg=f"cancel_order: {command}",
        )

    def cancel_all_orders(self, command: CancelAllOrders) -> None:
        side_str = f" {order_side_to_str(command.order_side)} " if command.order_side else " "
        self._log.info(f"Cancel all{side_str}orders", LogColor.BLUE)
        self.create_task(
            self._cancel_all_orders(command),
            log_msg=f"cancel_all_orders: {command}",
        )

    def batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        self._log.info(
            f"Batch cancel orders {[repr(c.client_order_id) for c in command.cancels]}",
            LogColor.BLUE,
        )
        self.create_task(
            self._batch_cancel_orders(command),
            log_msg=f"batch_cancel_orders: {command}",
        )

    def query_order(self, command: QueryOrder) -> None:
        self._log.info(f"Query {command.client_order_id!r}", LogColor.BLUE)
        self.create_task(
            self._query_order(command),
            log_msg=f"query_order: {command}",
        )

    async def generate_order_status_report(
        self,
        command: GenerateOrderStatusReport,
    ) -> OrderStatusReport | None:
        """
        Generate an `OrderStatusReport` for the given order identifier parameter(s).

        If the order is not found, or an error occurs, then logs and returns ``None``.

        Parameters
        ----------
        command : GenerateOrderStatusReport
            The command to generate the report.

        Returns
        -------
        OrderStatusReport or ``None``

        Raises
        ------
        ValueError
            If both the `client_order_id` and `venue_order_id` are ``None``.

        """
        raise NotImplementedError(
            "method `generate_order_status_report` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_order_status_reports(
        self,
        command: GenerateOrderStatusReports,
    ) -> list[OrderStatusReport]:
        """
        Generate a list of `OrderStatusReport`s with optional query filters.

        The returned list may be empty if no orders match the given parameters.

        Parameters
        ----------
        command : GenerateOrderStatusReports
            The command for generating the reports.

        Returns
        -------
        list[OrderStatusReport]

        """
        raise NotImplementedError(
            "method `generate_order_status_reports` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_fill_reports(
        self,
        command: GenerateFillReports,
    ) -> list[FillReport]:
        """
        Generate a list of `FillReport`s with optional query filters.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        command : GenerateFillReports
            The command for generating the reports.

        Returns
        -------
        list[FillReport]

        """
        raise NotImplementedError(
            "method `generate_fill_reports` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_position_status_reports(
        self,
        command: GeneratePositionStatusReports,
    ) -> list[PositionStatusReport]:
        """
        Generate a list of `PositionStatusReport`s with optional query filters.

        The returned list may be empty if no positions match the given parameters.

        Parameters
        ----------
        command : GeneratePositionStatusReports
            The command for generating the position status reports.

        Returns
        -------
        list[PositionStatusReport]

        """
        raise NotImplementedError(
            "method `generate_position_status_reports` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_mass_status(
        self,
        lookback_mins: int | None = None,
    ) -> ExecutionMassStatus | None:
        """
        Generate an `ExecutionMassStatus` report.

        Parameters
        ----------
        lookback_mins : int, optional
            The maximum lookback for querying closed orders, trades and positions.

        Returns
        -------
        ExecutionMassStatus or ``None``

        """
        self._log.info("Generating ExecutionMassStatus...")

        self.reconciliation_active = True

        mass_status = ExecutionMassStatus(
            client_id=self.id,
            account_id=self.account_id,
            venue=self.venue,
            report_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        since: pd.Timestamp | None = None
        if lookback_mins is not None:
            since = self._clock.utc_now() - timedelta(minutes=lookback_mins)

        order_status_command = GenerateOrderStatusReports(
            instrument_id=None,
            start=since,
            end=None,
            open_only=False,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        fill_reports_command = GenerateFillReports(
            instrument_id=None,
            venue_order_id=None,
            start=since,
            end=None,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        position_status_command = GeneratePositionStatusReports(
            instrument_id=None,
            start=since,
            end=None,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )

        try:
            reports = await asyncio.gather(
                self.generate_order_status_reports(order_status_command),
                self.generate_fill_reports(fill_reports_command),
                self.generate_position_status_reports(position_status_command),
            )

            mass_status.add_order_reports(reports=reports[0])
            mass_status.add_fill_reports(reports=reports[1])
            mass_status.add_position_reports(reports=reports[2])

            self.reconciliation_active = False

            return mass_status
        except Exception as e:
            self._log.exception("Cannot reconcile execution state", e)
        return None

    async def _query_order(self, command: QueryOrder) -> None:
        self._log.debug(f"Synchronizing order status {command}")

        command = GenerateOrderStatusReport(
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
            command_id=UUID4(),
            ts_init=self._clock.timestamp_ns(),
        )
        report: OrderStatusReport | None = await self.generate_order_status_report(command)

        if report is None:
            self._log.warning("Did not receive `OrderStatusReport` from request")
            return

        self._send_order_status_report(report)

    ############################################################################
    # Coroutines to implement
    ############################################################################
    async def _connect(self) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_connect` coroutine",  # pragma: no cover
        )

    async def _disconnect(self) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_disconnect` coroutine",  # pragma: no cover
        )

    async def _submit_order(self, command: SubmitOrder) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_submit_order` coroutine",  # pragma: no cover
        )

    async def _submit_order_list(self, command: SubmitOrderList) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_submit_order_list` coroutine",  # pragma: no cover
        )

    async def _modify_order(self, command: ModifyOrder) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_modify_order` coroutine",  # pragma: no cover
        )

    async def _cancel_order(self, command: CancelOrder) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_cancel_order` coroutine",  # pragma: no cover
        )

    async def _cancel_all_orders(self, command: CancelAllOrders) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_cancel_all_orders` coroutine",  # pragma: no cover
        )

    async def _batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_batch_cancel_orders` coroutine",  # pragma: no cover
        )
