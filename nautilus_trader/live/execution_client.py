# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
which may be presented directly by an exchange, or broker intermediary.
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
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
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
    venue : Venue, optional with no default so ``None`` must be passed explicitly
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
        success: str | None = None,
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
        success : str, optional
            The log message to write on actions success.

        Returns
        -------
        asyncio.Task

        """
        log_msg = log_msg or coro.__name__
        self._log.debug(f"Creating task {log_msg}.")
        task = self._loop.create_task(
            coro,
            name=coro.__name__,
        )
        task.add_done_callback(
            functools.partial(
                self._on_task_completed,
                actions,
                success,
            ),
        )
        return task

    def _on_task_completed(
        self,
        actions: Callable | None,
        success: str | None,
        task: Task,
    ) -> None:
        e: BaseException | None = task.exception()
        if e:
            tb_str = "".join(traceback.format_exception(type(e), e, e.__traceback__))
            self._log.error(
                f"Error on `{task.get_name()}`: " f"{task.exception()!r}\n{tb_str}",
            )
        else:
            if actions:
                try:
                    actions()
                except Exception as e:
                    tb_str = "".join(traceback.format_exception(type(e), e, e.__traceback__))
                    self._log.error(
                        f"Failed triggering action {actions.__name__} on `{task.get_name()}`: "
                        f"{e!r}\n{tb_str}",
                    )
            if success:
                self._log.info(success, LogColor.GREEN)

    def connect(self) -> None:
        """
        Connect the client.
        """
        self._log.info("Connecting...")
        self.create_task(
            self._connect(),
            actions=lambda: self._set_connected(True),
            success="Connected",
        )

    def disconnect(self) -> None:
        """
        Disconnect the client.
        """
        self._log.info("Disconnecting...")
        self.create_task(
            self._disconnect(),
            actions=lambda: self._set_connected(False),
            success="Disconnected",
        )

    def submit_order(self, command: SubmitOrder) -> None:
        self.create_task(
            self._submit_order(command),
            log_msg=f"submit_order: {command}",
        )

    def submit_order_list(self, command: SubmitOrderList) -> None:
        self.create_task(
            self._submit_order_list(command),
            log_msg=f"submit_order_list: {command}",
        )

    def modify_order(self, command: ModifyOrder) -> None:
        self.create_task(
            self._modify_order(command),
            log_msg=f"modify_order: {command}",
        )

    def cancel_order(self, command: CancelOrder) -> None:
        self.create_task(
            self._cancel_order(command),
            log_msg=f"cancel_order: {command}",
        )

    def cancel_all_orders(self, command: CancelAllOrders) -> None:
        self.create_task(
            self._cancel_all_orders(command),
            log_msg=f"cancel_all_orders: {command}",
        )

    def batch_cancel_orders(self, command: BatchCancelOrders) -> None:
        self.create_task(
            self._batch_cancel_orders(command),
            log_msg=f"batch_cancel_orders: {command}",
        )

    def query_order(self, command: QueryOrder) -> None:
        self.create_task(
            self._query_order(command),
            log_msg=f"query_order: {command}",
        )

    async def generate_order_status_report(
        self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId | None = None,
        venue_order_id: VenueOrderId | None = None,
    ) -> OrderStatusReport | None:
        """
        Generate an `OrderStatusReport` for the given order identifier parameter(s).

        If the order is not found, or an error occurs, then logs and returns ``None``.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the report.
        client_order_id : ClientOrderId, optional
            The client order ID for the report.
        venue_order_id : VenueOrderId, optional
            The venue order ID for the report.

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
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
        open_only: bool = False,
    ) -> list[OrderStatusReport]:
        """
        Generate a list of `OrderStatusReport`s with optional query filters.

        The returned list may be empty if no orders match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        start : pd.Timestamp, optional
            The start datetime (UTC) query filter.
        end : pd.Timestamp, optional
            The end datetime (UTC) query filter.
        open_only : bool, default False
            If the query is for open orders only.

        Returns
        -------
        list[OrderStatusReport]

        """
        raise NotImplementedError(
            "method `generate_order_status_reports` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_fill_reports(
        self,
        instrument_id: InstrumentId | None = None,
        venue_order_id: VenueOrderId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[FillReport]:
        """
        Generate a list of `FillReport`s with optional query filters.

        The returned list may be empty if no trades match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        venue_order_id : VenueOrderId, optional
            The venue order ID (assigned by the venue) query filter.
        start : pd.Timestamp, optional
            The start datetime (UTC) query filter.
        end : pd.Timestamp, optional
            The end datetime (UTC) query filter.

        Returns
        -------
        list[FillReport]

        """
        raise NotImplementedError(
            "method `generate_fill_reports` must be implemented in the subclass",
        )  # pragma: no cover

    async def generate_position_status_reports(
        self,
        instrument_id: InstrumentId | None = None,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[PositionStatusReport]:
        """
        Generate a list of `PositionStatusReport`s with optional query filters.

        The returned list may be empty if no positions match the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId, optional
            The instrument ID query filter.
        start : pd.Timestamp, optional
            The start datetime (UTC) query filter.
        end : pd.Timestamp, optional
            The end datetime (UTC) query filter.

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

        try:
            reports = await asyncio.gather(
                self.generate_order_status_reports(start=since),
                self.generate_fill_reports(start=since),
                self.generate_position_status_reports(start=since),
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
        self._log.debug(f"Synchronizing order status {command}.")

        report: OrderStatusReport | None = await self.generate_order_status_report(
            instrument_id=command.instrument_id,
            client_order_id=command.client_order_id,
            venue_order_id=command.venue_order_id,
        )

        if report is None:
            self._log.warning("Did not receive `OrderStatusReport` from request.")
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
