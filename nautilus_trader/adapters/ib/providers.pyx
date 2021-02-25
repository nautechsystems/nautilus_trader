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

import ib_insync

from nautilus_trader.model.identifiers cimport Security
from nautilus_trader.model.instrument cimport Instrument


cdef class IBInstrumentProvider:
    """
    Provides a means of loading `Instrument` objects through Interactive Brokers.
    """

    def __init__(self, client not None: ib_insync.Client):
        """
        Initialize a new instance of the `IBInstrumentProvider` class.

        Parameters
        ----------
        client : ib_insync.Client
            The Interactive Brokers client.

        """
        self.name = "IB"
        self.count = 0
        self._instruments = {}  # type: dict[Security, Instrument]
        self._client = client

    cpdef Instrument get(self, Security security):
        """
        Return the instrument for the given security (if found).

        Returns
        -------
        Instrument or None

        """
        return self._instruments.get(security)

    cdef Instrument _parse_instrument(self, dict values):
        pass
