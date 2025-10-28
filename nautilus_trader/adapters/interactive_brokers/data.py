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
from operator import attrgetter

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

        batch_quotes = command.params.get("batch_quotes", False)

        if contract.secType == "BAG" or batch_quotes:
            # For OptionSpread (BAG) instruments, use reqMktData instead of reqTickByTickData
            # because reqTickByTickData doesn't support BAG contracts
            await self._client.subscribe_market_data(
                instrument_id=command.instrument_id,
                contract=contract,
                generic_tick_list="",  # Empty for basic bid/ask data
            )
        else:
            await self._client.subscribe_market_data(
                instrument_id=command.instrument_id,
                contract=contract,
                generic_tick_list="",  # Empty for basic bid/ask data
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

        ticks = await self._handle_ticks_request(
            request.instrument_id,
            IBContract(**instrument.info["contract"]),
            "BID_ASK",
            request.limit,
            request.start,
            request.end,
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

        ticks = await self._handle_ticks_request(
            request.instrument_id,
            IBContract(**instrument.info["contract"]),
            "TRADES",
            request.limit,
            request.start,
            request.end,
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

    async def _handle_ticks_request(
        self,
        instrument_id: InstrumentId,
        contract: IBContract,
        tick_type: str,
        limit: int,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> list[QuoteTick | TradeTick]:
        if not start:
            limit = self._cache.tick_capacity

        if not end:
            end = pd.Timestamp.utcnow()

        ticks: list[QuoteTick | TradeTick] = []

        while (start and end > start) or (len(ticks) < limit > 0):
            await self._client.wait_until_ready()
            ticks_part = await self._client.get_historical_ticks(
                instrument_id,
                contract,
                tick_type,
                end_date_time=end,
                use_rth=self._use_regular_trading_hours,
                timeout=self._request_timeout,
            )

            if not ticks_part:
                break

            end = pd.Timestamp(min(ticks_part, key=attrgetter("ts_init")).ts_init, tz="UTC")
            ticks.extend(ticks_part)

        ticks.sort(key=lambda x: x.ts_init)

        return ticks

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
        bars: list[Bar] = []
        bars = await self._client.get_historical_bars(
            bar_type=request.bar_type,
            contract=contract,
            use_rth=self._use_regular_trading_hours,
            end_date_time=request.end,
            duration=duration_str,
            timeout=self._request_timeout,
        )

        if bars:
            bars = list(set(bars))
            bars.sort(key=lambda x: x.ts_init)
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
