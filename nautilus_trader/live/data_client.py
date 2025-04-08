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

from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.functions import format_utc_timerange
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.data.client import DataClient
from nautilus_trader.data.client import MarketDataClient
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestOrderBookSnapshot
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import SubscribeIndexPrices
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstrumentClose
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeInstrumentStatus
from nautilus_trader.data.messages import SubscribeMarkPrices
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstrumentClose
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeInstrumentStatus
from nautilus_trader.data.messages import UnsubscribeMarkPrices
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.model.identifiers import ClientId
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
    venue : Venue or ``None``
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

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, command: SubscribeData) -> None:
        self._add_subscription(command.data_type)
        self.create_task(
            self._subscribe(command),
            log_msg=f"subscribe: {command.data_type}",
            success_msg=f"Subscribed {command.data_type}",
            success_color=LogColor.BLUE,
        )

    def unsubscribe(self, command: UnsubscribeData) -> None:
        self._remove_subscription(command.data_type)
        self.create_task(
            self._unsubscribe(command),
            log_msg=f"unsubscribe_{command.data_type}",
            success_msg=f"Unsubscribed {command.data_type}",
            success_color=LogColor.BLUE,
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, request: RequestData) -> None:
        self._log.debug(f"Request {request.data_type} {request.request_id}")
        self.create_task(
            self._request(request),
            log_msg=f"request_{request.data_type}",
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

    async def _subscribe(self, command: SubscribeData) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe` coroutine",  # pragma: no cover
        )

    async def _unsubscribe(self, command: UnsubscribeBars) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe` coroutine",  # pragma: no cover
        )

    async def _request(self, request: RequestData) -> None:
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
    venue : Venue or ``None``
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

    # -- SUBSCRIPTIONS ----------------------------------------------------------------------------

    def subscribe(self, command: SubscribeData) -> None:
        self._add_subscription(command.data_type)
        self.create_task(
            self._subscribe(command),
            log_msg=f"subscribe: {command.data_type}",
            success_msg=f"Subscribed {command.data_type}",
            success_color=LogColor.BLUE,
        )

    def subscribe_instruments(self, command: SubscribeInstruments) -> None:
        instrument_ids = list(self._instrument_provider.get_all().keys())
        [self._add_subscription_instrument(i) for i in instrument_ids]
        self.create_task(
            self._subscribe_instruments(command),
            log_msg=f"subscribe: instruments {self.venue}",
            success_msg=f"Subscribed {self.venue} instruments",
            success_color=LogColor.BLUE,
        )

    def subscribe_instrument(self, command: SubscribeInstrument) -> None:
        self._add_subscription_instrument(command.instrument_id)
        self.create_task(
            self._subscribe_instrument(command),
            log_msg=f"subscribe: instrument {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} instrument",
            success_color=LogColor.BLUE,
        )

    def subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        self._add_subscription_order_book_deltas(command.instrument_id)
        self.create_task(
            self._subscribe_order_book_deltas(command),
            log_msg=f"subscribe: order_book_deltas {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} order book deltas depth={command.depth}",
            success_color=LogColor.BLUE,
        )

    def subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        self._add_subscription_order_book_snapshots(command.instrument_id)
        self.create_task(
            self._subscribe_order_book_snapshots(command),
            log_msg=f"subscribe: order_book_snapshots {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} order book snapshots depth={command.depth}",
            success_color=LogColor.BLUE,
        )

    def subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        self._add_subscription_quote_ticks(command.instrument_id)
        self.create_task(
            self._subscribe_quote_ticks(command),
            log_msg=f"subscribe: quote_ticks {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} quotes",
            success_color=LogColor.BLUE,
        )

    def subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        self._add_subscription_trade_ticks(command.instrument_id)
        self.create_task(
            self._subscribe_trade_ticks(command),
            log_msg=f"subscribe: trade_ticks {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} trades",
            success_color=LogColor.BLUE,
        )

    def subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        self._add_subscription_mark_prices(command.instrument_id)
        self.create_task(
            self._subscribe_mark_prices(command),
            log_msg=f"subscribe: mark_prices {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} mark prices",
            success_color=LogColor.BLUE,
        )

    def subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        self._add_subscription_index_prices(command.instrument_id)
        self.create_task(
            self._subscribe_index_prices(command),
            log_msg=f"subscribe: index_prices {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} index prices",
            success_color=LogColor.BLUE,
        )

    def subscribe_bars(self, command: SubscribeBars) -> None:
        PyCondition.is_true(
            command.bar_type.is_externally_aggregated(),
            "aggregation_source is not EXTERNAL",
        )

        self._add_subscription_bars(command.bar_type)
        self.create_task(
            self._subscribe_bars(command),
            log_msg=f"subscribe: bars {command.bar_type}",
            success_msg=f"Subscribed {command.bar_type} bars",
            success_color=LogColor.BLUE,
        )

    def subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        self._add_subscription_instrument_status(command.instrument_id)
        self.create_task(
            self._subscribe_instrument_status(command),
            log_msg=f"subscribe: instrument_status {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} instrument status ",
            success_color=LogColor.BLUE,
        )

    def subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        self._add_subscription_instrument_close(command.instrument_id)
        self.create_task(
            self._subscribe_instrument_close(command),
            log_msg=f"subscribe: instrument_close {command.instrument_id}",
            success_msg=f"Subscribed {command.instrument_id} instrument close",
            success_color=LogColor.BLUE,
        )

    def unsubscribe(self, command: UnsubscribeData) -> None:
        self._remove_subscription(command.data_type)
        self.create_task(
            self._unsubscribe(command),
            log_msg=f"unsubscribe {command.data_type}",
            success_msg=f"Unsubscribed {command.data_type}",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        instrument_ids = list(self._instrument_provider.get_all().keys())
        [self._remove_subscription_instrument(i) for i in instrument_ids]
        self.create_task(
            self._unsubscribe_instruments(command),
            log_msg=f"unsubscribe: instruments {self.venue}",
            success_msg=f"Unsubscribed {self.venue} instruments",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        self._remove_subscription_instrument(command.instrument_id)
        self.create_task(
            self._unsubscribe_instrument(command),
            log_msg=f"unsubscribe: instrument {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} instrument",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        self._remove_subscription_order_book_deltas(command.instrument_id)
        self.create_task(
            self._unsubscribe_order_book_deltas(command),
            log_msg=f"unsubscribe: order_book_deltas {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} order book deltas",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        self._remove_subscription_order_book_snapshots(command.instrument_id)
        self.create_task(
            self._unsubscribe_order_book_snapshots(command),
            log_msg=f"unsubscribe: order_book_snapshots {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} order book snapshots",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        self._remove_subscription_quote_ticks(command.instrument_id)
        self.create_task(
            self._unsubscribe_quote_ticks(command),
            log_msg=f"unsubscribe: quote_ticks {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} quotes",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        self._remove_subscription_trade_ticks(command.instrument_id)
        self.create_task(
            self._unsubscribe_trade_ticks(command),
            log_msg=f"unsubscribe: trade_ticks {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} trades",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_mark_prices(self, command: UnsubscribeMarkPrices) -> None:
        self._remove_subscription_trade_ticks(command.instrument_id)
        self.create_task(
            self._unsubscribe_trade_ticks(command),
            log_msg=f"unsubscribe: mark_prices {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} mark prices",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_index_prices(self, command: UnsubscribeMarkPrices) -> None:
        self._remove_subscription_trade_ticks(command.instrument_id)
        self.create_task(
            self._unsubscribe_trade_ticks(command),
            log_msg=f"unsubscribe: index_prices {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} index prices",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        self._remove_subscription_bars(command.bar_type)
        self.create_task(
            self._unsubscribe_bars(command),
            log_msg=f"unsubscribe: bars {command.bar_type}",
            success_msg=f"Unsubscribed {command.bar_type} bars",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        self._remove_subscription_instrument_status(command.instrument_id)
        self.create_task(
            self._unsubscribe_instrument_status(command),
            log_msg=f"unsubscribe: instrument_status {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} instrument status",
            success_color=LogColor.BLUE,
        )

    def unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        self._remove_subscription_instrument_close(command.instrument_id)
        self.create_task(
            self._unsubscribe_instrument_close(command),
            log_msg=f"unsubscribe: instrument_close {command.instrument_id}",
            success_msg=f"Unsubscribed {command.instrument_id} instrument close",
            success_color=LogColor.BLUE,
        )

    # -- REQUESTS ---------------------------------------------------------------------------------

    def request(self, request: RequestData) -> None:
        self._log.info(f"Request {request.data_type}", LogColor.BLUE)
        self.create_task(
            self._request(request),
            log_msg=f"request: {request.data_type}",
        )

    def request_instrument(self, request: RequestInstrument) -> None:
        time_range_str = format_utc_timerange(request.start, request.end)
        self._log.info(f"Request {request.instrument_id} instrument{time_range_str}", LogColor.BLUE)
        self.create_task(
            self._request_instrument(request),
            log_msg=f"request: instrument {request.instrument_id}",
        )

    def request_instruments(self, request: RequestInstruments) -> None:
        time_range_str = format_utc_timerange(request.start, request.end)
        self._log.info(
            f"Request {request.venue} instruments for{time_range_str}",
            LogColor.BLUE,
        )
        self.create_task(
            self._request_instruments(request),
            log_msg=f"request: instruments for {request.venue}",
        )

    def request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        time_range_str = format_utc_timerange(request.start, request.end)
        limit_str = f" limit={request.limit}" if request.limit != 0 else ""
        self._log.info(
            f"Request {request.instrument_id} quotes{time_range_str}{limit_str}",
            LogColor.BLUE,
        )
        self.create_task(
            self._request_quote_ticks(request),
            log_msg=f"request: quotes {request.instrument_id}",
        )

    def request_trade_ticks(self, request: RequestTradeTicks) -> None:
        time_range_str = format_utc_timerange(request.start, request.end)
        limit_str = f" limit={request.limit}" if request.limit != 0 else ""
        self._log.info(
            f"Request {request.instrument_id} trades{time_range_str}{limit_str}",
            LogColor.BLUE,
        )
        self.create_task(
            self._request_trade_ticks(request),
            log_msg=f"request: trades {request.instrument_id}",
        )

    def request_bars(self, request: RequestBars) -> None:
        time_range_str = format_utc_timerange(request.start, request.end)
        limit_str = f" limit={request.limit}" if request.limit != 0 else ""
        self._log.info(f"Request {request.bar_type} bars{time_range_str}{limit_str}", LogColor.BLUE)
        self.create_task(
            self._request_bars(request),
            log_msg=f"request: bars {request.bar_type}",
        )

    def request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        limit_str = f" limit={request.limit}" if request.limit != 0 else ""
        self._log.info(
            f"Request {request.instrument_id} order_book_snapshot{limit_str}",
            LogColor.BLUE,
        )
        self.create_task(
            self._request_order_book_snapshot(request),
            log_msg=f"request: order_book_snapshot {request.instrument_id}",
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

    async def _subscribe(self, command: SubscribeData) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe` coroutine",  # pragma: no cover
        )

    async def _subscribe_instruments(self, command: SubscribeInstruments) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_instruments` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument(self, command: SubscribeInstrument) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_instrument` coroutine",  # pragma: no cover
        )

    async def _subscribe_order_book_deltas(self, command: SubscribeOrderBook) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_order_book_deltas` coroutine",  # pragma: no cover
        )

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_order_book_snapshots` coroutine",  # pragma: no cover
        )

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _subscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_mark_prices` coroutine",  # pragma: no cover
        )

    async def _subscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_index_prices` coroutine",  # pragma: no cover
        )

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_bars` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_instrument_status` coroutine",  # pragma: no cover
        )

    async def _subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_instrument_close` coroutine",  # pragma: no cover
        )

    async def _unsubscribe(self, command: UnsubscribeData) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instruments(self, command: UnsubscribeInstruments) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_instruments` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument(self, command: UnsubscribeInstrument) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_instrument` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_order_book_deltas(self, command: UnsubscribeOrderBook) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_order_book_deltas` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_order_book_snapshots` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_mark_prices(self, command: SubscribeMarkPrices) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_mark_prices` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_index_prices(self, command: SubscribeIndexPrices) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_index_prices` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_bars` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_instrument_status` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_instrument_close` coroutine",  # pragma: no cover
        )

    async def _request(self, request: RequestData) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request` coroutine",  # pragma: no cover
        )

    async def _request_instrument(self, request: RequestInstrument) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_instrument` coroutine",  # pragma: no cover
        )

    async def _request_instruments(self, request: RequestInstruments) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_instruments` coroutine",  # pragma: no cover
        )

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_quote_ticks` coroutine",  # pragma: no cover
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_trade_ticks` coroutine",  # pragma: no cover
        )

    async def _request_bars(self, request: RequestBars) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request_bars` coroutine",  # pragma: no cover
        )

    async def _request_order_book_snapshot(self, request: RequestOrderBookSnapshot) -> None:
        raise NotImplementedError(
            "implement the `_request_order_book_snapshot` coroutine",  # pragma: no cover
        )
