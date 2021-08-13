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

from decimal import Decimal

from nautilus_trader.accounting.base cimport Account
from nautilus_trader.model.c_enums.position_side cimport PositionSide
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class MarginAccount(Account):
    cdef dict _leverages
    cdef dict _margins_initial
    cdef dict _margins_maint

# -- QUERIES ---------------------------------------------------------------------------------------

    cpdef dict leverages(self)
    cpdef dict margins_initial(self)
    cpdef dict margins_maint(self)
    cpdef object leverage(self, InstrumentId instrument_id)
    cpdef Money margin_initial(self, Currency currency=*)
    cpdef Money margin_maint(self, Currency currency=*)

# -- COMMANDS --------------------------------------------------------------------------------------

    cpdef void set_leverage(self, InstrumentId instrument_id, leverage: Decimal) except *
    cpdef void update_margin_maint(self, Money margin_maint) except *

# -- CALCULATIONS ----------------------------------------------------------------------------------

    cpdef Money calculate_margin_maint(
        self,
        Instrument instrument,
        PositionSide side,
        Quantity quantity,
        Price last,
        bint inverse_as_quote=*,
    )
