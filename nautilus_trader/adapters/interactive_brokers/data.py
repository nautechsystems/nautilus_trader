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
from __future__ import annotations

import asyncio

import pandas as pd

from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.parsing.data import timedelta_to_duration_str
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.data.messages import RequestBars
from nautilus_trader.data.messages import RequestData
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.data.messages import RequestInstruments
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import RequestTradeTicks
from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.data.messages import SubscribeInstrument
from nautilus_trader.data.messages import SubscribeInstrumentClose
from nautilus_trader.data.messages import SubscribeInstruments
from nautilus_trader.data.messages import SubscribeInstrumentStatus
from nautilus_trader.data.messages import SubscribeOrderBook
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.data.messages import UnsubscribeBars
from nautilus_trader.data.messages import UnsubscribeData
from nautilus_trader.data.messages import UnsubscribeInstrument
from nautilus_trader.data.messages import UnsubscribeInstrumentClose
from nautilus_trader.data.messages import UnsubscribeInstruments
from nautilus_trader.data.messages import UnsubscribeInstrumentStatus
from nautilus_trader.data.messages import UnsubscribeOrderBook
from nautilus_trader.data.messages import UnsubscribeQuoteTicks
from nautilus_trader.data.messages import UnsubscribeTradeTicks
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments.currency_pair import CurrencyPair


class InteractiveBrokersDataClient(LiveMarketDataClient):
    """
    Provides a data client for the InteractiveBrokers exchange by using the `Gateway` to
    stream market data.

    Parameters
    ----------
    loop : asyncio.AbstractEventLoop
        The event loop for the client.
    client : InteractiveBrokersClient
        The nautilus InteractiveBrokersClient using ibapi.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : LiveClock
        The clock for the client.
    instrument_provider : InteractiveBrokersInstrumentProvider
        The instrument provider.
    ibg_client_id : int
        Client ID used to connect TWS/Gateway.
    config : InteractiveBrokersDataClientConfig
        Configuration for the client.
    name : str, optional
        The custom client ID.

    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: InteractiveBrokersClient,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        instrument_provider: InteractiveBrokersInstrumentProvider,
        ibg_client_id: int,
        config: InteractiveBrokersDataClientConfig,
        name: str | None = None,
        connection_timeout: int = 300,
        request_timeout: int = 60,
    ) -> None:
        super().__init__(
            loop=loop,
            client_id=ClientId(name or f"{IB_VENUE.value}-{ibg_client_id:03d}"),
            venue=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )
        self._connection_timeout = connection_timeout
        self._request_timeout = request_timeout
        self._client = client
        self._handle_revised_bars = config.handle_revised_bars
        self._use_regular_trading_hours = config.use_regular_trading_hours
        self._market_data_type = config.market_data_type
        self._ignore_quote_tick_size_updates = config.ignore_quote_tick_size_updates

    @property
    def instrument_provider(self) -> InteractiveBrokersInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self):
        # Connect client
        await self._client.wait_until_ready(self._connection_timeout)
        self._client.registered_nautilus_clients.add(self.id)

        # Set instrument provider on client for price magnifier access
        self._client._instrument_provider = self._instrument_provider

        # Set Market Data Type
        await self._client.set_market_data_type(self._market_data_type)

        # Load instruments based on config
        await self.instrument_provider.initialize()
        for instrument in self._instrument_provider.list_all():
            self._handle_data(instrument)

    async def _disconnect(self):
        self._client.registered_nautilus_clients.discard(self.id)

        if self._client.is_running and self._client.registered_nautilus_clients == set():
            self._client.stop()

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
        if command.book_type == BookType.L3_MBO:
            self._log.error(
                "Cannot subscribe to order book deltas: "
                "L3_MBO data is not published by Interactive Brokers. "
                "Valid book types are L1_MBP, L2_MBP",
            )
            return

        if not (instrument := self._cache.instrument(command.instrument_id)):
            self._log.error(
                f"Cannot subscribe to order book deltas for {command.instrument_id}: instrument not found",
            )
            return

        depth = 20 if not command.depth else command.depth
        is_smart_depth = command.params.get("is_smart_depth", True)

        await self._client.subscribe_order_book(
            instrument_id=command.instrument_id,
            contract=IBContract(**instrument.info["contract"]),
            depth=depth,
            is_smart_depth=is_smart_depth,
        )

    async def _subscribe_order_book_snapshots(self, command: SubscribeOrderBook) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_subscribe_order_book_snapshots` coroutine",  # pragma: no cover
        )

    async def _subscribe_quote_ticks(self, command: SubscribeQuoteTicks) -> None:
        contract = self.instrument_provider.contract.get(command.instrument_id)
        if not contract:
            self._log.error(
                f"Cannot subscribe to quotes for {command.instrument_id}: instrument not found",
            )
            return

        # Use batch_quotes by default to avoid "Max number of tick-by-tick requests has been reached" error
        batch_quotes = command.params.get("batch_quotes", True)
        if contract.secType == "BAG" or batch_quotes:
            # For OptionSpread (BAG) instruments, always use reqMktData instead of reqTickByTickData
            # as not supported for BAG contracts
            await self._client.subscribe_market_data(
                instrument_id=command.instrument_id,
                contract=contract,
                generic_tick_list="",  # Empty for basic bid/ask data
            )
        else:
            await self._client.subscribe_ticks(
                instrument_id=command.instrument_id,
                contract=contract,
                tick_type="BidAsk",
                ignore_size=self._ignore_quote_tick_size_updates,
            )

    async def _subscribe_trade_ticks(self, command: SubscribeTradeTicks) -> None:
        if not (instrument := self._cache.instrument(command.instrument_id)):
            self._log.error(
                f"Cannot subscribe to trades for {command.instrument_id}: instrument not found",
            )
            return

        if isinstance(instrument, CurrencyPair):
            self._log.error(
                "Interactive Brokers does not support trades for CurrencyPair instruments",
            )
            return

        await self._client.subscribe_ticks(
            instrument_id=command.instrument_id,
            contract=IBContract(**instrument.info["contract"]),
            tick_type="AllLast",
            ignore_size=self._ignore_quote_tick_size_updates,
        )

    async def _subscribe_bars(self, command: SubscribeBars) -> None:
        contract = self.instrument_provider.contract.get(command.bar_type.instrument_id)

        if not contract:
            self._log.error(
                f"Cannot subscribe to bars for {command.bar_type.instrument_id}: instrument not found",
            )
            return

        if command.bar_type.spec.timedelta.total_seconds() == 5:
            await self._client.subscribe_realtime_bars(
                bar_type=command.bar_type,
                contract=contract,
                use_rth=self._use_regular_trading_hours,
            )
        else:
            await self._client.subscribe_historical_bars(
                bar_type=command.bar_type,
                contract=contract,
                use_rth=self._use_regular_trading_hours,
                handle_revised_bars=self._handle_revised_bars,
                params=command.params.copy(),
            )

    async def _subscribe_instrument_status(self, command: SubscribeInstrumentStatus) -> None:
        pass  # Subscribed as part of orderbook

    async def _subscribe_instrument_close(self, command: SubscribeInstrumentClose) -> None:
        pass  # Subscribed as part of orderbook

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
        is_smart_depth = command.params.get("is_smart_depth", True)
        await self._client.unsubscribe_order_book(
            instrument_id=command.instrument_id,
            is_smart_depth=is_smart_depth,
        )

    async def _unsubscribe_order_book_snapshots(self, command: UnsubscribeOrderBook) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_unsubscribe_order_book_snapshots` coroutine",  # pragma: no cover
        )

    async def _unsubscribe_quote_ticks(self, command: UnsubscribeQuoteTicks) -> None:
        await self._client.unsubscribe_ticks(command.instrument_id, "BidAsk")

    async def _unsubscribe_trade_ticks(self, command: UnsubscribeTradeTicks) -> None:
        await self._client.unsubscribe_ticks(command.instrument_id, "AllLast")

    async def _unsubscribe_bars(self, command: UnsubscribeBars) -> None:
        if command.bar_type.spec.timedelta == 5:
            await self._client.unsubscribe_realtime_bars(command.bar_type)
        else:
            await self._client.unsubscribe_historical_bars(command.bar_type)

    async def _unsubscribe_instrument_status(self, command: UnsubscribeInstrumentStatus) -> None:
        pass  # Subscribed as part of orderbook

    async def _unsubscribe_instrument_close(self, command: UnsubscribeInstrumentClose) -> None:
        pass  # Subscribed as part of orderbook

    async def _request(self, request: RequestData) -> None:
        raise NotImplementedError(  # pragma: no cover
            "implement the `_request` coroutine",  # pragma: no cover
        )

    async def _request_instrument(self, request: RequestInstrument) -> None:
        if request.start is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `start` which has no effect",
            )

        if request.end is not None:
            self._log.warning(
                f"Requesting instrument {request.instrument_id} with specified `end` which has no effect",
            )

        force_instrument_update = request.params.get("force_instrument_update", False)
        await self.instrument_provider.load_with_return_async(
            request.instrument_id,
            force_instrument_update=force_instrument_update,
        )

        if instrument := self.instrument_provider.find(request.instrument_id):
            self._handle_data(instrument)
        else:
            self._log.warning(f"Instrument for {request.instrument_id} not available")
            return

        self._handle_instrument(instrument, request.id, request.start, request.end, request.params)

    async def _request_instruments(self, request: RequestInstruments) -> None:
        force_instrument_update = request.params.get("force_instrument_update", False)
        loaded_instrument_ids: list[InstrumentId] = []

        if "ib_contracts" in request.params:
            # We allow to pass IBContract parameters to build futures or option chains
            ib_contracts = [IBContract(**d) for d in request.params["ib_contracts"]]
            loaded_instrument_ids = await self.instrument_provider.load_ids_with_return_async(
                ib_contracts,
                force_instrument_update=force_instrument_update,
            )
            loaded_instruments: list[Instrument] = []

            if loaded_instrument_ids:
                for instrument_id in loaded_instrument_ids:
                    instrument = self._cache.instrument(instrument_id)

                    if instrument:
                        loaded_instruments.append(instrument)
                    else:
                        self._log.warning(
                            f"Instrument {instrument_id} not found in cache after loading",
                        )
            else:
                self._log.warning("No instrument IDs were returned from load_ids_async")

            self._handle_instruments(
                venue=request.venue,
                instruments=loaded_instruments,
                correlation_id=request.id,
                start=request.start,
                end=request.end,
                params=request.params,
            )
            return

        # We ensure existing instruments in the cache have their IB representations loaded as well in the adapter
        instruments = self._cache.instruments()
        instrument_ids = [instrument.id for instrument in instruments]
        loaded_instrument_ids = await self.instrument_provider.load_ids_with_return_async(
            instrument_ids,
            force_instrument_update=force_instrument_update,
        )
        self._handle_instruments(
            venue=request.venue,
            instruments=[],
            correlation_id=request.id,
            start=request.start,
            end=request.end,
            params=request.params,
        )

    async def _request_quote_ticks(self, request: RequestQuoteTicks) -> None:
        if not (instrument := self._cache.instrument(request.instrument_id)):
            self._log.error(
                f"Cannot request quotes for {request.instrument_id}, instrument not found",
            )
            return

        end = request.end if request.end else pd.Timestamp.utcnow()

        ticks = await self.get_historical_ticks_paged(
            instrument_id=request.instrument_id,
            contract=IBContract(**instrument.info["contract"]),
            tick_type="BID_ASK",
            start_date_time=request.start,
            end_date_time=end,
            limit=request.limit,
            use_rth=self._use_regular_trading_hours,
            timeout=self._request_timeout,
        )
        if not ticks:
            self._log.warning(f"No quote tick data received for {request.instrument_id}")
            return

        self._handle_quote_ticks(
            request.instrument_id,
            ticks,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def _request_trade_ticks(self, request: RequestTradeTicks) -> None:
        if not (instrument := self._cache.instrument(request.instrument_id)):
            self._log.error(
                f"Cannot request trades for {request.instrument_id}: instrument not found",
            )
            return

        if isinstance(instrument, CurrencyPair):
            self._log.error(
                "Interactive Brokers does not support trades for CurrencyPair instruments",
            )
            return

        end = request.end if request.end else pd.Timestamp.utcnow()

        ticks = await self.get_historical_ticks_paged(
            instrument_id=request.instrument_id,
            contract=IBContract(**instrument.info["contract"]),
            tick_type="TRADES",
            start_date_time=request.start,
            end_date_time=end,
            limit=request.limit,
            use_rth=self._use_regular_trading_hours,
            timeout=self._request_timeout,
        )
        if not ticks:
            self._log.warning(f"No trades received for {request.instrument_id}")
            return

        self._handle_trade_ticks(
            request.instrument_id,
            ticks,
            request.id,
            request.start,
            request.end,
            request.params,
        )

    async def get_historical_ticks_paged(
        self,
        instrument_id: InstrumentId,
        contract: IBContract,
        tick_type: str,
        start_date_time: pd.Timestamp,
        end_date_time: pd.Timestamp,
        use_rth: bool = True,
        timeout: int = 60,
        limit: int = 0,
    ) -> list[TradeTick | QuoteTick]:
        """
        Retrieve historical ticks using pagination to handle large time ranges.

        This method iterates backward from the end_date_time, requesting batches of ticks
        until the start_date_time is reached or the limit is satisfied.

        When both a time range and limit are specified, the method will stop when either
        the start_date_time is reached or the limit is satisfied, whichever comes first.
        If a limit is specified without a start_date_time boundary, pagination will
        continue until the limit is reached or no more data is available.

        Parameters
        ----------
        instrument_id : InstrumentId
            The identifier of the instrument for which to retrieve ticks.
        contract : IBContract
            The Interactive Brokers contract details for the instrument.
        tick_type : str
            The type of ticks to retrieve ("TRADES" or "BID_ASK").
        start_date_time : pd.Timestamp
            The start date time for the ticks.
        end_date_time : pd.Timestamp
            The end date time for the ticks.
        limit : int, default 0
            Maximum number of ticks to retrieve. If 0, no limit is applied.
        use_rth : bool, default True
            Whether to use regular trading hours.
        timeout : int, default 60
             The timeout (seconds) for each individual request.

        Returns
        -------
        list[TradeTick | QuoteTick]
            A list of aggregated ticks sorted by initialization timestamp, filtered to
            the requested time range and limited to the specified count if provided.

        """
        data: list[TradeTick | QuoteTick] = []

        # Ensure UTC
        start_date_time = time_object_to_dt(start_date_time)
        current_end_date_time = time_object_to_dt(end_date_time)
        start_date_time_nanos = dt_to_unix_nanos(start_date_time)
        end_date_time_nanos = dt_to_unix_nanos(end_date_time)

        # Use 1 millisecond decrement to avoid duplicate/skipped ticks in high-frequency data
        TIMESTAMP_DECREMENT_NS = 1_000_000

        await self._client.wait_until_ready()

        while current_end_date_time > start_date_time and (limit == 0 or len(data) < limit):
            self._log.info(
                f"{instrument_id}: Requesting {tick_type} ticks ending at {current_end_date_time}",
            )

            ticks = await self._client.get_historical_ticks(
                instrument_id=instrument_id,
                contract=contract,
                tick_type=tick_type,
                end_date_time=current_end_date_time,
                use_rth=use_rth,
                timeout=timeout,
            )

            # Break early if no ticks returned (reached beginning of available data)
            if not ticks:
                break

            self._log.info(
                f"{instrument_id}: Number of {tick_type} ticks retrieved in batch: {len(ticks)}",
            )

            # Filter ticks to ensure they're within the requested time range
            # When iterating backward, filter ticks before start_date_time
            filtered_ticks = [
                tick
                for tick in ticks
                if start_date_time_nanos <= tick.ts_init <= end_date_time_nanos
            ]

            if not filtered_ticks:
                # No ticks in this batch are within range, break to avoid infinite loop
                break

            # Find minimum timestamp from filtered ticks
            min_timestamp_nanos = min(tick.ts_init for tick in filtered_ticks)

            # Update end_date_time to 1ms before the minimum timestamp to avoid duplicates
            current_end_date_time = unix_nanos_to_dt(min_timestamp_nanos - TIMESTAMP_DECREMENT_NS)

            data.extend(filtered_ticks)
            self._log.info(f"Total number of {tick_type} ticks in data: {len(data)}")

            # Break early if limit is reached
            if limit > 0 and len(data) >= limit:
                break

        sorted_data = sorted(data, key=lambda x: x.ts_init)

        # Apply limit if specified (trim to most recent ticks)
        if limit > 0 and len(sorted_data) > limit:
            sorted_data = sorted_data[-limit:]

        return sorted_data

    async def _request_bars(self, request: RequestBars) -> None:
        contract = self.instrument_provider.contract.get(request.bar_type.instrument_id)
        if not contract:
            self._log.error(f"Cannot request {request.bar_type} bars: instrument not found")
            return

        if not request.bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {request.bar_type} bars: only time bars are aggregated by Interactive Brokers",
            )
            return

        duration = request.end - request.start
        duration_str = timedelta_to_duration_str(duration)
        bars = await self.get_historical_bars_chunked(
            bar_type=request.bar_type,
            contract=contract,
            start_date_time=request.start,
            end_date_time=request.end,
            duration=duration_str,
            use_rth=self._use_regular_trading_hours,
            timeout=self._request_timeout,
        )

        if bars:
            bars = list(set(bars))
            bars.sort(key=lambda x: x.ts_init)

            # Apply limit if specified
            limit = request.limit
            if limit > 0 and len(bars) > limit:
                bars = bars[-limit:]

            self._handle_bars(
                request.bar_type,
                bars,
                request.id,
                request.start,
                request.end,
                request.params,
            )
            status_msg = {"id": request.id, "status": "Success"}
        else:
            self._log.warning(f"No bar data received for {request.bar_type}")
            status_msg = {"id": request.id, "status": "Failed"}

        # Publish Status event
        self._msgbus.publish(
            topic=f"requests.{request.id}",
            msg=status_msg,
        )

    async def get_historical_bars_chunked(
        self,
        bar_type: BarType,
        contract: IBContract,
        start_date_time: pd.Timestamp | None = None,
        end_date_time: pd.Timestamp | None = None,
        duration: str | None = None,
        use_rth: bool = True,
        timeout: int = 60,
    ) -> list[Bar]:
        """
        Retrieve historical bars in chunks to handle large duration requests.

        This method breaks down a large historical data request into smaller segments
        (years, days, seconds) to comply with IB API limits and avoid timeouts. It iterates
        through these segments and aggregates the results.

        Parameters
        ----------
        bar_type : BarType
            The type of bar to retrieve.
        contract : IBContract
             The Interactive Brokers contract details for the instrument.
        start_date_time : datetime.datetime
             The start date time for the bars. If provided, duration is derived.
        end_date_time : datetime.datetime
             The end date time for the bars.
        duration : str
             The amount of time to go back from the end_date_time.
        use_rth : bool, default True
             Whether to use regular trading hours.
        timeout : int, default 60
             The timeout (seconds) for each individual request segment.

        Returns
        -------
        list[Bar]
             A list of aggregated Bar objects sorted by initialization timestamp.

        """
        # Adjust start and end time based on the timezone
        if start_date_time:
            start_date_time = time_object_to_dt(start_date_time)

        if end_date_time:
            end_date_time = time_object_to_dt(end_date_time)

        data: list[Bar] = []

        # We need to calculate duration segments based on start/end or duration
        segments = self._calculate_duration_segments(
            start_date_time,
            end_date_time,
            duration,
        )

        for segment_end_date_time, segment_duration in segments:
            self._log.info(
                f"{bar_type.instrument_id}: Requesting historical bars: {bar_type} ending on '{segment_end_date_time}' "
                f"with duration '{segment_duration}'",
            )

            bars = await self._client.get_historical_bars( # Changed self.get_historical_bars to self._client.get_historical_bars
                bar_type,
                contract,
                use_rth,
                segment_end_date_time,
                segment_duration,
                timeout=timeout,
            )
            if bars:
                self._log.info(
                    f"{bar_type.instrument_id}: Number of bars retrieved in batch: {len(bars)}",
                )
                data.extend(bars)
                self._log.info(f"Total number of bars in data: {len(data)}")
            else:
                self._log.info(f"{bar_type.instrument_id}: No bars retrieved for: {bar_type}")

        return sorted(data, key=lambda x: x.ts_init)

    def _calculate_duration_segments(
        self,
        start_date: pd.Timestamp | None,
        end_date: pd.Timestamp,
        duration: str | None,
    ) -> list[tuple[pd.Timestamp, str]]:
        # Calculate the difference in years, days, and seconds between two dates for the
        # purpose of requesting specific date ranges for historical bars.
        #
        # This function breaks down the time difference between two provided dates (start_date
        # and end_date) into separate components: years, days, and seconds. It accounts for leap
        # years in its calculation of years and considers detailed time components (hours, minutes,
        # seconds) for precise calculation of seconds.
        #
        # Each component of the time difference (years, days, seconds) is represented as a
        # tuple in the returned list.
        # The first element is the date that indicates the end point of that time segment
        # when moving from start_date to end_date. For example, if the function calculates 1
        # year, the date for the year entry will be the end date after 1 year has passed
        # from start_date. This helps in understanding the progression of time from start_date
        # to end_date in segmented intervals.

        if duration:
            return [(end_date, duration)]

        total_delta = end_date - start_date

        # Calculate full years in the time delta
        years = total_delta.days // 365
        minus_years_date = end_date - pd.Timedelta(days=365 * years)

        # Calculate remaining days after subtracting full years
        days = (minus_years_date - start_date).days
        minus_days_date = minus_years_date - pd.Timedelta(days=days)

        # Calculate remaining time in seconds
        delta = minus_days_date - start_date
        subsecond = (
            1
            if delta.components.milliseconds > 0
               or delta.components.microseconds > 0
               or delta.components.nanoseconds > 0
            else 0
        )
        seconds = (
            delta.components.hours * 3600
            + delta.components.minutes * 60
            + delta.components.seconds
            + subsecond
        )

        results = []

        if years:
            results.append((end_date, f"{years} Y"))

        if days:
            results.append((minus_years_date, f"{days} D"))

        if seconds:
            results.append((minus_days_date, f"{seconds} S"))

        return results
