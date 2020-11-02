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
from nautilus_trader.model.identifiers cimport Venue


cdef class Connect(Command):
    cdef readonly Venue venue
    """The venue for the command.\n\n:returns: `Venue`"""


cdef class Disconnect(Command):
    cdef readonly Venue venue
    """The venue for the command.\n\n:returns: `Venue`"""


cdef class DataCommand(Command):
    cdef readonly type data_type
    """The data type of the command.\n\n:returns: `type`"""
    cdef readonly dict options
    """The command options.\n\n:returns: `dict`"""


cdef class Subscribe(DataCommand):
    pass


cdef class Unsubscribe(DataCommand):
    pass


cdef class RequestData(DataCommand):
    pass


cdef class KillSwitch(Command):
    cdef readonly TraderId trader_id
    """The trader identifier associated with the command.\n\n:returns: `TraderId`"""
