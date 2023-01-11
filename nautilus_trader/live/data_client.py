# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
The `LiveDataClient` class is responsible for interfacing with a particular API
which may be presented directly by an exchange, or broker intermediary. It
could also be possible to write clients for specialized data publishers.
"""

import asyncio
import functools
from asyncio import Task
from collections.abc import Coroutine
from typing import Any, Callable, Optional

import pandas as pd

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.base import DataType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.msgbus.bus import MessageBus


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
    logger : Logger
        The logger for the client.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client_id: ClientId,
        venue: Optional[Venue],
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        config: Optional[dict[str, Any]] = None,
    ):
        super().__init__(
            client_id=client_id,
            venue=venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop

    async def run_after_delay(self, delay, coro) -> None:
        await asyncio.sleep(delay)
        return await coro

    def create_task(
        self,
        coro: Coroutine,
        name: Optional[str] = None,
        actions: Optional[Callable] = None,
        success: Optional[str] = None,
    ):
        name = name or coro.__name__
        self._log.debug(f"Creating task {name}.")
        task = self._loop.create_task(
            coro,
            name=name,
        )
        task.add_done_callback(
            functools.partial(
                self._on_task_completed,
                actions,
                success,
            ),
        )

    def _on_task_completed(
        self,
        actions: Optional[Callable],
        success: Optional[str],
        task: Task,
    ) -> None:
        if task.exception():
            self._log.error(
                f"Error on `{task.get_name()}`: " f"{repr(task.exception())}",
            )
        else:
            if actions:
                try:
                    actions()
                except Exception as e:
                    self._log.error(
                        f"Failed triggering action {actions.__name__} on `{task.get_name()}`: "
                        f"{repr(e)}",
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
            name="connect",
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
            name="disconnect",
            actions=lambda: self._set_connected(False),
            success="Disconnected",
        )

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        self.create_task(
            self._subscribe(data_type),
            name=f"subscribe: {data_type}",
            actions=lambda: self._add_subscription(data_type),
        )

    def unsubscribe(self, data_type: DataType) -> None:
        self.create_task(
            self._unsubscribe(data_type),
            name=f"unsubscribe_{data_type}",
            actions=lambda: self._remove_subscription(data_type),
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, data_type: DataType, correlation_id: UUID4) -> None:
        self._log.debug(f"Request {data_type} {correlation_id}.")
        self.create_task(
            self._request(data_type, correlation_id),
            name=f"request_{data_type}",
        )

    ############################################################################
    # Coroutines to implement
    ############################################################################
    async def _connect(self) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_connect` coroutine",  # pragma: no cover
        )

    async def _disconnect(self) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_disconnect` coroutine",  # pragma: no cover
        )

    async def _subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe` coroutine",  # pragma: no cover
        )

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe` coroutine",  # pragma: no cover
        )

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_request` coroutine",  # pragma: no cover
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
    instrument_provider : InstrumentProvider
        The instrument provider for the client.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    logger : Logger
        The logger for the client.
    config : dict[str, object], optional
        The configuration for the instance.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client_id: ClientId,
        venue: Optional[Venue],
        instrument_provider: InstrumentProvider,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        config: Optional[dict[str, Any]] = None,
    ):
        PyCondition.type(instrument_provider, InstrumentProvider, "instrument_provider")

        super().__init__(
            client_id=client_id,
            venue=venue,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self._loop = loop
        self._instrument_provider = instrument_provider

    async def run_after_delay(self, delay, coro) -> None:
        await asyncio.sleep(delay)
        return await coro

    def create_task(
        self,
        coro: Coroutine,
        name: Optional[str] = None,
        actions: Optional[Callable] = None,
        success: Optional[str] = None,
    ):
        name = name or coro.__name__
        self._log.debug(f"Creating task {name}.")
        task = self._loop.create_task(
            coro,
            name=name,
        )
        task.add_done_callback(
            functools.partial(
                self._on_task_completed,
                actions,
                success,
            ),
        )

    def _on_task_completed(
        self,
        actions: Optional[Callable],
        success: Optional[str],
        task: Task,
    ) -> None:
        if task.exception():
            self._log.error(
                f"Error on `{task.get_name()}`: " f"{repr(task.exception())}",
            )
        else:
            if actions:
                try:
                    actions()
                except Exception as e:
                    self._log.error(
                        f"Failed triggering action {actions.__name__} on `{task.get_name()}`: "
                        f"{repr(e)}",
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
            name="connected",
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
            name="disconnect",
            actions=lambda: self._set_connected(False),
            success="Disconnected",
        )

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, data_type: DataType) -> None:
        self.create_task(
            self._subscribe(data_type),
            name=f"subscribe: {data_type}",
            actions=lambda: self._add_subscription(data_type),
        )

    def subscribe_instruments(self) -> None:
        instrument_ids = list(self._instrument_provider.get_all().keys())
        self.create_task(
            self._subscribe_instruments(),
            name="subscribe_all_instruments",
            actions=lambda: [self._add_subscription_instrument(i) for i in instrument_ids],
        )

    def subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_instruments(),
            name=f"subscribe: instrument_{instrument_id}",
            actions=lambda: self._add_subscription_instrument(instrument_id),
        )

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict[str, Any] = None,
    ) -> None:
        self.create_task(
            self._subscribe_order_book_deltas(
                instrument_id=instrument_id,
                book_type=book_type,
                depth=depth,
                kwargs=kwargs,
            ),
            name=f"subscribe: order_book_deltas: {instrument_id}",
            actions=lambda: self._add_subscription_order_book_deltas(instrument_id),
        )

    def subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict = None,
    ) -> None:
        self.create_task(
            self._subscribe_order_book_snapshots(
                instrument_id=instrument_id,
                book_type=book_type,
                depth=depth,
                kwargs=kwargs,
            ),
            name=f"subscribe: order_book_snapshots: {instrument_id}",
            actions=lambda: self._add_subscription_order_book_snapshots(instrument_id),
        )

    def subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_ticker(instrument_id),
            name=f"subscribe: ticker: {instrument_id}",
            actions=lambda: self._add_subscription_ticker(instrument_id),
        )

    def subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_quote_ticks(instrument_id),
            name=f"subscribe: quote_ticks: {instrument_id}",
            actions=lambda: self._add_subscription_quote_ticks(instrument_id),
        )

    def subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_trade_ticks(instrument_id),
            name=f"subscribe: trade_ticks: {instrument_id}",
            actions=lambda: self._add_subscription_trade_ticks(instrument_id),
        )

    def subscribe_bars(self, bar_type: BarType) -> None:
        PyCondition.true(bar_type.is_externally_aggregated(), "aggregation_source is not EXTERNAL")

        self.create_task(
            self._subscribe_bars(bar_type),
            name=f"subscribe: bars {bar_type}",
            actions=lambda: self._add_subscription_bars(bar_type),
        )

    def subscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_instrument_status_updates(instrument_id),
            name=f"subscribe: instrument_status_updates: {instrument_id}",
            actions=lambda: self._add_subscription_instrument_status_updates(instrument_id),
        )

    def subscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._subscribe_instrument_close(instrument_id),
            name=f"subscribe: instrument_close: {instrument_id}",
            actions=lambda: self._add_subscription_instrument_close(instrument_id),
        )

    def unsubscribe(self, data_type: DataType) -> None:
        self.create_task(
            self._unsubscribe(data_type),
            name=f"unsubscribe: {data_type}",
            actions=lambda: self._remove_subscription(data_type),
        )

    def unsubscribe_instruments(self) -> None:
        instrument_ids = list(self._instrument_provider.get_all().keys())
        self.create_task(
            self._unsubscribe_instruments(),
            name="unsubscribe_instruments",
            actions=lambda: [self._remove_subscription_instrument(i) for i in instrument_ids],
        )

    def unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_instrument(instrument_id),
            name=f"unsubscribe_instrument: {instrument_id}",
            actions=lambda: self._remove_subscription_instrument(instrument_id),
        )

    def unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_order_book_deltas(instrument_id),
            name=f"unsubscribe_order_book_deltas: {instrument_id}",
            actions=lambda: self._remove_subscription_order_book_deltas(instrument_id),
        )

    def unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_order_book_snapshots(instrument_id),
            name=f"unsubscribe_order_book_snapshots: {instrument_id}",
            actions=lambda: self._remove_subscription_order_book_snapshots(instrument_id),
        )

    def unsubscribe_ticker(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_ticker(instrument_id),
            name=f"unsubscribe_ticker: {instrument_id}",
            actions=lambda: self._remove_subscription_ticker(instrument_id),
        )

    def unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_quote_ticks(instrument_id),
            name=f"unsubscribe_quote_ticks: {instrument_id}",
            actions=lambda: self._remove_subscription_quote_ticks(instrument_id),
        )

    def unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_trade_ticks(instrument_id),
            name=f"unsubscribe_trade_ticks: {instrument_id}",
            actions=lambda: self._remove_subscription_trade_ticks(instrument_id),
        )

    def unsubscribe_bars(self, bar_type: BarType) -> None:
        self.create_task(
            self._unsubscribe_bars(bar_type),
            name=f"unsubscribe_bars: {bar_type}",
            actions=lambda: self._remove_subscription_bars(bar_type),
        )

    def unsubscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_instrument_status_updates(instrument_id),
            name=f"unsubscribe_instrument_status_updates: {instrument_id}",
            actions=lambda: self._remove_subscription_instrument_status_updates(instrument_id),
        )

    def unsubscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        self.create_task(
            self._unsubscribe_instrument_close(instrument_id),
            name=f"unsubscribe_instrument_close: {instrument_id}",
            actions=lambda: self._remove_subscription_instrument_close(instrument_id),
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, data_type: DataType, correlation_id: UUID4) -> None:
        self.create_task(
            self._request(data_type, correlation_id),
            name=f"request {data_type}",
        )

    def request_instrument(self, instrument_id: InstrumentId, correlation_id: UUID4):
        self.create_task(
            self._request_instrument(instrument_id, correlation_id),
            name=f"request_instrument: {instrument_id}",
        )

    def request_instruments(self, venue: Venue, correlation_id: UUID4):
        self._log.debug(f"Request instruments for {venue} {correlation_id}.")
        self.create_task(
            self._request_instruments(venue, correlation_id),
            name="request_instruments",
        )

    def request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        self._log.debug(f"Request quote ticks {instrument_id}.")
        self.create_task(
            self._request_quote_ticks(
                instrument_id=instrument_id,
                limit=limit,
                correlation_id=correlation_id,
                from_datetime=from_datetime,
                to_datetime=to_datetime,
            ),
            name="request_quote_ticks",
        )

    def request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        self._log.debug(f"Request trade ticks {instrument_id}.")
        self.create_task(
            self._request_trade_ticks(
                instrument_id=instrument_id,
                limit=limit,
                correlation_id=correlation_id,
                from_datetime=from_datetime,
                to_datetime=to_datetime,
            ),
            name="request_trade_ticks",
        )

    def request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        self._log.debug(f"Request bars {bar_type}.")
        self.create_task(
            self._request_bars(
                bar_type=bar_type,
                limit=limit,
                correlation_id=correlation_id,
                from_datetime=from_datetime,
                to_datetime=to_datetime,
            ),
            name="request_bars",
        )

    ############################################################################
    # Coroutines to implement
    ############################################################################
    async def _connect(self):
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_connect` coroutine",  # pragma: no cover
        )

    async def _disconnect(self):
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_disconnect` coroutine",  # pragma: no cover
        )

    async def _subscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe` coroutine",  # pragma: no cover
        )

    async def _subscribe_instruments(self) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_instruments` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_instrument` coroutine",  # pragma: no cover
        )

    async def _subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict[str, Any] = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_order_book_deltas` coroutine",  # pragma: no cover
        )

    async def _subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = None,
        kwargs: dict[str, Any] = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_order_book_snapshots` coroutine",  # pragma: no cover
        )

    async def _subscribe_ticker(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_ticker` coroutine",  # pragma: no cover
        )

    async def _subscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_bars` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_instrument_status_updates` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_subscribe_instrument_close` coroutine",  # pragma: no cover
        )

    async def _unsubscribe(self, data_type: DataType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instruments(self) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_instruments` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_instrument` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_order_book_deltas(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_order_book_deltas` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_order_book_snapshots(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_order_book_snapshots` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_ticker(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_ticker` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_quote_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_bars` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument_status_updates(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_instrument_status_updates` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_unsubscribe_instrument_close` coroutine",  # pragma: no cover
        )

    async def _request(self, data_type: DataType, correlation_id: UUID4) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_request` coroutine",  # pragma: no cover
        )

    async def _request_instrument(self, instrument_id: InstrumentId, correlation_id: UUID4):
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_request_instrument` coroutine",  # pragma: no cover
        )

    async def _request_instruments(self, venue: Venue, correlation_id: UUID4):
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_request_instruments` coroutine",  # pragma: no cover
        )

    async def _request_quote_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_request_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_request_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        from_datetime: Optional[pd.Timestamp] = None,
        to_datetime: Optional[pd.Timestamp] = None,
    ) -> None:
        raise NotImplementedError(  # pragma: no cover
            "please implement the `_request_bars` coroutine",  # pragma: no cover
        )
