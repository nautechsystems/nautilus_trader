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
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.bracket cimport BracketOrder


cdef class TradingCommand(Command):
    cdef readonly TraderId trader_id
    """The trader ID associated with the command.\n\n:returns: `TraderId`"""
    cdef readonly StrategyId strategy_id
    """The strategy ID associated with the command.\n\n:returns: `StrategyId`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID associated with the command.\n\n:returns: `InstrumentId`"""


cdef class SubmitOrder(TradingCommand):
    cdef readonly PositionId position_id
    """The position ID associated with the command.\n\n:returns: `PositionId`"""
    cdef readonly Order order
    """The order for the command.\n\n:returns: `Order`"""

    @staticmethod
    cdef SubmitOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(SubmitOrder obj)


cdef class SubmitBracketOrder(TradingCommand):
    cdef readonly BracketOrder bracket_order
    """The bracket order to submit.\n\n:returns: `BracketOrder`"""

    @staticmethod
    cdef SubmitBracketOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(SubmitBracketOrder obj)


cdef class UpdateOrder(TradingCommand):
    cdef readonly ClientOrderId client_order_id
    """The client order ID associated with the command.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue order ID associated with the command.\n\n:returns: `VenueOrderId`"""
    cdef readonly Quantity quantity
    """The updated quantity for the command.\n\n:returns: `Quantity` or None"""
    cdef readonly Price price
    """The updated price for the command.\n\n:returns: `Price` or None"""
    cdef readonly Price trigger
    """The updated trigger price for the command.\n\n:returns: `Price` or None"""

    @staticmethod
    cdef UpdateOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(UpdateOrder obj)


cdef class CancelOrder(TradingCommand):
    cdef readonly ClientOrderId client_order_id
    """The client order ID associated with the command.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue order ID associated with the command.\n\n:returns: `VenueOrderId`"""

    @staticmethod
    cdef CancelOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CancelOrder obj)
