# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from functools import partial
from typing import Callable, Dict, List, Optional

import ib_insync
import pandas as pd
from ib_insync import Contract
from ib_insync import ContractDetails
from ib_insync import RealTimeBar
from ib_insync import RealTimeBarList
from ib_insync import Ticker
from ib_insync.ticker import nan

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.common import ContractId
from nautilus_trader.adapters.interactive_brokers.parsing.data import generate_trade_id
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import defaultdict
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.msgbus.bus import MessageBus


class InteractiveBrokersDataClient(LiveMarketDataClient):
    """
    Provides a data client for the InteractiveBrokers exchange.
    """

    def __init__(
        self,
        loop: asyncio.AbstractEventLoop,
        client: ib_insync.IB,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
        logger: Logger,
        instrument_provider: InteractiveBrokersInstrumentProvider,
    ):
        """
        Initialize a new instance of the ``InteractiveBrokersDataClient`` class.

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        client : IB
            The ib_insync IB client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.
        instrument_provider : InteractiveBrokersInstrumentProvider
            The instrument provider.

        """
        super().__init__(
            loop=loop,
            client_id=ClientId(IB_VENUE.value),
            venue=None,
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config={"name": "InteractiveBrokersDataClient"},
        )

        self._client = client
        self._tickers: Dict[ContractId, List[Ticker]] = defaultdict(list)
        self._last_bar_time: pd.Timestamp = pd.Timestamp("1970-01-01", tz="UTC")

    @property
    def instrument_provider(self) -> InteractiveBrokersInstrumentProvider:
        return self._instrument_provider  # type: ignore

    def connect(self):
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        # Connect client
        if not self._client.isConnected():
            await self._client.connect()

        # Load instruments based on config
        # try:
        await self.instrument_provider.initialize()
        # except Exception as e:
        #     self._log.exception(e)
        #     return
        for instrument in self.instrument_provider.get_all().values():
            self._handle_data(instrument)
        self._set_connected(True)
        self._log.info("Connected.")

    def disconnect(self):
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        # Disconnect clients
        if self._client.isConnected():
            self._client.disconnect()

        self._set_connected(False)
        self._log.info("Disconnected.")

    def subscribe_order_book_snapshots(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int = 5,
        kwargs: Optional[Dict] = None,
    ):
        if book_type == BookType.L1_TBBO:
            return self._request_top_of_book(instrument_id=instrument_id)
        elif book_type == BookType.L2_MBP:
            if depth == 0:
                depth = (
                    5  # depth = 0 is default for Nautilus, but not handled by Interactive Brokers
                )
            return self._request_market_depth(
                instrument_id=instrument_id,
                handler=self._on_order_book_snapshot,
                depth=depth,
            )
        else:
            raise NotImplementedError("L3 orderbook not available for Interactive Brokers")

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: int = 5,
        kwargs: Optional[Dict] = None,
    ):
        raise NotImplementedError("Orderbook deltas not implemented for Interactive Brokers (yet)")

    def subscribe_trade_ticks(self, instrument_id: InstrumentId):
        contract_details: ContractDetails = self.instrument_provider.contract_details[
            instrument_id.value
        ]
        ticker = self._client.reqMktData(
            contract=contract_details.contract,
        )
        ticker.updateEvent += self._on_trade_ticker_update
        self._tickers[ContractId(ticker.contract.conId)].append(ticker)

    def subscribe_quote_ticks(self, instrument_id: InstrumentId):
        contract_details: ContractDetails = self.instrument_provider.contract_details[
            instrument_id.value
        ]
        ticker = self._client.reqMktData(
            contract=contract_details.contract,
        )
        ticker.updateEvent += partial(
            self._on_quote_tick_update, contract=contract_details.contract
        )
        self._tickers[ContractId(ticker.contract.conId)].append(ticker)

    def subscribe_bars(self, bar_type: BarType):
        price_type: PriceType = bar_type.spec.price_type
        contract_details: ContractDetails = self.instrument_provider.contract_details[
            bar_type.instrument_id.value
        ]

        what_to_show = {
            PriceType.ASK: "ASK",
            PriceType.BID: "BID",
            PriceType.LAST: "TRADES",
            PriceType.MID: "MIDPOINT",
        }

        bar_list: RealTimeBarList = self._client.reqRealTimeBars(
            contract=contract_details.contract,
            barSize=5,
            whatToShow=what_to_show[price_type],
            useRTH=False,
        )

        bar_list.updateEvent += partial(self._on_bar_update, bar_type=bar_type)

    def _request_top_of_book(self, instrument_id: InstrumentId):
        contract_details: ContractDetails = self.instrument_provider.contract_details[
            instrument_id.value
        ]
        ticker = self._client.reqTickByTickData(
            contract=contract_details.contract,
            tickType="BidAsk",
        )
        ticker.updateEvent += self._on_top_level_snapshot
        self._tickers[ContractId(ticker.contract.conId)].append(ticker)

    def _request_market_depth(self, instrument_id: InstrumentId, handler: Callable, depth: int = 5):
        contract_details: ContractDetails = self.instrument_provider.contract_details[
            instrument_id.value
        ]
        ticker = self._client.reqMktDepth(
            contract=contract_details.contract,
            numRows=depth,
        )
        ticker.updateEvent += handler
        self._tickers[ContractId(ticker.contract.conId)].append(ticker)

    # def _on_order_book_delta(self, ticker: Ticker):
    #     instrument_id = self.instrument_provider.contract_id_to_instrument_id[
    #         ticker.contract.conId
    #     ]
    #     for depth in ticker.domTicks:
    #         update = OrderBookDelta(
    #             instrument_id=instrument_id,
    #             book_type=BookType.L2_MBP,
    #             action=MKT_DEPTH_OPERATIONS[depth.operation],
    #             order=Order(
    #                 price=Price.from_str(str(depth.price)),
    #                 size=Quantity.from_str(str(depth.size)),
    #                 side=IB_SIDE[depth.side],
    #             ),
    #             ts_event=dt_to_unix_nanos(depth.time),
    #             ts_init=self._clock.timestamp_ns(),
    #         )
    #         self._handle_data(update)

    def _on_quote_tick_update(self, tick: Ticker, contract: Contract):
        instrument_id = self.instrument_provider.contract_id_to_instrument_id[contract.conId]
        ts_init = self._clock.timestamp_ns()
        ts_event = min(dt_to_unix_nanos(tick.time), ts_init)
        quote_tick = QuoteTick(
            instrument_id=instrument_id,
            bid=Price.from_str(str(tick.bid) if tick.bid not in (None, nan) else "0"),
            bid_size=Quantity.from_str(
                str(tick.bidSize) if tick.bidSize not in (None, nan) else "0"
            ),
            ask=Price.from_str(str(tick.ask) if tick.ask not in (None, nan) else "0"),
            ask_size=Quantity.from_str(
                str(tick.askSize) if tick.askSize not in (None, nan) else "0"
            ),
            ts_event=ts_event,
            ts_init=ts_init,
        )
        self._handle_data(quote_tick)

    def _on_top_level_snapshot(self, ticker: Ticker):
        instrument_id = self.instrument_provider.contract_id_to_instrument_id[ticker.contract.conId]
        ts_init = self._clock.timestamp_ns()
        ts_event = min(dt_to_unix_nanos(ticker.time), ts_init)
        snapshot = OrderBookSnapshot(
            book_type=BookType.L1_TBBO,
            instrument_id=instrument_id,
            bids=[(ticker.bid, ticker.bidSize)],
            asks=[(ticker.ask, ticker.askSize)],
            ts_event=ts_event,
            ts_init=ts_init,
        )
        self._handle_data(snapshot)

    def _on_order_book_snapshot(self, ticker: Ticker, book_type: BookType = BookType.L2_MBP):
        instrument_id = self.instrument_provider.contract_id_to_instrument_id[ticker.contract.conId]
        ts_init = self._clock.timestamp_ns()
        ts_event = min(dt_to_unix_nanos(ticker.time), ts_init)
        if not (ticker.domBids or ticker.domAsks):
            return
        snapshot = OrderBookSnapshot(
            book_type=book_type,
            instrument_id=instrument_id,
            bids=[(level.price, level.size) for level in ticker.domBids],
            asks=[(level.price, level.size) for level in ticker.domAsks],
            ts_event=ts_event,
            ts_init=ts_init,
        )
        self._handle_data(snapshot)

    def _on_trade_ticker_update(self, ticker: Ticker):
        instrument_id = self.instrument_provider.contract_id_to_instrument_id[ticker.contract.conId]
        for tick in ticker.ticks:
            price = str(tick.price)
            size = str(tick.size)
            ts_init = self._clock.timestamp_ns()
            ts_event = min(dt_to_unix_nanos(tick.time), ts_init)
            update = TradeTick(
                instrument_id=instrument_id,
                price=Price.from_str(price),
                size=Quantity.from_str(size),
                aggressor_side=AggressorSide.UNKNOWN,
                trade_id=generate_trade_id(ts_event=ts_event, price=price, size=size),
                ts_event=ts_event,
                ts_init=ts_init,
            )
            self._handle_data(update)

    def _on_bar_update(
        self,
        bars: List[RealTimeBar],
        hasNewBar: bool,
        bar_type: BarType,
    ):

        if not hasNewBar:
            return

        for bar in bars:
            if bar.time <= self._last_bar_time:
                continue
            instrument = self._cache.instrument(bar_type.instrument_id)
            ts_init = self._clock.timestamp_ns()
            ts_event = min(dt_to_unix_nanos(bar.time), ts_init)
            data = Bar(
                bar_type=bar_type,
                open=Price(bar.open_, instrument.price_precision),
                high=Price(bar.high, instrument.price_precision),
                low=Price(bar.low, instrument.price_precision),
                close=Price(bar.close, instrument.price_precision),
                volume=Quantity(bar.volume, instrument.size_precision),
                ts_event=ts_event,
                ts_init=ts_init,
            )
            self._handle_data(data)
            self._last_bar_time = bar.time
