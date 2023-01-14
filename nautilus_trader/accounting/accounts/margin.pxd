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

from decimal import Decimal

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.model.enums_c cimport PositionSide
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport MarginBalance
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class MarginAccount(Account):
    cdef dict _leverages
    cdef dict _margins

    cdef readonly default_leverage
    """The accounts default leverage setting.\n\n:returns: `Decimal`"""

# -- QUERIES --------------------------------------------------------------------------------------

    cpdef dict margins(self)
    cpdef dict margins_init(self)
    cpdef dict margins_maint(self)
    cpdef dict leverages(self)
    cpdef object leverage(self, InstrumentId instrument_id)
    cpdef Money margin_init(self, InstrumentId instrument_id)
    cpdef Money margin_maint(self, InstrumentId instrument_id)
    cpdef MarginBalance margin(self, InstrumentId instrument_id)

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void set_default_leverage(self, leverage: Decimal) except *
    cpdef void set_leverage(self, InstrumentId instrument_id, leverage: Decimal) except *
    cpdef void update_margin_init(self, InstrumentId instrument_id, Money margin_init) except *
    cpdef void update_margin_maint(self, InstrumentId instrument_id, Money margin_maint) except *
    cpdef void update_margin(self, MarginBalance margin) except *
    cpdef void clear_margin_init(self, InstrumentId instrument_id) except *
    cpdef void clear_margin_maint(self, InstrumentId instrument_id) except *
    cpdef void clear_margin(self, InstrumentId instrument_id) except *

# -- CALCULATIONS ---------------------------------------------------------------------------------

    cpdef Money calculate_margin_init(
        self,
        Instrument instrument,
        Quantity quantity,
        Price price,
        bint inverse_as_quote=*,
    )

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price price,
        bint inverse_as_quote=*,
    )
