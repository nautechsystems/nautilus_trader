# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.enums_c cimport OrderSide
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.identifiers cimport ExecAlgorithmId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport PositionId
from nautilus_trader.model.identifiers cimport StrategyId
from nautilus_trader.model.identifiers cimport TraderId
from nautilus_trader.model.identifiers cimport VenueOrderId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orders.base cimport Order
from nautilus_trader.model.orders.list cimport OrderList


cdef class TradingCommand(Command):
    cdef readonly ClientId client_id
    """The execution client ID for the command.\n\n:returns: `ClientId` or ``None``"""
    cdef readonly TraderId trader_id
    """The trader ID associated with the command.\n\n:returns: `TraderId`"""
    cdef readonly StrategyId strategy_id
    """The strategy ID associated with the command.\n\n:returns: `StrategyId`"""
    cdef readonly InstrumentId instrument_id
    """The instrument ID associated with the command.\n\n:returns: `InstrumentId`"""


cdef class SubmitOrder(TradingCommand):
    cdef readonly Order order
    """The order to submit.\n\n:returns: `Order`"""
    cdef readonly ExecAlgorithmId exec_algorithm_id
    """The execution algorithm ID for the order.\n\n:returns: `ExecAlgorithmId` or ``None``"""
    cdef readonly PositionId position_id
    """The position ID to associate with the order.\n\n:returns: `PositionId` or ``None``"""

    @staticmethod
    cdef SubmitOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(SubmitOrder obj)


cdef class SubmitOrderList(TradingCommand):
    cdef readonly OrderList order_list
    """The order list to submit.\n\n:returns: `OrderList`"""
    cdef readonly ExecAlgorithmId exec_algorithm_id
    """The execution algorithm ID for the order list.\n\n:returns: `ExecAlgorithmId` or ``None``"""
    cdef readonly PositionId position_id
    """The position ID to associate with the orders.\n\n:returns: `PositionId` or ``None``"""
    cdef readonly bint has_emulated_order
    """If the contained order_list holds at least one emulated order.\n\n:returns: `bool`"""

    @staticmethod
    cdef SubmitOrderList from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(SubmitOrderList obj)


cdef class ModifyOrder(TradingCommand):
    cdef readonly ClientOrderId client_order_id
    """The client order ID associated with the command.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue order ID associated with the command.\n\n:returns: `VenueOrderId` or ``None``"""
    cdef readonly Quantity quantity
    """The updated quantity for the command.\n\n:returns: `Quantity` or ``None``"""
    cdef readonly Price price
    """The updated price for the command.\n\n:returns: `Price` or ``None``"""
    cdef readonly Price trigger_price
    """The updated trigger price for the command.\n\n:returns: `Price` or ``None``"""

    @staticmethod
    cdef ModifyOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(ModifyOrder obj)


cdef class CancelOrder(TradingCommand):
    cdef readonly ClientOrderId client_order_id
    """The client order ID associated with the command.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue order ID associated with the command.\n\n:returns: `VenueOrderId` or ``None``"""

    @staticmethod
    cdef CancelOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CancelOrder obj)


cdef class CancelAllOrders(TradingCommand):
    cdef readonly OrderSide order_side
    """The order side for the command.\n\n:returns: `OrderSide`"""

    @staticmethod
    cdef CancelAllOrders from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(CancelAllOrders obj)


cdef class BatchCancelOrders(TradingCommand):
    cdef readonly list cancels

    @staticmethod
    cdef BatchCancelOrders from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(BatchCancelOrders obj)


cdef class QueryOrder(TradingCommand):
    cdef readonly ClientOrderId client_order_id
    """The client order ID for the order to query.\n\n:returns: `ClientOrderId`"""
    cdef readonly VenueOrderId venue_order_id
    """The venue order ID for the order to query.\n\n:returns: `VenueOrderId` or ``None``"""

    @staticmethod
    cdef QueryOrder from_dict_c(dict values)

    @staticmethod
    cdef dict to_dict_c(QueryOrder obj)
