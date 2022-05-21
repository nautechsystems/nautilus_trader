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

"""
This module provides a data client for backtesting.
"""

from cpython.datetime cimport datetime

from nautilus_trader.cache.cache cimport Cache
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID4
from nautilus_trader.data.client cimport DataClient
from nautilus_trader.data.client cimport MarketDataClient
from nautilus_trader.model.c_enums.book_type cimport BookType
from nautilus_trader.model.data.bar cimport BarType
from nautilus_trader.model.data.base cimport DataType
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.msgbus.bus cimport MessageBus


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
    logger : Logger
        The logger for the client.
    config : dict[str, object], optional
        The configuration for the instance.
    """

    def __init__(
        self,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        super().__init__(
            client_id=client_id,
            venue=Venue(client_id.to_str()),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
            config=config,
        )

        self.is_connected = False

    cpdef void _start(self) except *:
        self._log.info(f"Connecting...")
        self.is_connected = True
        self._log.info(f"Connected.")

    cpdef void _stop(self) except *:
        self._log.info(f"Disconnecting...")
        self.is_connected = False
        self._log.info(f"Disconnected.")

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe(self, DataType data_type) except *:
        """
        Subscribe to the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type to subscribe to.

        """
        Condition.not_none(data_type, "data_type")

        self._add_subscription(data_type)
        # Do nothing else for backtest

    cpdef void unsubscribe(self, DataType data_type) except *:
        """
        Unsubscribe from the given data type.

        Parameters
        ----------
        data_type : DataType
            The data_type to unsubscribe from.

        """
        Condition.not_none(data_type, "data_type")

        self._remove_subscription(data_type)
        # Do nothing else for backtest

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID4 correlation_id) except *:
        """
        Request the given data type.

        Parameters
        ----------
        data_type : DataType
            The data type to request.
        correlation_id : UUID4
            The correlation ID for the response.

        """
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
    logger : Logger
        The logger for the client.
    """

    def __init__(
        self,
        ClientId client_id not None,
        MessageBus msgbus not None,
        Cache cache not None,
        Clock clock not None,
        Logger logger not None,
    ):
        super().__init__(
            client_id=client_id,
            venue=Venue(client_id.to_str()),
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self.is_connected = False

    cpdef void _start(self) except *:
        self._log.info(f"Connecting...")
        self.is_connected = True
        self._log.info(f"Connected.")

    cpdef void _stop(self) except *:
        self._log.info(f"Disconnecting...")
        self.is_connected = False
        self._log.info(f"Disconnected.")

# -- SUBSCRIPTIONS --------------------------------------------------------------------------------

    cpdef void subscribe_instruments(self) except *:
        """
        Subscribe to `Instrument` data for the venue.

        """
        cdef Instrument instrument
        for instrument in self._cache.instruments(Venue(self.id.value)):
            self.subscribe_instrument(instrument.id)
        # Do nothing else for backtest

    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_instrument(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_snapshots(
        self,
        InstrumentId instrument_id,
        BookType book_type,
        int depth=0,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to `OrderBook` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
            The order book level.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_order_book_snapshots(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_order_book_deltas(
        self,
        InstrumentId instrument_id,
        BookType book_type,
        int depth=0,
        dict kwargs=None,
    ) except *:
        """
        Subscribe to `OrderBook` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to subscribe to.
        book_type : BookType {``L1_TBBO``, ``L2_MBP``, ``L3_MBO``}
            The order book type.
        depth : int, optional
            The maximum depth for the order book. A depth of 0 is maximum depth.
        kwargs : dict, optional
            The keyword arguments for exchange specific parameters.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_order_book_deltas(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_ticker(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `Ticker` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The ticker instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_ticker(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_quote_ticks(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_trade_ticks(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")

        self._add_subscription_bars(bar_type)
        # Do nothing else for backtest

    cpdef void subscribe_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `InstrumentStatusUpdates` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_instrument_status_updates(instrument_id)
        # Do nothing else for backtest

    cpdef void subscribe_instrument_close_prices(self, InstrumentId instrument_id) except *:
        """
        Subscribe to `InstrumentClosePrice` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to subscribe to.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._add_subscription_instrument_close_prices(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_instruments(self) except *:
        """
        Unsubscribe from `Instrument` data for the venue.

        """
        self._subscriptions_instrument.clear()
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `Instrument` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_instrument(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBookData` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_order_book_deltas(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_order_book_snapshots(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `OrderBook` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The order book instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_order_book_snapshots(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_ticker(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `Ticker` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The ticker instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_ticker(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `QuoteTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_quote_ticks(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `TradeTick` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_trade_ticks(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")

        self._remove_subscription_bars(bar_type)
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument_status_updates(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `InstrumentStatusUpdates` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_instrument_status_updates(instrument_id)
        # Do nothing else for backtest

    cpdef void unsubscribe_instrument_close_prices(self, InstrumentId instrument_id) except *:
        """
        Unsubscribe from `InstrumentClosePrice` data for the given instrument ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument to unsubscribe from.

        """
        Condition.not_none(instrument_id, "instrument_id")

        self._remove_subscription_instrument_close_prices(instrument_id)
        # Do nothing else for backtest

# -- REQUESTS -------------------------------------------------------------------------------------

    cpdef void request_instrument(self, InstrumentId instrument_id, UUID4 correlation_id) except *:
        """
        Request an instrument for the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the request.
        correlation_id : UUID4
            The correlation ID for the request.

        """
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

        self._handle_data_response(
            data_type=data_type,
            data=[instrument],  # Data engine handles lists of instruments
            correlation_id=correlation_id,
        )

    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,  # Can be None
        datetime to_datetime,    # Can be None
        int limit,
        UUID4 correlation_id,
    ) except *:
        """
        Request historical quote ticks for the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If ``None`` then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID4
            The correlation ID for the request.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(correlation_id, "correlation_id")

        # Do nothing else for backtest

    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,  # Can be None
        datetime to_datetime,    # Can be None
        int limit,
        UUID4 correlation_id,
    ) except *:
        """
        Request historical trade ticks for the given parameters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The tick instrument ID for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If ``None`` then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID4
            The correlation ID for the request.

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        # Do nothing else for backtest

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,  # Can be None
        datetime to_datetime,    # Can be None
        int limit,
        UUID4 correlation_id,
    ) except *:
        """
        Request historical bars for the given parameters from the data engine.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If ``None`` then will default
            to the current datetime.
        limit : int
            The limit for the number of returned bars.
        correlation_id : UUID4
            The correlation ID for the request.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        # Do nothing else for backtest
