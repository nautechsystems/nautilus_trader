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
may be presented directly by a venue, or through a broker intermediary.

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
        success_color : LogColor, default ``NORMAL``
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
                    tb_str = "".join(traceback.format_exception(type(e), e, e.__traceback__))
                    self._log.error(
                        f"Failed triggering action {actions.__name__} on '{task.get_name()}': "
                        f"{e!r}\n{tb_str}",
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

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        self._add_subscription(data_type)
        self.create_task(
            self._subscribe(data_type),
            log_msg=f"subscribe: {data_type}",
            success_msg=f"Subscribed {data_type}",
            success_color=LogColor.BLUE,
        )

    def unsubscribe(self, data_type: DataType) -> None:
        self._remove_subscription(data_type)
        self.create_task(
            self._unsubscribe(data_type),
            log_msg=f"unsubscribe_{data_type}",
            success_msg=f"Unsubscribed {data_type}",
            success_color=LogColor.BLUE,
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, data_type: DataType, correlation_id: UUID4) -> None:
        self._log.debug(f"Request {data_type} {correlation_id}")
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
        success_color : LogColor, default ``NORMAL``
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
                    tb_str = "".join(traceback.format_exception(type(e), e, e.__traceback__))
                    self._log.error(
                        f"Failed triggering action {actions.__name__} on '{task.get_name()}': "
                        f"{e!r}\n{tb_str}",
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

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        self._add_subscription(data_type)
        self.create_task(
            self._subscribe(data_type),
            log_msg=f"subscribe: {data_type}",
            success_msg=f"Subscribed {data_type}",
            success_color=LogColor.BLUE,
        )

    def subscribe_instruments(self) -> None:
        instrument_ids = list(self._instrument_provider.get_all().keys())
        [self._add_subscription_instrument(i) for i in instrument_ids]
        self.create_task(
            self._subscribe_instruments(),
            log_msg=f"subscribe: instruments {self.venue}",
            success_msg=f"Subscribed {self.venue} instruments",
            success_color=LogColor.BLUE,
        )

    def subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self._add_subscription_instrument(instrument_id)
        self.create_task(
            self._subscribe_instrument(instrument_id),
            log_msg=f"subscribe: instrument {instrument_id}",
            success_msg=f"Subscribed {instrument_id} instrument",
            success_color=LogColor.BLUE,
        )

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        self._add_subscription_order_book_deltas(instrument_id)
        self.create_task(
            self._subscribe_order_book_deltas(
                instrument_id=instrument_id,
                book_type=book_type,
                depth=depth,
                kwargs=kwargs,
            ),
            log_msg=f"subscribe: order_book_deltas {instrument_id}",
            success_msg=f"Subscribed {instrument_id} order book deltas depth={depth}",
            success_color=LogColor.BLUE,
        )

    def subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int | None = None,
        kwargs: dict[str, Any] | None = None,
    ) -> None:
        self._add_subscription_order_book_snapshots(instrument_id)
        self.create_task(
            self._subscribe_order_book_snapshots(
                instrument_id=instrument_id,
                book_type=book_type,
                depth=depth,
                kwargs=kwargs,
            ),
            log_msg=f"subscribe: order_book_snapshots {instrument_id}",
            success_msg=f"Subscribed {instrument_id} order book snapshots depth={depth}",
            success_color=LogColor.BLUE,
        )

    def subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self._add_subscription_quote_ticks(instrument_id)
        self.create_task(
            self._subscribe_quote_ticks(instrument_id),
            log_msg=f"subscribe: quote_ticks {instrument_id}",
            success_msg=f"Subscribed {instrument_id} quotes",
            success_color=LogColor.BLUE,
        )

    def subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self._add_subscription_trade_ticks(instrument_id)
        self.create_task(
            self._subscribe_trade_ticks(instrument_id),
            log_msg=f"subscribe: trade_ticks {instrument_id}",
            success_msg=f"Subscribed {instrument_id} trades",
            success_color=LogColor.BLUE,
        )

    def subscribe_bars(self, bar_type: BarType) -> None:
        PyCondition.true(bar_type.is_externally_aggregated(), "aggregation_source is not EXTERNAL")

        self._add_subscription_bars(bar_type)
        self.create_task(
            self._subscribe_bars(bar_type),
            log_msg=f"subscribe: bars {bar_type}",
            success_msg=f"Subscribed {bar_type} bars",
            success_color=LogColor.BLUE,
        )

    def subscribe_instrument_status(self, instrument_id: InstrumentId) -> None:
        self._add_subscription_instrument_status(instrument_id)
        self.create_task(
            self._subscribe_instrument_status(instrument_id),
            log_msg=f"subscribe: instrument_status {instrument_id}",
            success_msg=f"Subscribed {instrument_id} instrument status ",
            success_color=LogColor.BLUE,
        )

    def subscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        self._add_subscription_instrument_close(instrument_id)
        self.create_task(
            self._subscribe_instrument_close(instrument_id),
            log_msg=f"subscribe: instrument_close {instrument_id}",
            success_msg=f"Subscribed {instrument_id} instrument close",
            success_color=LogColor.BLUE,
        )

    def unsubscribe(self, data_type: DataType) -> None:
        self._remove_subscription(data_type)
        self.create_task(
            self._unsubscribe(data_type),
            log_msg=f"unsubscribe {data_type}",
            success_msg=f"Unsubscribed {data_type}",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_instruments(self) -> None:
        instrument_ids = list(self._instrument_provider.get_all().keys())
        [self._remove_subscription_instrument(i) for i in instrument_ids]
        self.create_task(
            self._unsubscribe_instruments(),
            log_msg=f"unsubscribe: instruments {self.venue}",
            success_msg=f"Unsubscribed {self.venue} instruments",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_instrument(instrument_id)
        self.create_task(
            self._unsubscribe_instrument(instrument_id),
            log_msg=f"unsubscribe: instrument {instrument_id}",
            success_msg=f"Unsubscribed {instrument_id} instrument",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_order_book_deltas(instrument_id)
        self.create_task(
            self._unsubscribe_order_book_deltas(instrument_id),
            log_msg=f"unsubscribe: order_book_deltas {instrument_id}",
            success_msg=f"Unsubscribed {instrument_id} order book deltas",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_order_book_snapshots(instrument_id)
        self.create_task(
            self._unsubscribe_order_book_snapshots(instrument_id),
            log_msg=f"unsubscribe: order_book_snapshots {instrument_id}",
            success_msg=f"Unsubscribed {instrument_id} order book snapshots",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_quote_ticks(instrument_id)
        self.create_task(
            self._unsubscribe_quote_ticks(instrument_id),
            log_msg=f"unsubscribe: quote_ticks {instrument_id}",
            success_msg=f"Unsubscribed {instrument_id} quotes",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_trade_ticks(instrument_id)
        self.create_task(
            self._unsubscribe_trade_ticks(instrument_id),
            log_msg=f"unsubscribe: trade_ticks {instrument_id}",
            success_msg=f"Unsubscribed {instrument_id} trades",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_bars(self, bar_type: BarType) -> None:
        self._remove_subscription_bars(bar_type)
        self.create_task(
            self._unsubscribe_bars(bar_type),
            log_msg=f"unsubscribe: bars {bar_type}",
            success_msg=f"Unsubscribed {bar_type} bars",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_instrument_status(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_instrument_status(instrument_id)
        self.create_task(
            self._unsubscribe_instrument_status(instrument_id),
            log_msg=f"unsubscribe: instrument_status {instrument_id}",
            success_msg=f"Unsubscribed {instrument_id} instrument status",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        self._remove_subscription_instrument_close(instrument_id)
        self.create_task(
            self._unsubscribe_instrument_close(instrument_id),
            log_msg=f"unsubscribe: instrument_close {instrument_id}",
            success_msg=f"Unsubscribed {instrument_id} instrument close",
            success_color=LogColor.BLUE,
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, data_type: DataType, correlation_id: UUID4) -> None:
        self._log.info(f"Request {data_type}", LogColor.BLUE)
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
        time_range = f" {start} to {end}" if (start or end) else ""
        self._log.info(f"Request {instrument_id} instrument{time_range}", LogColor.BLUE)
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
        time_range = f" {start} to {end}" if (start or end) else ""
        self._log.info(
            f"Request {venue} instruments for{time_range}",
            LogColor.BLUE,
        )
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
        time_range = f" {start} to {end}" if (start or end) else ""
        limit_str = f" limit={limit}" if limit else ""
        self._log.info(f"Request {instrument_id} quote ticks{time_range}{limit_str}", LogColor.BLUE)
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
        time_range = f" {start} to {end}" if (start or end) else ""
        limit_str = f" limit={limit}" if limit else ""
        self._log.info(f"Request {instrument_id} trade ticks{time_range}{limit_str}", LogColor.BLUE)
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
        time_range = f" {start} to {end}" if (start or end) else ""
        limit_str = f" limit={limit}" if limit else ""
        self._log.info(f"Request {bar_type} bars{time_range}{limit_str}", LogColor.BLUE)
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

    def request_order_book_snapshot(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
    ) -> None:
        limit_str = f" limit={limit}" if limit else ""
        self._log.info(f"Request {instrument_id} order_book_snapshot{limit_str}", LogColor.BLUE)
        self.create_task(
            self._request_order_book_snapshot(
                instrument_id=instrument_id,
                limit=limit,
                correlation_id=correlation_id,
            ),
            log_msg=f"request: order_book_snapshot {instrument_id}",
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

    async def _request_order_book_snapshot(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
    ) -> None:
        raise NotImplementedError(
            "implement the `_request_order_book_snapshot` coroutine",  # pragma: no cover
        )  # pra
