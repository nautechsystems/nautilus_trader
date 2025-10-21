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
Provides a centralized topic cache for managing message bus topic generation and caching.

The `TopicCache` consolidates all topic generation methods and their caching dictionaries
that were previously scattered across the data engine and other components.
"""

from nautilus_trader.model.data cimport BarType
from nautilus_trader.model.data cimport DataType
from nautilus_trader.model.data cimport OrderBookDelta
from nautilus_trader.model.data cimport OrderBookDepth10
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue


cdef class TopicCache:
    """
    Provides a centralized cache for message bus topic generation and caching.

    This class consolidates all topic generation methods and their caching dictionaries
    that were previously scattered across the data engine and other components.
    """

    def __init__(self):
        # Initialize all topic cache dictionaries
        self._topic_cache_instruments = {}
        self._topic_cache_instruments_pattern = {}
        self._topic_cache_deltas = {}
        self._topic_cache_depth = {}
        self._topic_cache_quotes = {}
        self._topic_cache_trades = {}
        self._topic_cache_status = {}
        self._topic_cache_mark_prices = {}
        self._topic_cache_index_prices = {}
        self._topic_cache_funding_rates = {}
        self._topic_cache_close_prices = {}
        self._topic_cache_snapshots = {}
        self._topic_cache_custom = {}
        self._topic_cache_custom_simple = {}
        self._topic_cache_bars = {}
        self._topic_cache_signal = {}

    cpdef str get_instrument_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_instruments.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.instrument.{instrument_id.venue}.{instrument_id.symbol}"
            self._topic_cache_instruments[key] = topic

        return topic

    cpdef str get_instruments_topic(self, Venue venue):
        """
        Get the topic pattern for all instruments at a venue.

        Parameters
        ----------
        venue : Venue
            The venue for the pattern.

        Returns
        -------
        str
            The topic pattern string.

        """
        cdef str topic = self._topic_cache_instruments_pattern.get(venue)
        if topic is None:
            topic = f"data.instrument.{venue}.*"
            self._topic_cache_instruments_pattern[venue] = topic

        return topic

    cpdef str get_book_topic(self, type book_data_type, InstrumentId instrument_id, bint historical = False):
        if book_data_type == OrderBookDelta:
            return self.get_deltas_topic(instrument_id, historical)
        elif book_data_type == OrderBookDepth10:
            return self.get_depth_topic(instrument_id, historical)
        else:  # pragma: no cover (design-time error)
            raise TypeError(f"Invalid book data type, was {book_data_type}")

    cpdef str get_deltas_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_deltas.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.book.deltas.{instrument_id.venue}.{instrument_id.symbol.topic()}"
            self._topic_cache_deltas[key] = topic

        return topic

    cpdef str get_depth_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_depth.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.book.depth.{instrument_id.venue}.{instrument_id.symbol.topic()}"
            self._topic_cache_depth[key] = topic

        return topic

    cpdef str get_quotes_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_quotes.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.quotes.{instrument_id.venue}.{instrument_id.symbol}"
            self._topic_cache_quotes[key] = topic

        return topic

    cpdef str get_trades_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_trades.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.trades.{instrument_id.venue}.{instrument_id.symbol}"
            self._topic_cache_trades[key] = topic

        return topic

    cpdef str get_status_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_status.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.status.{instrument_id.venue}.{instrument_id.symbol}"
            self._topic_cache_status[key] = topic

        return topic

    cpdef str get_mark_prices_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_mark_prices.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.mark_prices.{instrument_id.venue}.{instrument_id.symbol}"
            self._topic_cache_mark_prices[key] = topic

        return topic

    cpdef str get_index_prices_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_index_prices.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.index_prices.{instrument_id.venue}.{instrument_id.symbol}"
            self._topic_cache_index_prices[key] = topic

        return topic

    cpdef str get_funding_rates_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_funding_rates.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.funding_rates.{instrument_id.venue}.{instrument_id.symbol}"
            self._topic_cache_funding_rates[key] = topic

        return topic

    cpdef str get_close_prices_topic(self, InstrumentId instrument_id, bint historical = False):
        cdef tuple key = (instrument_id, historical)
        cdef str topic = self._topic_cache_close_prices.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.venue.close_price.{instrument_id}"
            self._topic_cache_close_prices[key] = topic

        return topic

    cpdef str get_snapshots_topic(self, InstrumentId instrument_id, int interval_ms, bint historical = False):
        cdef tuple key = (instrument_id, interval_ms, historical)
        cdef str topic = self._topic_cache_snapshots.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.book.snapshots.{instrument_id.venue}.{instrument_id.symbol}.{interval_ms}"
            self._topic_cache_snapshots[key] = topic

        return topic

    cpdef str get_custom_data_topic(self, DataType data_type, InstrumentId instrument_id = None, bint historical = False):
        cdef:
            tuple key
            str topic

        # Handle both cases: with and without instrument_id
        if instrument_id is not None:
            key = (data_type, instrument_id, historical)
            topic = self._topic_cache_custom.get(key)
            if topic is None:
                if data_type.metadata:
                    topic = f"{'historical.' if historical else ''}data.{data_type.topic}"
                else:
                    topic = f"{'historical.' if historical else ''}data.{data_type.type.__name__}.{instrument_id.venue}.{instrument_id.symbol.topic()}"
                self._topic_cache_custom[key] = topic
        else:
            key = (data_type, historical)
            topic = self._topic_cache_custom_simple.get(key)
            if topic is None:
                topic = f"{'historical.' if historical else ''}data.{data_type.topic}"
                self._topic_cache_custom_simple[key] = topic

        return topic

    cpdef str get_bars_topic(self, BarType bar_type, bint historical = False):
        cdef tuple key = (bar_type, historical)
        cdef str topic = self._topic_cache_bars.get(key)
        if topic is None:
            topic = f"{'historical.' if historical else ''}data.bars.{bar_type}"
            self._topic_cache_bars[key] = topic

        return topic

    cpdef str get_signal_topic(self, str name):
        """
        Get the topic for a signal subscription.

        Parameters
        ----------
        name : str
            The signal name.

        Returns
        -------
        str
            The topic string.

        """
        cdef str topic = self._topic_cache_signal.get(name)
        if topic is None:
            topic = f"data.Signal{name.title()}*"
            self._topic_cache_signal[name] = topic

        return topic

    cpdef void clear_cache(self):
        self._topic_cache_instruments.clear()
        self._topic_cache_instruments_pattern.clear()
        self._topic_cache_deltas.clear()
        self._topic_cache_depth.clear()
        self._topic_cache_quotes.clear()
        self._topic_cache_trades.clear()
        self._topic_cache_status.clear()
        self._topic_cache_mark_prices.clear()
        self._topic_cache_index_prices.clear()
        self._topic_cache_funding_rates.clear()
        self._topic_cache_close_prices.clear()
        self._topic_cache_snapshots.clear()
        self._topic_cache_custom.clear()
        self._topic_cache_custom_simple.clear()
        self._topic_cache_bars.clear()
        self._topic_cache_signal.clear()
