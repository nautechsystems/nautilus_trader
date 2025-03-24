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
This module provides a data client for backtesting.
"""

from nautilus_trader.common.config import NautilusConfig

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.data.messages cimport RequestBars
from nautilus_trader.data.messages cimport RequestData
from nautilus_trader.data.messages cimport RequestInstrument
from nautilus_trader.data.messages cimport RequestInstruments
from nautilus_trader.data.messages cimport RequestOrderBookSnapshot
from nautilus_trader.data.messages cimport RequestQuoteTicks
from nautilus_trader.data.messages cimport RequestTradeTicks
from nautilus_trader.data.messages cimport SubscribeBars
from nautilus_trader.data.messages cimport SubscribeData
from nautilus_trader.data.messages cimport SubscribeIndexPrices
from nautilus_trader.data.messages cimport SubscribeInstrument
from nautilus_trader.data.messages cimport SubscribeInstrumentClose
from nautilus_trader.data.messages cimport SubscribeInstruments
from nautilus_trader.data.messages cimport SubscribeInstrumentStatus
from nautilus_trader.data.messages cimport SubscribeMarkPrices
from nautilus_trader.data.messages cimport SubscribeOrderBook
from nautilus_trader.data.messages cimport SubscribeQuoteTicks
from nautilus_trader.data.messages cimport SubscribeTradeTicks
from nautilus_trader.data.messages cimport UnsubscribeBars
from nautilus_trader.data.messages cimport UnsubscribeData
from nautilus_trader.data.messages cimport UnsubscribeIndexPrices
from nautilus_trader.data.messages cimport UnsubscribeInstrument
from nautilus_trader.data.messages cimport UnsubscribeInstrumentClose
from nautilus_trader.data.messages cimport UnsubscribeInstruments
from nautilus_trader.data.messages cimport UnsubscribeInstrumentStatus
from nautilus_trader.data.messages cimport UnsubscribeMarkPrices
from nautilus_trader.data.messages cimport UnsubscribeOrderBook
from nautilus_trader.data.messages cimport UnsubscribeQuoteTicks
from nautilus_trader.data.messages cimport UnsubscribeTradeTicks
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument


cdef class BacktestDataClient(DataClient):
    """
    Provides an implementation of `DataClient` for backtesting.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    config : NautilusConfig, optional
        The configuration for the instance.
    """

    def __init__(
        self,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        config: NautilusConfig | None = None,
    ) -> None:
        super().__init__(
            client_id=client_id,
            venue=Venue(client_id.to_str()),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self.is_connected = False

    cpdef void _start(self):
        self._log.info(f"Connecting...")
        self.is_connected = True
        self._log.info(f"Connected")

    cpdef void _stop(self):
        self._log.info(f"Disconnecting...")
        self.is_connected = False
        self._log.info(f"Disconnected")

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe(self, SubscribeData command):
        Condition.not_none(command.data_type, "data_type")

        self._add_subscription(command.data_type)
        # Do nothing else for backtest

    cpdef void unsubscribe(self, UnsubscribeData command):
        Condition.not_none(command.data_type, "data_type")

        self._remove_subscription(command.data_type)
        # Do nothing else for backtest

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request(self, RequestData request):
        Condition.not_none(request.data_type, "data_type")
        # Do nothing else for backtest


cdef class BacktestMarketDataClient(MarketDataClient):
    """
    Provides an implementation of `MarketDataClient` for backtesting.

    Parameters
    ----------
    client_id : ClientId
        The data client ID.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : Clock
        The clock for the client.
    """

    def __init__(
        self,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
    ):
        super().__init__(
            client_id=client_id,
            venue=Venue(client_id.to_str()),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )

        self.is_connected = False

    cpdef void _start(self):
        self._log.info(f"Connecting...")
        self.is_connected = True
        self._log.info(f"Connected")

    cpdef void _stop(self):
        self._log.info(f"Disconnecting...")
        self.is_connected = False
        self._log.info(f"Disconnected")

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe_instruments(self, SubscribeInstruments command):
        cdef Instrument instrument
        for instrument in self._cache.instruments(Venue(self.id.value)):
            subscribe = SubscribeInstrument(
                instrument_id=instrument.id,
                client_id=command.client_id,
                venue=command.venue,
                command_id=command.id,
                ts_init=command.ts_init,
                params=command.params,
            )
            self.subscribe_instrument(subscribe)

    cpdef void subscribe_instrument(self, SubscribeInstrument command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `Instrument` data",
            )
            return

        self._add_subscription_instrument(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_deltas(self, SubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `OrderBookDelta` data, "
                "no data has been loaded for this instrument",
            )
            return

        self._add_subscription_order_book_deltas(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_snapshots(self, SubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `OrderBook` data, "
                "no data has been loaded for this instrument.",
            )
            return

        self._add_subscription_order_book_snapshots(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_quote_ticks(self, SubscribeQuoteTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `QuoteTick` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_quote_ticks(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_trade_ticks(self, SubscribeTradeTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `TradeTick` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_trade_ticks(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_mark_prices(self, SubscribeMarkPrices command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `MarkPriceUpdate` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_mark_prices(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_index_prices(self, SubscribeIndexPrices command):
        Condition.not_none(command.instrument_id, "instrument_id")

        if not self._cache.instrument(command.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.instrument_id} to subscribe for `IndexPriceUpdate` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_index_prices(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_bars(self, SubscribeBars command):
        Condition.not_none(command.bar_type, "bar_type")

        if not self._cache.instrument(command.bar_type.instrument_id):
            self._log.error(
                f"Cannot find instrument {command.bar_type.instrument_id} to subscribe for `Bar` data, "
                "No data has been loaded for this instrument",
            )
            return

        self._add_subscription_bars(command.bar_type)
        # Do nothing else for backtest

    cpdef void subscribe_instrument_status(self, SubscribeInstrumentStatus command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._add_subscription_instrument_status(command.instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_instrument_close(self, SubscribeInstrumentClose command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._add_subscription_instrument_close(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_instruments(self, UnsubscribeInstruments command):
        self._subscriptions_instrument.clear()
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument(self, UnsubscribeInstrument command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_instrument(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_deltas(self, UnsubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_order_book_deltas(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_snapshots(self, UnsubscribeOrderBook command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_order_book_snapshots(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_quote_ticks(self, UnsubscribeQuoteTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_quote_ticks(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_trade_ticks(self, UnsubscribeTradeTicks command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_trade_ticks(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_mark_prices(self, UnsubscribeMarkPrices command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_mark_prices(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_index_prices(self, UnsubscribeIndexPrices command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_index_prices(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_bars(self, UnsubscribeBars command):
        Condition.not_none(command.bar_type, "bar_type")

        self._remove_subscription_bars(command.bar_type)
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument_status(self, UnsubscribeInstrumentStatus command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_instrument_status(command.instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument_close(self, UnsubscribeInstrumentClose command):
        Condition.not_none(command.instrument_id, "instrument_id")

        self._remove_subscription_instrument_close(command.instrument_id)
        # Do nothing else for backtest

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request_instrument(self, RequestInstrument request):
        cdef Instrument instrument = self._cache.instrument(request.instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {request.instrument_id}")
            return

        self._handle_instrument(instrument, request.id, request.params)

    cpdef void request_instruments(self, RequestInstruments request):
        cdef list instruments = self._cache.instruments(request.venue)
        if not instruments:
            self._log.error(f"Cannot find instruments")
            return

        self._handle_instruments(request.venue, instruments, request.id, request.params)

    cpdef void request_order_book_snapshot(self, RequestOrderBookSnapshot request):
        # Do nothing else for backtest
        pass

    cpdef void request_quote_ticks(self, RequestQuoteTicks request):
        # Do nothing else for backtest
        pass

    cpdef void request_trade_ticks(self, RequestTradeTicks request):
        # Do nothing else for backtest
        pass

    cpdef void request_bars(self, RequestBars request):
        # Do nothing else for backtest
        pass
