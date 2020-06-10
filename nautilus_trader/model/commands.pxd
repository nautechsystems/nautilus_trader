# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport ValidString
from nautilus_trader.core.message cimport Command
from nautilus_trader.model.identifiers cimport OrderId, TraderId, StrategyId, PositionId, AccountId
from nautilus_trader.model.objects cimport Price, Quantity
from nautilus_trader.model.order cimport Order, AtomicOrder


cdef class AccountInquiry(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id


cdef class SubmitOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly StrategyId strategy_id
    cdef readonly PositionId position_id
    cdef readonly Order order


cdef class SubmitAtomicOrder(Command):
    cdef readonly TraderId trader_id
    cdef readonly AccountId account_id
    cdef readonly StrategyId strategy_id
    cdef readonly PositionId position_id
    cdef readonly AtomicOrder atomic_order


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
