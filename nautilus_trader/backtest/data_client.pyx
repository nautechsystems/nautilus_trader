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
This module provides a data client for backtesting.
"""

from nautilus_trader.common.config import NautilusConfig

from cpython.datetime cimport datetime

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.component cimport Clock
from nautilus_trader.common.component cimport MessageBus
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport BookType
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
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
    ):
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
        self._log.info(f"Connected.")

    cpdef void _stop(self):
        self._log.info(f"Disconnecting...")
        self.is_connected = False
        self._log.info(f"Disconnected.")

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe(self, DataType data_type):
        Condition.not_none(data_type, "data_type")

        self._add_subscription(data_type)
        # Do nothing else for backtest

    cpdef void unsubscribe(self, DataType data_type):
        Condition.not_none(data_type, "data_type")

        self._remove_subscription(data_type)
        # Do nothing else for backtest

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID4 correlation_id):
        Condition.not_none(data_type, "data_type")
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
        self._log.info(f"Connected.")

    cpdef void _stop(self):
        self._log.info(f"Disconnecting...")
        self.is_connected = False
        self._log.info(f"Disconnected.")

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe_instruments(self):
        cdef Instrument instrument
        for instrument in self._cache.instruments(Venue(self.id.value)):
            self.subscribe_instrument(instrument.id)
        # Do nothing else for backtest

    cpdef void subscribe_instrument(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        if not self._cache.instrument(instrument_id):
            self._log.error(
                f"Cannot find instrument {instrument_id} to subscribe for `Instrument` data.",
            )
            return

        self._add_subscription_instrument(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        BookType book_type,
        int depth = 0,
        dict kwargs = None,
    ):
        Condition.not_none(instrument_id, "instrument_id")

        if not self._cache.instrument(instrument_id):
            self._log.error(
                f"Cannot find instrument {instrument_id} to subscribe for `OrderBookDelta` data. "
                "No data has been loaded for this instrument.",
            )
            return

        self._add_subscription_order_book_deltas(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_snapshots(
        self,
        InstrumentId instrument_id,
        BookType book_type,
        int depth = 0,
        dict kwargs = None,
    ):
        Condition.not_none(instrument_id, "instrument_id")

        if not self._cache.instrument(instrument_id):
            self._log.error(
                f"Cannot find instrument {instrument_id} to subscribe for `OrderBook` data. "
                "No data has been loaded for this instrument.",
            )
            return

        self._add_subscription_order_book_snapshots(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        if not self._cache.instrument(instrument_id):
            self._log.error(
                f"Cannot find instrument {instrument_id} to subscribe for `QuoteTick` data. "
                "No data has been loaded for this instrument.",
            )
            return

        self._add_subscription_quote_ticks(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        if not self._cache.instrument(instrument_id):
            self._log.error(
                f"Cannot find instrument {instrument_id} to subscribe for `TradeTick` data. "
                "No data has been loaded for this instrument.",
            )
            return

        self._add_subscription_trade_ticks(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_bars(self, BarType bar_type):
        Condition.not_none(bar_type, "bar_type")

        if not self._cache.instrument(bar_type.instrument_id):
            self._log.error(
                f"Cannot find instrument {bar_type.instrument_id} to subscribe for `Bar` data. "
                "No data has been loaded for this instrument.",
            )
            return

        self._add_subscription_bars(bar_type)
        # Do nothing else for backtest

    cpdef void subscribe_venue_status(self, Venue venue):
        Condition.not_none(venue, "venue")

        self._add_subscription_venue_status(venue)
        # Do nothing else for backtest

    cpdef void subscribe_instrument_status(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_instrument_status(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_instrument_close(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_instrument_close(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_instruments(self):
        self._subscriptions_instrument.clear()
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_instrument(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_order_book_deltas(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_snapshots(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_order_book_snapshots(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_quote_ticks(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_trade_ticks(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_bars(self, BarType bar_type):
        Condition.not_none(bar_type, "bar_type")

        self._remove_subscription_bars(bar_type)
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument_status(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_instrument_status(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_venue_status(self, Venue venue):
        Condition.not_none(venue, "venue")

        self._remove_subscription_venue_status(venue)

    cpdef void unsubscribe_instrument_close(self, InstrumentId instrument_id):
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_instrument_close(instrument_id)
        # Do nothing else for backtest

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request_instrument(
        self,
        InstrumentId instrument_id,
        UUID4 correlation_id,
        datetime start: datetime | None = None,
        datetime end: datetime | None = None,
    ):
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(correlation_id, "correlation_id")

        cdef Instrument instrument = self._cache.instrument(instrument_id)
        if instrument is None:
            self._log.error(f"Cannot find instrument for {instrument_id}.")
            return

        data_type = DataType(
            type=Instrument,
            metadata={"instrument_id": instrument_id},
        )

        self._handle_instrument(
            instrument=instrument,
            correlation_id=correlation_id,
        )

    cpdef void request_instruments(
        self,
        Venue venue,
        UUID4 correlation_id,
        datetime start: datetime | None = None,
        datetime end: datetime | None = None,
    ):
        Condition.not_none(correlation_id, "correlation_id")

        cdef list instruments = self._cache.instruments(venue)
        if not instruments:
            self._log.error(f"Cannot find instruments.")
            return

        self._handle_instruments(
            venue=venue,
            instruments=instruments,
            correlation_id=correlation_id,
        )

    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        int limit,
        UUID4 correlation_id,
        datetime start: datetime | None = None,
        datetime end: datetime | None = None,
    ):
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(correlation_id, "correlation_id")

        # Do nothing else for backtest

    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        int limit,
        UUID4 correlation_id,
        datetime start: datetime | None = None,
        datetime end: datetime | None = None,
    ):
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        # Do nothing else for backtest

    cpdef void request_bars(
        self,
        BarType bar_type,
        int limit,
        UUID4 correlation_id,
        datetime start: datetime | None = None,
        datetime end: datetime | None = None,
    ):
        Condition.not_none(bar_type, "bar_type")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        # Do nothing else for backtest
