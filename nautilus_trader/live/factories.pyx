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

from nautilus_trader.core.uuid cimport UUID, uuid4
from nautilus_trader.common.uuid cimport UUIDFactory


cdef class LiveUUIDFactory(UUIDFactory):
    """
    Provides a UUID factory for live trading. Generates version 4 UUID's.
    """

    def __init__(self):
        """
        Initialize a new instance of the LiveUUIDFactory class.
        """
        super().__init__()

    cpdef UUID generate(self):
        """
        Return a generated UUID version 4.

        :return UUID.
        """
        return uuid4()
