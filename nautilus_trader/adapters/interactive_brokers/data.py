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
from typing import Dict, Optional

import ib_insync
from ib_insync import ContractDetails
from ib_insync import Ticker

from nautilus_trader.adapters.interactive_brokers.common import IB_VENUE
from nautilus_trader.adapters.interactive_brokers.common import ContractId
from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.live.data_client import LiveMarketDataClient
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
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
            instrument_provider=instrument_provider,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config={"name": "InteractiveBrokersDataClient"},
        )
        self._client = client
        self._tickers: Dict[ContractId, Ticker] = {}
        self.instrument_provider = instrument_provider

    def connect(self):
        """
        Connect the client to InteractiveBrokers.
        """
        self._log.info("Connecting...")
        self._loop.create_task(self._connect())

    async def _connect(self):
        # Connect client
        if not self._client.isConnected():
            await self._client.connect()
        try:
            await self._instrument_provider.load_all_or_wait_async()
        except Exception as ex:
            self._log.exception(ex)
            return
        for instrument in self._instrument_provider.get_all().values():
            self._handle_data(instrument)
        self._set_connected(True)
        self._log.info("Connected.")

    def disconnect(self):
        """
        Disconnect the client from Binance.
        """
        self._log.info("Disconnecting...")
        self._loop.create_task(self._disconnect())

    async def _disconnect(self):
        # Disconnect clients
        if self._client.isConnected():
            await self._client.disconnect()

        self._set_connected(False)
        self._log.info("Disconnected.")

    def subscribe_order_book_deltas(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
        depth: Optional[int] = 0,
        kwargs=None,
    ):
        """
        Subscribe to `OrderBook` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        depth : int, optional, default None
            The maximum depth for the subscription.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        if book_type == BookType.L1_TBBO:
            self._subscribe_order_book_deltas_L1(instrument_id=instrument_id)
        elif book_type == BookType.L2_MBP:
            self._subscribe_order_book_deltas_L2(instrument_id=instrument_id, depth=depth)

    def _subscribe_order_book_deltas_L1(self, instrument_id: InstrumentId):
        raise NotImplementedError

    def _subscribe_order_book_deltas_L2(self, instrument_id: InstrumentId, depth: int):
        contract_details: ContractDetails = self.instrument_provider.contract_details[instrument_id]
        ticker = self._client.reqMktDepth(
            contract=contract_details.contract,
            numRows=depth,
        )
        ticker.updateEvent += self._on_ticker_update
        self._tickers[ContractId(ticker.contract.conId)] = ticker

    def subscribe_trade_ticks(self, instrument_id: InstrumentId):
        contract_details: ContractDetails = self.instrument_provider.contract_details[instrument_id]
        ticker = self._client.reqMktData(
            contract=contract_details.contract,
        )
        ticker.updateEvent += self._on_ticker_update
        self._tickers[ContractId(ticker.contract.conId)] = ticker

    def _on_ticker_update(self, ticker: Ticker):
        pass
