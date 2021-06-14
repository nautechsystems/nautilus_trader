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

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.instruments.betting cimport BettingInstrument
from nautilus_trader.model.instruments.cfd cimport CFDInstrument
from nautilus_trader.model.instruments.crypto_swap cimport CryptoSwap
from nautilus_trader.model.instruments.currency cimport CurrencySpot
from nautilus_trader.model.instruments.future cimport Future
from nautilus_trader.model.instruments.option cimport Option


cdef class InstrumentUnpacker:
    """
    Provides a means of unpacking instruments from value dictionaries.
    """

    @staticmethod
    cdef Instrument unpack_c(dict values):
        Condition.not_none(values, "values")

        cdef str instrument_type = values.get("type")
        if instrument_type is None:
            raise RuntimeError("Cannot unpack instrument: no 'type' key.")

        if instrument_type == Instrument.__name__:
            return Instrument.from_dict(values)
        elif instrument_type == BettingInstrument.__name__:
            return BettingInstrument.from_dict(values)
        elif instrument_type == CFDInstrument.__name__:
            return CFDInstrument.from_dict(values)
        elif instrument_type == CryptoSwap.__name__:
            return CryptoSwap.from_dict(values)
        elif instrument_type == CurrencySpot.__name__:
            return CurrencySpot.from_dict(values)
        elif instrument_type == Future.__name__:
            return Future.from_dict(values)
        elif instrument_type == Option.__name__:
            return Option.from_dict(values)
        else:
            raise RuntimeError(
                f"Cannot unpack instrument: unrecognized type '{instrument_type}'"
            )

    @staticmethod
    def unpack(dict values) -> Instrument:
        """
        Return an instrument unpacked from the given values.

        Parameters
        ----------
        values : dict[str, object]

        Returns
        -------
        Instrument

        """
        return InstrumentUnpacker.unpack_c(values)
