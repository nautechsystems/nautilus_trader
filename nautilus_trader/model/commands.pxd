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
from nautilus_trader.core.types cimport ValidString
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order cimport BracketOrder
from nautilus_trader.model.order cimport Order


cdef class AccountInquiry(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id


cdef class SubmitOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly StrategyId strategy_id
    cdef readonly PositionId position_id
    cdef readonly Order order


cdef class SubmitBracketOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly StrategyId strategy_id
    cdef readonly PositionId position_id
    cdef readonly BracketOrder bracket_order


cdef class ModifyOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly OrderId order_id
    cdef readonly Quantity modified_quantity
    cdef readonly Price modified_price


cdef class CancelOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly OrderId order_id
    cdef readonly ValidString cancel_reason
