# -------------------------------------------------------------------------------------------------
# <copyright file="reports.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import pandas as pd

from typing import Dict

from nautilus_trader.model.c_enums.order_state cimport OrderState
from nautilus_trader.model.c_enums.order_side cimport order_side_to_string
from nautilus_trader.model.c_enums.order_type cimport order_type_to_string
from nautilus_trader.model.identifiers cimport OrderId, PositionId
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position


cdef class ReportProvider:
    """
    Provides order fill and trade reports.
    """

    def __init__(self):
        """
        Initializes a new instance of the ReportProvider class.
        """

    cpdef object get_orders_report(self, dict orders: Dict[OrderId, Order]):
        """
        Return an orders report dataframe.
        
        :param orders: The dictionary of order_ids and order objects.
        :return pd.DataFrame.
        """
        if len(orders) == 0:
            return pd.DataFrame()

        cdef list orders_all = [self._order_to_dict(o) for o in orders.values()]

        return pd.DataFrame(data=orders_all).set_index('order_id')

    cpdef object get_order_fills_report(self, dict orders: Dict[OrderId, Order]):
        """
        Return an order fills report dataframe.
        
        :param orders: The dictionary of order_ids and order objects.
        :return pd.DataFrame.
        """
        if len(orders) == 0:
            return pd.DataFrame()

        cdef list filled_orders = [self._order_to_dict(o) for o in orders.values() if o.state == OrderState.FILLED]

        return pd.DataFrame(data=filled_orders).set_index('order_id')

    cpdef object get_positions_report(self, dict positions: Dict[PositionId, Position]):
        """
        Return a positions report dataframe.
        
        :param positions: The dictionary of position_ids and objects.
        :return pd.DataFrame.
        """
        if len(positions) == 0:
            return pd.DataFrame()

        cdef list trades = [self._position_to_dict(p) for p in positions.values() if p.is_closed]

        return pd.DataFrame(data=trades).set_index('position_id')

    cdef dict _order_to_dict(self, Order order):
        return {'order_id': order.id.value,
                'timestamp': order.last_event.timestamp,
                'symbol': order.symbol.code,
                'side': order_side_to_string(order.side),
                'type': order_type_to_string(order.type),
                'quantity': order.quantity.value,
                'avg_price': order.average_price.value,
                'slippage': order.slippage}

    cdef dict _position_to_dict(self, Position position):
        return {'position_id': position.id.value,
                'symbol': position.symbol.code,
                'direction': order_side_to_string(position.entry_direction),
                'peak_quantity': position.peak_quantity.value,
                'opened_time': position.opened_time,
                'closed_time': position.closed_time,
                'avg_open_price': position.average_open_price,
                'avg_close_price': position.average_close_price,
                'realized_points': position.realized_points,
                'realized_return': position.realized_return}
