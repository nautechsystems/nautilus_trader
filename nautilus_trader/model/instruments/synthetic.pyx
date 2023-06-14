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

from libc.stdint cimport uint8_t

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.model cimport SyntheticInstrument_API
from nautilus_trader.core.rust.model cimport symbol_clone
from nautilus_trader.core.rust.model cimport synthetic_instrument_calculate
from nautilus_trader.core.rust.model cimport synthetic_instrument_drop
from nautilus_trader.core.rust.model cimport synthetic_instrument_new
from nautilus_trader.core.rust.model cimport synthetic_instrument_precision
from nautilus_trader.core.string cimport pybytes_to_cstr
from nautilus_trader.core.string cimport pystr_to_cstr
from nautilus_trader.model.identifiers cimport Symbol


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
        If the `forumula` is not a valid string.
    """

    def __init__(
        self,
        Symbol symbol not None,
        uint8_t precision,
        list components not None,
        str formula not None,
    ):
        Condition.true(len(components) >= 2, "There must be at least two component instruments")
        Condition.true(precision <= 9, f"invalid `precision` greater than max 9, was {precision}")
        Condition.valid_string(formula, "formula")

        self._mem = synthetic_instrument_new(
            symbol_clone(&symbol._mem),
            precision,
            pybytes_to_cstr(msgspec.json.encode(components)),
            pystr_to_cstr(formula)
        )

    def __del__(self) -> None:
        if self._mem._0 != NULL:
            synthetic_instrument_drop(self._mem)

    @property
    def precision(self) -> int:
        """
        Return the precision for the synthetic instrument.

        Returns
        -------
        int

        """
        return synthetic_instrument_precision(&self._mem)
