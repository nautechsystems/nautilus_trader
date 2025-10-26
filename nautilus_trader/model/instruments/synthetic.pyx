# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
from libc.math cimport isnan
from libc.stdint cimport uint8_t
from libc.stdint cimport uint64_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.core cimport CVec
from nautilus_trader.core.rust.model cimport ERROR_PRICE
from nautilus_trader.core.rust.model cimport Price_t
from nautilus_trader.core.rust.model cimport SyntheticInstrument_API
from nautilus_trader.core.rust.model cimport symbol_new
from nautilus_trader.core.rust.model cimport synthetic_instrument_calculate
from nautilus_trader.core.rust.model cimport synthetic_instrument_change_formula
from nautilus_trader.core.rust.model cimport synthetic_instrument_components_count
from nautilus_trader.core.rust.model cimport synthetic_instrument_components_to_cstr
from nautilus_trader.core.rust.model cimport synthetic_instrument_drop
from nautilus_trader.core.rust.model cimport synthetic_instrument_formula_to_cstr
from nautilus_trader.core.rust.model cimport synthetic_instrument_id
from nautilus_trader.core.rust.model cimport synthetic_instrument_is_valid_formula
from nautilus_trader.core.rust.model cimport synthetic_instrument_new
from nautilus_trader.core.rust.model cimport synthetic_instrument_price_increment
from nautilus_trader.core.rust.model cimport synthetic_instrument_price_precision
from nautilus_trader.core.rust.model cimport synthetic_instrument_ts_event
from nautilus_trader.core.rust.model cimport synthetic_instrument_ts_init
from nautilus_trader.core.string cimport cstr_to_pybytes
from nautilus_trader.core.string cimport cstr_to_pystr
from nautilus_trader.core.string cimport pybytes_to_cstr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price


cdef class SyntheticInstrument(Data):
    """
    Represents a synthetic instrument with prices derived from component instruments using a
    formula.

    The `id` for the synthetic will become `{symbol}.{SYNTH}`.

    Parameters
    ----------
    symbol : Symbol
        The symbol for the synthetic instrument.
    price_precision : uint8_t
        The price precision for the synthetic instrument.
    components : list[InstrumentId]
        The component instruments for the synthetic instrument.
    formula : str
        The derivation formula for the synthetic instrument.
    ts_event : uint64_t
        UNIX timestamp (nanoseconds) when the data event occurred.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the data object was initialized.

    Raises
    ------
    ValueError
        If `price_precision` is greater than 9.
    OverflowError
        If `price_precision` is negative (< 0).
    ValueError
        If the `components` list does not contain at least 2 instrument IDs.
    ValueError
        If the `formula` is not a valid string.
    ValueError
        If the `formula` is not a valid expression.

    Warnings
    --------
    All component instruments should already be defined and exist in the cache prior to defining
    a new synthetic instrument.

    """

    def __init__(
        self,
        Symbol symbol not None,
        uint8_t price_precision,
        list components not None,
        str formula not None,
        uint64_t ts_event,
        uint64_t ts_init,
    ) -> None:
        Condition.is_true(price_precision <= 9, f"invalid `price_precision` greater than max 9, was {price_precision}")
        Condition.is_true(len(components) >= 2, "There must be at least two component instruments")
        Condition.list_type(components, InstrumentId, "components")
        Condition.valid_string(formula, "formula")

        if not synthetic_instrument_is_valid_formula(&self._mem, pystr_to_cstr(formula)):
            raise ValueError(f"invalid `formula`, was '{formula}'")

        self._mem = synthetic_instrument_new(
            symbol._mem,
            price_precision,
            pybytes_to_cstr(msgspec.json.encode([c.value for c in components])),
            pystr_to_cstr(formula),
            ts_event,
            ts_init,
        )
        self.id = InstrumentId(symbol, Venue("SYNTH"))

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            synthetic_instrument_drop(self._mem)

    # TODO: It's currently not safe to pickle synthetic instruments
    # def __getstate__(self):
    #     return (
    #         self.id.symbol.value,
    #         self.price_precision,
    #         msgspec.json.encode([c.value for c in self.components]),
    #         self.formula,
    #         self.ts_event,
    #         self.ts_init,
    #     )
    #
    # def __setstate__(self, state):
    #     self._mem = synthetic_instrument_new(
    #         symbol_new(pystr_to_cstr(state[0])),
    #         state[1],
    #         pybytes_to_cstr(state[2]),
    #         pystr_to_cstr(state[3]),
    #         state[4],
    #         state[5],
    #     )

    def __eq__(self, SyntheticInstrument other) -> bool:
        return self.id == other.id

    def __hash__(self) -> int:
        return hash(self.id)

    # @property
    # def id(self) -> InstrumentId:
    #     """
    #     Return the synthetic instruments ID.
    #
    #     Returns
    #     -------
    #     InstrumentId
    #
    #     """
    #     return InstrumentId.from_mem_c(synthetic_instrument_id(&self._mem))

    @property
    def price_precision(self) -> int:
        """
        Return the precision for the synthetic instrument.

        Returns
        -------
        int

        """
        return synthetic_instrument_price_precision(&self._mem)

    @property
    def price_increment(self) -> Price:
        """
        Return the minimum price increment (tick size) for the synthetic instrument.

        Returns
        -------
        Price

        """
        return Price.from_mem_c(synthetic_instrument_price_increment(&self._mem))

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
        Return the synthetic instrument internal derivation formula.

        Returns
        -------
        str

        """
        return cstr_to_pystr(synthetic_instrument_formula_to_cstr(&self._mem))

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return synthetic_instrument_ts_event(&self._mem)

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return synthetic_instrument_ts_init(&self._mem)

    cpdef void change_formula(self, str formula):
        """
        Change the internal derivation formula for the synthetic instrument.

        Parameters
        ----------
        formula : str
            The derivation formula to change to.

        Raises
        ------
        ValueError
            If the `formula` is not a valid string.
        ValueError
            If the `formula` is not a valid expression.

        """
        Condition.valid_string(formula, "formula")

        if not synthetic_instrument_is_valid_formula(&self._mem, pystr_to_cstr(formula)):
            raise ValueError(f"invalid `formula`, was '{formula}'")

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

        Raises
        ------
        ValueError
            If `inputs` is empty, contains a NaN value, or length is different from components count.
        RuntimeError
            If an internal error occurs when calculating the price.

        """
        Condition.not_empty(inputs, "inputs")

        cdef uint64_t len_ = len(inputs)

        cdef uint64_t components_count = synthetic_instrument_components_count(&self._mem)
        if len_ != components_count:
            raise ValueError(
                f"error calculating {self.id} `SyntheticInstrument` price: "
                f"length of inputs ({len_}) not equal to components count ({components_count})",
            )

        cdef double value
        for value in inputs:
            if isnan(value):
                raise ValueError(f"NaN detected in inputs {inputs}")

        # Create a C doubles buffer
        cdef double* data = <double *>PyMem_Malloc(len_ * sizeof(double))
        if not data:
            raise MemoryError()

        cdef uint64_t i
        for i in range(len_):
            data[i] = <double>inputs[i]

        # Create CVec
        cdef CVec* cvec = <CVec *>PyMem_Malloc(1 * sizeof(CVec))
        if not cvec:
            raise MemoryError()

        cvec.ptr = data
        cvec.len = len_
        cvec.cap = len_

        cdef Price_t mem = synthetic_instrument_calculate(&self._mem, cvec)
        if mem.precision == ERROR_PRICE.precision:
            raise RuntimeError(
                f"error calculating {self.id} `SyntheticInstrument` price from {inputs}",
            )

        cdef Price price = Price.from_mem_c(mem)

        PyMem_Free(cvec.ptr) # De-allocate buffer
        PyMem_Free(cvec) # De-allocate cvec

        return price

    @staticmethod
    cdef SyntheticInstrument from_dict_c(dict values):
        Condition.not_none(values, "values")
        return SyntheticInstrument(
            symbol=Symbol(values["symbol"]),
            price_precision=values["price_precision"],
            components=[InstrumentId.from_str_c(c) for c in values["components"]],
            formula=values["formula"],
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    cdef dict to_dict_c(SyntheticInstrument obj):
        Condition.not_none(obj, "obj")
        return {
            "type": "SyntheticInstrument",
            "symbol": obj.id.symbol.value,
            "price_precision": obj.price_precision,
            "components": [c.value for c in obj.components],
            "formula": obj.formula,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
        }

    @staticmethod
    def from_dict(dict values) -> SyntheticInstrument:
        """
        Return an instrument from the given initialization values.

        Parameters
        ----------
        values : dict[str, object]
            The values to initialize the instrument with.

        Returns
        -------
        SyntheticInstrument

        """
        return SyntheticInstrument.from_dict_c(values)

    @staticmethod
    def to_dict(SyntheticInstrument obj) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        return SyntheticInstrument.to_dict_c(obj)
