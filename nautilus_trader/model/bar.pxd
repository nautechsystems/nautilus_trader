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

from cpython.datetime cimport datetime

from nautilus_trader.model.c_enums.bar_aggregation cimport BarAggregation
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity


cdef class BarSpecification:
    cdef readonly int step
    """
    Returns
    -------
    int
        The specified step size for bar aggregation.

    """

    cdef readonly BarAggregation aggregation
    """
    Returns
    -------
    BarAggregation
        The specified aggregation method for bars.

    """

    cdef readonly PriceType price_type
    """
    Returns
    -------
    PriceType
        The specified price type for bar aggregation.

    """

    @staticmethod
    cdef BarSpecification from_string_c(str value)
    cdef str aggregation_string(self)
    cdef str price_type_string(self)


cdef class BarType:
    cdef readonly Symbol symbol
    """
    Returns
    -------
    Symbol
        The symbol of the bar type.

    """

    cdef readonly BarSpecification spec
    """
    Returns
    -------
    BarSpecification
        The specification of the bar type.

    """

    @staticmethod
    cdef BarType from_string_c(str value)
    cdef bint is_time_aggregated(self) except *
    cdef str aggregation_string(self)
    cdef str price_type_string(self)


cdef class Bar:
    cdef readonly Price open
    """
    Returns
    -------
    Price
        The open price of the bar.

    """

    cdef readonly Price high
    """
    Returns
    -------
    Price
        The high price of the bar.

    """

    cdef readonly Price low
    """
    Returns
    -------
    Price
        The low price of the bar.

    """

    cdef readonly Price close
    """
    Returns
    -------
    Price
        The close price of the bar.

    """

    cdef readonly Quantity volume
    """
    Returns
    -------
    Quantity
        The volume of the bar.

    """

    cdef readonly datetime timestamp
    """
    Returns
    -------
    datetime
        The timestamp the bar closed at.

    """

    cdef readonly bint checked
    """
    Returns
    -------
    bool
        If the input values were integrity checked.

    """

    @staticmethod
    cdef Bar from_serializable_string_c(str value)
    cpdef str to_serializable_string(self)
