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

from nautilus_trader.core.message cimport Command
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.common.execution cimport ExecutionClient
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.node_clients cimport MessageClient, MessageSubscriber
from nautilus_trader.serialization.base cimport CommandSerializer, EventSerializer

cdef class LiveExecClient(ExecutionClient):
    cdef MessageClient _command_client
    cdef MessageSubscriber _event_subscriber

    cdef CommandSerializer _command_serializer
    cdef EventSerializer _event_serializer

    cdef readonly TraderId trader_id
    cdef readonly ClientId client_id

    cpdef void _send_command(self, Command command) except *
    cpdef void _recv_event(self, str topic, bytes event_bytes) except *
