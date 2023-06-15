# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from cpython.mem cimport PyMem_Free
from cpython.mem cimport PyMem_Malloc
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport SyntheticInstrument_API
from nautilus_trader.core.rust.model cimport symbol_clone
from nautilus_trader.core.rust.model cimport synthetic_instrument_calculate
from nautilus_trader.core.rust.model cimport synthetic_instrument_change_formula
from nautilus_trader.core.rust.model cimport synthetic_instrument_components_to_cstr
from nautilus_trader.core.rust.model cimport synthetic_instrument_drop
from nautilus_trader.core.rust.model cimport synthetic_instrument_formula_to_cstr
from nautilus_trader.core.rust.model cimport synthetic_instrument_id
from nautilus_trader.core.rust.model cimport synthetic_instrument_new
from nautilus_trader.core.rust.model cimport synthetic_instrument_precision
from nautilus_trader.core.string cimport cstr_to_pybytes
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pybytes_to_cstr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price


cdef class SyntheticInstrument:
    """
    Represents a synthetic instrument with prices derived from component instruments using a
    formula.

    The `id` for the synthetic will become {symbol}.{SYNTH}.

    Parameters
    ----------
    symbol : Symbol
        The symbol for the synethic instrument.
    precision : uint8_t
        The price precision for the synthetic instrument.
    components : list[InstrumentId]
        The component instruments for the synthetic instrument.
    formula : str
        The derivation formula for the synthetic instrument.

    Raises
    ------
    ValueError
        If `precision` is greater than 9.
    OverflowError
        If `precision` is negative (< 0).
    ValueError
        If the `components` list does not contain at least 2 instrument IDs.
    ValueError
        If the `formula` is not a valid string.
    """

    def __init__(
        self,
        Symbol symbol not None,
        uint8_t precision,
        list components not None,
        str formula not None,
    ):
        Condition.true(precision <= 9, f"invalid `precision` greater than max 9, was {precision}")
        Condition.true(len(components) >= 2, "There must be at least two component instruments")
        Condition.list_type(components, InstrumentId, "components")
        Condition.valid_string(formula, "formula")

        self._mem = synthetic_instrument_new(
            symbol_clone(&symbol._mem),
            precision,
            pybytes_to_cstr(msgspec.json.encode([c.value for c in components])),
            pystr_to_cstr(formula),
        )

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            synthetic_instrument_drop(self._mem)

    @property
    def id(self) -> InstrumentId:
        """
        Return the synthetic instruments ID.

        Returns
        -------
        InstrumentId

        """
        return InstrumentId.from_mem_c(synthetic_instrument_id(&self._mem))

    @property
    def precision(self) -> int:
        """
        Return the precision for the synthetic instrument.

        Returns
        -------
        int

        """
        return synthetic_instrument_precision(&self._mem)

    @property
    def components(self) -> list[InstrumentId]:
        """
        Return the components of the synthetic instrument.

        Returns
        -------
        list[InstrumentId]

        """
        cdef bytes components_bytes = cstr_to_pybytes(synthetic_instrument_components_to_cstr(&self._mem))
        return [InstrumentId.from_str_c(c) for c in msgspec.json.decode(components_bytes)]

    @property
    def formula(self) -> str:
        """
        Return the synthetic instrument derivation formula.

        Returns
        -------
        str

        """
        return cstr_to_pystr(synthetic_instrument_formula_to_cstr(&self._mem))

    cpdef void change_formula(self, str formula):
        """
        Change the internal derivation formula by recompiling the internal evaluation engine.

        Parameters
        ----------
        formula : str
            The derivation formula to change to.

        """
        Condition.valid_string(formula, "formula")

        synthetic_instrument_change_formula(&self._mem, pystr_to_cstr(formula))

    cpdef Price calculate(self, list[double] inputs):
        """
        Calculate the price of the synthetic instrument from the given `inputs`.

        Parameters
        ----------
        inputs : list[double]

        Returns
        -------
        Price

        """
        # Create a C doubles buffer
        cdef uint64_t len_ = len(inputs)
        cdef double * data = <double *>PyMem_Malloc(len_ * sizeof(double))
        if not data:
            raise MemoryError()

        cdef uint64_t i
        for i in range(len_):
            data[i] = <double>inputs[i]

        # Create CVec
        cvec = <CVec *> PyMem_Malloc(1 * sizeof(CVec))
        if not cvec:
            raise MemoryError()

        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        cdef Price_t mem = synthetic_instrument_calculate(&self._mem, cvec)
        cdef Price price = Price.from_mem_c(mem)

        PyMem_Free(cvec.ptr) # De-allocate buffer
        PyMem_Free(cvec) # De-allocate cvec

        return price
