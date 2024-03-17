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

import asyncio
from operator import attrgetter
from typing import Any

import pandas as pd

# fmt: off
from nautilus_trader.adapters.interactive_brokers.client import InteractiveBrokersClient
from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.parsing.data import timedelta_to_duration_str
from nautilus_trader.adapters.interactive_brokers.providers import InteractiveBrokersInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.currency_pair import CurrencyPair


# fmt: on


class InteractiveBrokersDataClient(LiveMarketDataClient):
    """
    Provides a data client for the InteractiveBrokers exchange by using the `Gateway` to
    stream market data.
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
    ) -> None:
        """
        Initialize a new instance of the ``InteractiveBrokersDataClient`` class.

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

        """
        super().__init__(
            loop=loop,
            client_id=ClientId(f"{IB_VENUE.value}-{ibg_client_id:03d}"),
            venue=None,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            instrument_provider=instrument_provider,
            config=config,
        )
        self._client = client
        self._handle_revised_bars = config.handle_revised_bars
        self._use_regular_trading_hours = config.use_regular_trading_hours
        self._market_data_type = config.market_data_type

    @property
    def instrument_provider(self) -> InteractiveBrokersInstrumentProvider:
        return self._instrument_provider  # type: ignore

    async def _connect(self):
        # Connect client
        await self._client.wait_until_ready()
        self._client.registered_nautilus_clients.add(self.id)

        # Set Market Data Type
        await self._client.set_market_data_type(self._market_data_type)

        # Load instruments based on config
        await self.instrument_provider.initialize()
        for instrument in self._instrument_provider.list_all():
            self._handle_data(instrument)

    async def _disconnect(self):
        self._client.registered_nautilus_clients.remove(self.id)
        if self._client.is_running and self._client.registered_nautilus_clients == set():
            self._client.stop()

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
        if not (instrument := self._cache.instrument(instrument_id)):
            self._log.error(
                f"Cannot subscribe to QuoteTicks for {instrument_id}, Instrument not found.",
            )
            return

        await self._client.subscribe_ticks(
            instrument_id=instrument_id,
            contract=IBContract(**instrument.info["contract"]),
            tick_type="BidAsk",
        )

    async def _subscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        if not (instrument := self._cache.instrument(instrument_id)):
            self._log.error(
                f"Cannot subscribe to TradeTicks for {instrument_id}, Instrument not found.",
            )
            return

        if isinstance(instrument, CurrencyPair):
            self._log.error(
                "InteractiveBrokers doesn't support Trade Ticks for CurrencyPair.",
            )
            return

        await self._client.subscribe_ticks(
            instrument_id=instrument_id,
            contract=IBContract(**instrument.info["contract"]),
            tick_type="AllLast",
        )

    async def _subscribe_bars(self, bar_type: BarType) -> None:
        if not (instrument := self._cache.instrument(bar_type.instrument_id)):
            self._log.error(f"Cannot subscribe to {bar_type}, Instrument not found.")
            return

        if bar_type.spec.timedelta.total_seconds() == 5:
            await self._client.subscribe_realtime_bars(
                bar_type=bar_type,
                contract=IBContract(**instrument.info["contract"]),
                use_rth=self._use_regular_trading_hours,
            )
        else:
            await self._client.subscribe_historical_bars(
                bar_type=bar_type,
                contract=IBContract(**instrument.info["contract"]),
                use_rth=self._use_regular_trading_hours,
                handle_revised_bars=self._handle_revised_bars,
            )

    async def _subscribe_instrument_status(self, instrument_id: InstrumentId) -> None:
        pass  # Subscribed as part of orderbook

    async def _subscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        pass  # Subscribed as part of orderbook

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
        await self._client.unsubscribe_ticks(instrument_id, "BidAsk")

    async def _unsubscribe_trade_ticks(self, instrument_id: InstrumentId) -> None:
        await self._client.unsubscribe_ticks(instrument_id, "AllLast")

    async def _unsubscribe_bars(self, bar_type: BarType) -> None:
        if bar_type.spec.timedelta == 5:
            await self._client.unsubscribe_realtime_bars(bar_type)
        else:
            await self._client.unsubscribe_historical_bars(bar_type)

    async def _unsubscribe_instrument_status(self, instrument_id: InstrumentId) -> None:
        pass  # Subscribed as part of orderbook

    async def _unsubscribe_instrument_close(self, instrument_id: InstrumentId) -> None:
        pass  # Subscribed as part of orderbook

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
        if start is not None:
            self._log.warning(
                f"Requesting instrument {instrument_id} with specified `start` which has no effect.",
            )

        if end is not None:
            self._log.warning(
                f"Requesting instrument {instrument_id} with specified `end` which has no effect.",
            )

        await self.instrument_provider.load_async(instrument_id)
        if instrument := self.instrument_provider.find(instrument_id):
            self._handle_data(instrument)
        else:
            self._log.warning(f"{instrument_id} not available.")
            return
        self._handle_instrument(instrument, correlation_id)

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
        if not (instrument := self._cache.instrument(instrument_id)):
            self._log.error(
                f"Cannot request QuoteTicks for {instrument_id}, Instrument not found.",
            )
            return

        ticks = await self._handle_ticks_request(
            IBContract(**instrument.info["contract"]),
            "BID_ASK",
            limit,
            start,
            end,
        )
        if not ticks:
            self._log.warning(f"QuoteTicks not received for {instrument_id}")
            return

        self._handle_quote_ticks(instrument_id, ticks, correlation_id)

    async def _request_trade_ticks(
        self,
        instrument_id: InstrumentId,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        if not (instrument := self._cache.instrument(instrument_id)):
            self._log.error(
                f"Cannot request TradeTicks for {instrument_id}, Instrument not found.",
            )
            return

        if isinstance(instrument, CurrencyPair):
            self._log.error(
                "InteractiveBrokers doesn't support Trade Ticks for CurrencyPair.",
            )
            return

        ticks = await self._handle_ticks_request(
            IBContract(**instrument.info["contract"]),
            "TRADES",
            limit,
            start,
            end,
        )
        if not ticks:
            self._log.warning(f"TradeTicks not received for {instrument_id}")
            return

        self._handle_trade_ticks(instrument_id, ticks, correlation_id)

    async def _handle_ticks_request(
        self,
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
                contract,
                tick_type,
                end_date_time=end,
                use_rth=self._use_regular_trading_hours,
            )
            if not ticks_part:
                break
            end = pd.Timestamp(min(ticks_part, key=attrgetter("ts_init")).ts_init, tz="UTC")
            ticks.extend(ticks_part)

        ticks.sort(key=lambda x: x.ts_init)
        return ticks

    async def _request_bars(
        self,
        bar_type: BarType,
        limit: int,
        correlation_id: UUID4,
        start: pd.Timestamp | None = None,
        end: pd.Timestamp | None = None,
    ) -> None:
        if not (instrument := self._cache.instrument(bar_type.instrument_id)):
            self._log.error(
                f"Cannot request {bar_type}, Instrument not found.",
            )
            return

        if not bar_type.spec.is_time_aggregated():
            self._log.error(
                f"Cannot request {bar_type}: only time bars are aggregated by InteractiveBrokers.",
            )
            return

        if not start and limit == 0:
            limit = 1000

        if not end:
            end = pd.Timestamp.utcnow()

        if start:
            duration = end - start
            duration_str = timedelta_to_duration_str(duration)
        else:
            duration_str = "7 D" if bar_type.spec.timedelta.total_seconds() >= 60 else "1 D"

        bars: list[Bar] = []
        while (start and end > start) or (len(bars) < limit > 0):
            bars_part: list[Bar] = await self._client.get_historical_bars(
                bar_type=bar_type,
                contract=IBContract(**instrument.info["contract"]),
                use_rth=self._use_regular_trading_hours,
                end_date_time=end.strftime("%Y%m%d %H:%M:%S %Z"),
                duration=duration_str,
            )
            bars.extend(bars_part)
            if not bars_part or start:
                break
            end = pd.Timestamp(min(bars, key=attrgetter("ts_event")).ts_event, tz="UTC")

        if bars:
            bars = list(set(bars))
            bars.sort(key=lambda x: x.ts_init)
            self._handle_bars(bar_type, bars, bars[0], correlation_id)
            status_msg = {"id": correlation_id, "status": "Success"}
        else:
            self._log.warning(f"Bar Data not received for {bar_type}")
            status_msg = {"id": correlation_id, "status": "Failed"}

        # Publish Status event
        self._msgbus.publish(
            topic=f"requests.{correlation_id}",
            msg=status_msg,
        )
