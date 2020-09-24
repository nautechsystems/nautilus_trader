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

from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.message cimport Response
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.messages cimport DataResponse
from nautilus_trader.network.node_clients cimport MessageClient
from nautilus_trader.network.node_clients cimport MessageSubscriber
from nautilus_trader.serialization.base cimport DataSerializer
from nautilus_trader.serialization.base cimport InstrumentSerializer
from nautilus_trader.serialization.constants cimport *


cdef class LiveDataEngine(DataEngine):
    cdef MessageClient _data_client
    cdef MessageSubscriber _data_subscriber
    cdef MessageSubscriber _tick_subscriber
    cdef DataSerializer _data_serializer
    cdef InstrumentSerializer _instrument_serializer
    cdef ObjectCache _cached_symbols
    cdef ObjectCache _cached_bar_types
    cdef dict _correlation_index

    cdef readonly TraderId trader_id
    cdef readonly ClientId client_id
    cdef readonly UUID last_request_id

    cpdef void _set_callback(self, UUID request_id, handler: callable) except *
    cpdef object _pop_callback(self, UUID correlation_id)
    cpdef void _handle_response(self, Response response) except *
    cpdef void _handle_data_response(self, DataResponse response) except *
    cpdef void _handle_instruments_py(self, list instruments) except *
    cpdef void _handle_tick_msg(self, str topic, bytes payload) except *
    cpdef void _handle_sub_msg(self, str topic, bytes payload) except *
