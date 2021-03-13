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

from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument


cdef class InstrumentProvider:
    """
    The abstract base class for all instrument providers.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(self, Venue venue not None, bint load_all=False):
        """
        Initialize a new instance of the `InstrumentProvider` class.

        Parameters
        ----------
        venue : Venue
            The venue for the provider.
        load_all : bool, optional
            If all instruments should be loaded at instantiation.

        """
        self.venue = venue
        self.count = 0

        self._currencies = {}   # type: dict[str, Currency]
        self._instruments = {}  # type: dict[Symbol, Instrument]

        if load_all:
            self.load_all()

    async def load_all_async(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void load_all(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef dict get_all(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Instrument get(self, InstrumentId instrument_id):
        """
        Get the instrument for the given instrument identifier (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the instrument

        Returns
        -------
        Instrument or None

        """
        return self.get_c(instrument_id.symbol)

    cpdef Currency currency(self, str code):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cdef Instrument get_c(self, Symbol symbol):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")
