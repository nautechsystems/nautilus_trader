# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime

import threading

from nautilus_trader.adapters.oanda.providers cimport OandaInstrumentProvider
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data_client cimport LiveMarketDataClient
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick


cdef class OandaDataClient(LiveMarketDataClient):
    cdef object _client
    cdef str _account_id
    cdef set _subscribed_instruments
    cdef dict _subscribed_quote_ticks
    cdef OandaInstrumentProvider _instrument_provider
    cdef object _update_instruments_handle

    cpdef void _load_instruments(self) except *
    cpdef void _request_instrument(self, InstrumentId instrument_id, UUID correlation_id) except *
    cpdef void _request_instruments(self, UUID correlation_id) except *
    cpdef void _subscribed_instruments_update(self) except *
    cpdef void _subscribed_instruments_load_and_send(self) except *
    cpdef void _request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *
    cpdef void _stream_prices(self, InstrumentId instrument_id, event: threading.Event) except *
    cdef QuoteTick _parse_quote_tick(self, InstrumentId instrument_id, dict values)
    cdef Bar _parse_bar(self, BarType bar_type, Instrument instrument, dict values, PriceType price_type)

# -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    cpdef void _handle_data_py(self, Data data) except *
    cpdef void _handle_instruments_py(self, list instruments, UUID correlation_id) except *
    cpdef void _handle_quote_ticks_py(self, InstrumentId instrument_id, list ticks, UUID correlation_id) except *
    cpdef void _handle_trade_ticks_py(self, InstrumentId instrument_id, list ticks, UUID correlation_id) except *
    cpdef void _handle_bars_py(self, BarType bar_type, list bars, Bar partial, UUID correlation_id) except *
