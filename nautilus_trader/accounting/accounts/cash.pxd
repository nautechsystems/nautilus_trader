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

from nautilus_trader.accounting.accounts.base cimport Account
from nautilus_trader.core.rust.model cimport OrderSide
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CashAccount(Account):
    cdef dict _balances_locked

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void update_balance_locked(self, InstrumentId instrument_id, Money locked)
    cpdef void clear_balance_locked(self, InstrumentId instrument_id)

# -- CALCULATIONS ---------------------------------------------------------------------------------

    cpdef Money calculate_balance_locked(
        self,
        Instrument instrument,
        OrderSide side,
        Quantity quantity,
        Price price,
        bint use_quote_for_inverse=*,
    )

    @staticmethod
    cdef dict to_dict_c(CashAccount obj)

    @staticmethod
    cdef CashAccount from_dict_c(dict values)
