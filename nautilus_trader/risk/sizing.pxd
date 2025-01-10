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

from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class PositionSizer:
    cdef readonly Instrument instrument
    """The instrument for position sizing.\n\n:returns: `Instrument`"""

    cpdef void update_instrument(self, Instrument instrument)
    cpdef Quantity calculate(
        self,
        Price entry,
        Price stop_loss,
        Money equity,
        risk,
        commission_rate=*,
        exchange_rate=*,
        hard_limit=*,
        unit_batch_size=*,
        int units=*,
    )

    cdef object _calculate_risk_ticks(self, Price entry, Price stop_loss)
    cdef object _calculate_riskable_money(self, equity, risk, commission_rate)


cdef class FixedRiskSizer(PositionSizer):
    pass
