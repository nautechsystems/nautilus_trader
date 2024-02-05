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
The `LiveDataClient` class is responsible for interfacing with a particular API which
may be presented directly by an exchange, or broker intermediary.

It could also be possible to write clients for specialized data providers.

"""

import asyncio
import functools
import traceback
from asyncio import Task
from collections.abc import Callable
from collections.abc import Coroutine
from typing import Any

import pandas as pd

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


class LiveDataClient(DataClient):
    """
    The base class for all live data clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The client venue. If multi-venue then can be ``None``.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    config : NautilusConfig, optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client_id: ClientId,
        venue: Venue | None,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        config: NautilusConfig | None = None,
    ) -> None:
        super().__init__(
            client_id=client_id,
            venue=venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self._loop = loop

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

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        self.create_task(
            self._subscribe(data_type),
            log_msg=f"subscribe: {data_type}",
            actions=lambda: self._add_subscription(data_type),
        )

    def unsubscribe(self, data_type: DataType) -> None:
        self.create_task(
            self._unsubscribe(data_type),
            log_msg=f"unsubscribe_{data_type}",
            actions=lambda: self._remove_subscription(data_type),
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, data_type: DataType, correlation_id: UUID4) -> None:
        self._log.debug(f"Request {data_type} {correlation_id}.")
        self.create_task(
            self._request(data_type, correlation_id),
            log_msg=f"request_{data_type}",
        )

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

    async def _subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe` coroutine",  # pragma: no cover
        )

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe` coroutine",  # pragma: no cover
        )

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request` coroutine",  # pragma: no cover
        )


class LiveMarketDataClient(MarketDataClient):
    """
    The base class for all live data clients.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client_id : ClientId
        The client ID.
    venue : Venue, optional with no default so ``None`` must be passed explicitly
        The client venue. If multi-venue then can be ``None``.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : InstrumentProvider
        The instrument provider for the client.
    config : NautilusConfig, optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client_id: ClientId,
        venue: Venue | None,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: InstrumentProvider,
        config: NautilusConfig | None = None,
    ) -> None:
        PyCondition.type(instrument_provider, InstrumentProvider, "instrument_provider")

        super().__init__(
            client_id=client_id,
            venue=venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self._loop = loop
        self._instrument_provider = instrument_provider

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
        if task.exception():
            self._log.error(
                f"Error on `{task.get_name()}`: " f"{task.exception()!r}",
            )
        else:
            if actions:
                try:
                    actions()
                except Exception as e:
                    self._log.error(
                        f"Failed triggering action {actions.__name__} on `{task.get_name()}`: "
                        f"{e!r}",
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

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        self.create_task(
            self._subscribe(data_type),
            log_msg=f"subscribe: {data_type}",
            actions=lambda: self._add_subscription(data_type),
        )

    def subscribe_instruments(self) -> None:
        instrument_ids = list(self._instrument_provider.get_all().keys())
        self.create_task(
            self._subscribe_instruments(),
            log_msg=f"subscribe: instruments {self.venue}",
            actions=lambda: [self._add_subscription_instrument(i) for i in instrument_ids],
        )

    def subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_instrument(instrument_id),
            log_msg=f"subscribe: instrument {instrument_id}",
            actions=lambda: self._add_subscription_instrument(instrument_id),
        )

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        self.create_task(
            self._subscribe_order_book_deltas(
                instrument_id=instrument_id,
                book_type=book_type,
                depth=depth,
                kwargs=kwargs,
            ),
            log_msg=f"subscribe: order_book_deltas {instrument_id}",
            actions=lambda: self._add_subscription_order_book_deltas(instrument_id),
        )

    def subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        self.create_task(
            self._subscribe_order_book_snapshots(
                instrument_id=instrument_id,
                book_type=book_type,
                depth=depth,
                kwargs=kwargs,
            ),
            log_msg=f"subscribe: order_book_snapshots {instrument_id}",
            actions=lambda: self._add_subscription_order_book_snapshots(instrument_id),
        )

    def subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_quote_ticks(instrument_id),
            log_msg=f"subscribe: quote_ticks {instrument_id}",
            actions=lambda: self._add_subscription_quote_ticks(instrument_id),
        )

    def subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_trade_ticks(instrument_id),
            log_msg=f"subscribe: trade_ticks {instrument_id}",
            actions=lambda: self._add_subscription_trade_ticks(instrument_id),
        )

    def subscribe_bars(self, bar_type: BarType) -> None:
        PyCondition.true(bar_type.is_externally_aggregated(), "aggregation_source is not EXTERNAL")

        self.create_task(
            self._subscribe_bars(bar_type),
            log_msg=f"subscribe: bars {bar_type}",
            actions=lambda: self._add_subscription_bars(bar_type),
        )

    def subscribe_instrument_status(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_instrument_status(instrument_id),
            log_msg=f"subscribe: instrument_status {instrument_id}",
            actions=lambda: self._add_subscription_instrument_status(instrument_id),
        )

    def subscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_instrument_close(instrument_id),
            log_msg=f"subscribe: instrument_close {instrument_id}",
            actions=lambda: self._add_subscription_instrument_close(instrument_id),
        )

    def unsubscribe(self, data_type: DataType) -> None:
        self.create_task(
            self._unsubscribe(data_type),
            log_msg=f"unsubscribe {data_type}",
            actions=lambda: self._remove_subscription(data_type),
        )

    def unsubscribe_instruments(self) -> None:
        instrument_ids = list(self._instrument_provider.get_all().keys())
        self.create_task(
            self._unsubscribe_instruments(),
            actions=lambda: [self._remove_subscription_instrument(i) for i in instrument_ids],
        )

    def unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_instrument(instrument_id),
            log_msg=f"unsubscribe: instrument {instrument_id}",
            actions=lambda: self._remove_subscription_instrument(instrument_id),
        )

    def unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_order_book_deltas(instrument_id),
            log_msg=f"unsubscribe: order_book_deltas {instrument_id}",
            actions=lambda: self._remove_subscription_order_book_deltas(instrument_id),
        )

    def unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_order_book_snapshots(instrument_id),
            log_msg=f"unsubscribe: order_book_snapshots {instrument_id}",
            actions=lambda: self._remove_subscription_order_book_snapshots(instrument_id),
        )

    def unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_quote_ticks(instrument_id),
            log_msg=f"unsubscribe: quote_ticks {instrument_id}",
            actions=lambda: self._remove_subscription_quote_ticks(instrument_id),
        )

    def unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_trade_ticks(instrument_id),
            log_msg=f"unsubscribe: trade_ticks {instrument_id}",
            actions=lambda: self._remove_subscription_trade_ticks(instrument_id),
        )

    def unsubscribe_bars(self, bar_type: BarType) -> None:
        self.create_task(
            self._unsubscribe_bars(bar_type),
            log_msg=f"unsubscribe: bars {bar_type}",
            actions=lambda: self._remove_subscription_bars(bar_type),
        )

    def unsubscribe_instrument_status(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_instrument_status(instrument_id),
            log_msg=f"unsubscribe: instrument_status {instrument_id}",
            actions=lambda: self._remove_subscription_instrument_status(instrument_id),
        )

    def unsubscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_instrument_close(instrument_id),
            log_msg=f"unsubscribe: instrument_close {instrument_id}",
            actions=lambda: self._remove_subscription_instrument_close(instrument_id),
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, data_type: DataType, correlation_id: UUID4) -> None:
        self._log.debug(f"Request data {data_type}.")
        self.create_task(
            self._request(data_type, correlation_id),
            log_msg=f"request: {data_type}",
        )

    def request_instrument(
        self,
        instrument_id: InstrumentId,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        self._log.debug(f"Request instrument {instrument_id}.")
        self.create_task(
            self._request_instrument(
                instrument_id=instrument_id,
                correlation_id=correlation_id,
                start=start,
                end=end,
            ),
            log_msg=f"request: instrument {instrument_id}",
        )

    def request_instruments(
        self,
        venue: Venue,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        self._log.debug(f"Request instruments for {venue} {correlation_id}.")
        self.create_task(
            self._request_instruments(
                venue=venue,
                correlation_id=correlation_id,
                start=start,
                end=end,
            ),
            log_msg=f"request: instruments for {venue}",
        )

    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        self._log.debug(f"Request quote ticks {instrument_id}.")
        self.create_task(
            self._request_quote_ticks(
                instrument_id=instrument_id,
                limit=limit,
                correlation_id=correlation_id,
                start=start,
                end=end,
            ),
            log_msg=f"request: quote ticks {instrument_id}",
        )

    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        self._log.debug(f"Request trade ticks {instrument_id}.")
        self.create_task(
            self._request_trade_ticks(
                instrument_id=instrument_id,
                limit=limit,
                correlation_id=correlation_id,
                start=start,
                end=end,
            ),
            log_msg=f"request: trade ticks {instrument_id}",
        )

    def request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        self._log.debug(f"Request bars {bar_type}.")
        self.create_task(
            self._request_bars(
                bar_type=bar_type,
                limit=limit,
                correlation_id=correlation_id,
                start=start,
                end=end,
            ),
            log_msg=f"request: bars {bar_type}",
        )

    ############################################################################
    # Coroutines to implement
    ############################################################################
    async def _connect(self):
        raise NotImplementedError(  # pragma: no cover
            "implement the `_connect` coroutine",  # pragma: no cover
        )

    async def _disconnect(self):
        raise NotImplementedError(  # pragma: no cover
            "implement the `_disconnect` coroutine",  # pragma: no cover
        )

    async def _subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe` coroutine",  # pragma: no cover
        )

    async def _subscribe_instruments(self) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_instruments` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_instrument` coroutine",  # pragma: no cover
        )

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_order_book_deltas` coroutine",  # pragma: no cover
        )

    async def _subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_order_book_snapshots` coroutine",  # pragma: no cover
        )

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_bars` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument_status(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_instrument_status` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_instrument_close` coroutine",  # pragma: no cover
        )

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instruments(self) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_instruments` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_instrument` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_order_book_deltas` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_order_book_snapshots` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_bars` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument_status(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_instrument_status` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_instrument_close` coroutine",  # pragma: no cover
        )

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request` coroutine",  # pragma: no cover
        )

    async def _request_instrument(
        self,
        instrument_id: InstrumentId,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_instrument` coroutine",  # pragma: no cover
        )

    async def _request_instruments(
        self,
        venue: Venue,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_instruments` coroutine",  # pragma: no cover
        )

    async def _request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_bars` coroutine",  # pragma: no cover
        )
