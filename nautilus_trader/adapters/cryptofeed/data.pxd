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

from nautilus_trader.adapters.ccxt.data cimport CCXTDataClient
from nautilus_trader.model.instrument cimport Instrument


cdef class CryptofeedDataClient(CCXTDataClient):
    cdef object _subscriber

    cdef dict _market_id_to_instrument

    cdef object _handle_messages_task

    cdef str _url

    cdef str _make_order_book_topic(self, Instrument instrument)
    cdef str _make_trade_tick_topic(self, Instrument instrument)
    cdef str _convert_instrument_to_market_id(self, Instrument instrument)
