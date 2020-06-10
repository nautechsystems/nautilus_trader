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

from nautilus_trader.core.message cimport Request, Response
from nautilus_trader.network.identifiers cimport ClientId, ServerId, SessionId


cdef class Connect(Request):
    cdef readonly ClientId client_id
    cdef readonly str authentication


cdef class Connected(Response):
    cdef readonly str message
    cdef readonly ServerId server_id
    cdef readonly SessionId session_id


cdef class Disconnect(Request):
    cdef readonly ClientId client_id
    cdef readonly SessionId session_id


cdef class Disconnected(Response):
    cdef readonly str message
    cdef readonly ServerId server_id
    cdef readonly SessionId session_id


cdef class MessageReceived(Response):
    cdef readonly str received_type


cdef class MessageRejected(Response):
    cdef readonly str message


cdef class QueryFailure(Response):
    cdef readonly str message


cdef class DataRequest(Request):
    cdef readonly dict query


cdef class DataResponse(Response):
    cdef readonly bytes data
    cdef readonly str data_type
    cdef readonly str data_encoding
