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

from nautilus_trader.core.message cimport Command
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport Order


cdef class SubmitOrder(Command):
    cdef Venue _venue
    cdef TraderId _trader_id
    cdef AccountId _account_id
    cdef StrategyId _strategy_id
    cdef PositionId _position_id
    cdef Order _order


cdef class SubmitBracketOrder(Command):
    cdef Venue _venue
    cdef TraderId _trader_id
    cdef AccountId _account_id
    cdef StrategyId _strategy_id
    cdef BracketOrder _bracket_order


cdef class ModifyOrder(Command):
    cdef Venue _venue
    cdef TraderId _trader_id
    cdef AccountId _account_id
    cdef ClientOrderId _cl_ord_id
    cdef Quantity _quantity
    cdef Price _price


cdef class CancelOrder(Command):
    cdef Venue _venue
    cdef TraderId _trader_id
    cdef AccountId _account_id
    cdef ClientOrderId _cl_ord_id
