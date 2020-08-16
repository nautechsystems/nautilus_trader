# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.tick cimport QuoteTick, TradeTick
from nautilus_trader.model.bar cimport BarType, Bar
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.core.message cimport Event


cdef class Handler:
    cdef readonly object handle


cdef class QuoteTickHandler(Handler):
    cdef void handle(self, QuoteTick tick) except *


cdef class TradeTickHandler(Handler):
    cdef void handle(self, TradeTick tick) except *


cdef class BarHandler(Handler):
    cdef void handle(self, BarType bar_type, Bar bar) except *


cdef class InstrumentHandler(Handler):
    cdef void handle(self, Instrument instrument) except *


cdef class EventHandler(Handler):
    cdef void handle(self, Event event) except *
