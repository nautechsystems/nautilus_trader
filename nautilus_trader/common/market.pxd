# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport date

from nautilus_trader.common.exchange cimport ExchangeRateCalculator
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class CommissionModel:

    cpdef Money calculate(
        self,
        Symbol symbol,
        Quantity filled_quantity,
        Price filled_price,
        double exchange_rate,
        Currency currency,
        LiquiditySide liquidity_side,
    )
    cpdef Money calculate_for_notional(
        self,
        Symbol symbol,
        Money notional_value,
        LiquiditySide liquidity_side,
    )

    cdef double _get_commission_rate(
        self,
        Symbol symbol,
        LiquiditySide liquidity_side,
    )


cdef class GenericCommissionModel(CommissionModel):

    cdef dict rates
    cdef double default_rate_bp
    cdef Money minimum


cdef class MakerTakerCommissionModel(CommissionModel):

    cdef dict taker_rates
    cdef dict maker_rates
    cdef double taker_default_rate_bp
    cdef double maker_default_rate_bp


cdef class RolloverInterestCalculator:
    cdef ExchangeRateCalculator _exchange_calculator
    cdef dict _rate_data

    cpdef object get_rate_data(self)
    cpdef double calc_overnight_rate(self, Symbol symbol, date timestamp) except *
