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

from nautilus_trader.core.message cimport Command
from nautilus_trader.model.identifiers cimport AccountId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport OrderId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.order.base cimport Order
from nautilus_trader.model.order.bracket cimport BracketOrder


cdef class TradingCommand(Command):
    cdef readonly Venue venue
    """The venue the command relates to.\n\n:returns: `Venue`"""


cdef class SubmitOrder(TradingCommand):
    cdef readonly TraderId trader_id
    """The trader identifier associated with the command.\n\n:returns: `TraderId`"""
    cdef readonly AccountId account_id
    """The account identifier associated with the command.\n\n:returns: `AccountId`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the command.\n\n:returns: `StrategyId`"""
    cdef readonly PositionId position_id
    """The position identifier associated with the command.\n\n:returns: `PositionId`"""
    cdef readonly Order order
    """The order for the command.\n\n:returns: `Order`"""
    cdef readonly bint approved
    """If the order has been risk approved.\n\n:returns: `bool`"""

    cdef void approve(self) except *


cdef class SubmitBracketOrder(TradingCommand):
    cdef readonly TraderId trader_id
    """The trader identifier associated with the command.\n\n:returns: `TraderId`"""
    cdef readonly AccountId account_id
    """The account identifier associated with the command.\n\n:returns: `AccountId`"""
    cdef readonly StrategyId strategy_id
    """The strategy identifier associated with the command.\n\n:returns: `StrategyId`"""
    cdef readonly BracketOrder bracket_order
    """The bracket order to submit.\n\n:returns: `BracketOrder`"""
    cdef readonly bint approved
    """If the bracket order has been risk approved.\n\n:returns: `bool`"""

    cdef void approve(self) except *


cdef class CancelOrder(TradingCommand):
    cdef readonly TraderId trader_id
    """The trader identifier associated with the command.\n\n:returns: `TraderId`"""
    cdef readonly AccountId account_id
    """The account identifier associated with the command.\n\n:returns: `AccountId`"""
    cdef readonly ClientOrderId cl_ord_id
    """The client order identifier associated with the command.\n\n:returns: `ClientOrderId`"""
    cdef readonly OrderId order_id
    """The order identifier associated with the command.\n\n:returns: `OrderId`"""


cdef class AmendOrder(TradingCommand):
    cdef readonly TraderId trader_id
    """The trader identifier associated with the command.\n\n:returns: `TraderId`"""
    cdef readonly AccountId account_id
    """The account identifier associated with the command.\n\n:returns: `AccountId`"""
    cdef readonly ClientOrderId cl_ord_id
    """The client order identifier associated with the command.\n\n:returns: `ClientOrderId`"""
    cdef readonly Quantity quantity
    """The quantity for the command.\n\n:returns: `Quantity`"""
    cdef readonly Price price
    """The price for the command.\n\n:returns: `Price`"""
