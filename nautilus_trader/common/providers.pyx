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
from nautilus_trader.model.instruments.base cimport Instrument


cdef class InstrumentProvider:
    """
    The abstract base class for all instrument providers.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self):
        """
        Initialize a new instance of the ``InstrumentProvider`` class.

        """
        self._instruments = {}  # type: dict[InstrumentId, Instrument]
        self._currencies = {}   # type: dict[str, Currency]

    @property
    def count(self) -> int:
        """
        The count of instruments held by the provider.

        Returns
        -------
        int

        """
        return len(self._instruments)

    async def load_all_async(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void load_all(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void load(self, InstrumentId instrument_id, dict details) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    cpdef void add(self, Instrument instrument) except *:
        """
        Add the given instrument to the provider.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        self._instruments[instrument.id] = instrument

    cpdef dict get_all(self):
        """
        Return all loaded instruments.

        If no instruments loaded, will return an empty dict.

        Returns
        -------
        dict[InstrumentId, Instrument]

        """
        return self._instruments.copy()

    cpdef dict currencies(self):
        """
        Return all currencies held by the instrument provider.

        Returns
        -------
        dict[str, Currency]

        """
        return self._currencies.copy()

    cpdef Currency currency(self, str code):
        """
        Return the currency with the given code (if found).

        Parameters
        ----------
        code : str
            The currency code.

        Returns
        -------
        Currency or None

        """
        cdef Currency currency = self._currencies.get(code)
        if currency is None:
            currency = Currency.from_str_c(code)
        return currency

    cpdef Instrument find(self, InstrumentId instrument_id):
        """
        Return the instrument for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The ID for the instrument

        Returns
        -------
        Instrument or None

        """
        return self._instruments.get(instrument_id)
