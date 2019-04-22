#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="reports.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

import pandas as pd

from inv_trader.enums.order_status cimport OrderStatus
from inv_trader.enums.order_side cimport order_side_string
from inv_trader.enums.order_type cimport order_type_string
from inv_trader.model.order cimport Order
from inv_trader.model.position cimport Position


cdef class ReportProvider:
    """
    Provides order fill and trade reports.
    """

    def __init__(self):
        """
        Initializes a new instance of the ReportProvider class.
        """

    cpdef object get_order_fills_report(self, dict orders):
        """
        Return an order fill report dataframe.
        
        :param orders: The list of order objects.
        :return: pd.DataFrame.
        """
        cdef list filled_orders = [self._order_to_dict(o) for o in orders.values() if o.status == OrderStatus.FILLED]

        return pd.DataFrame(data=filled_orders).set_index('order_id')

    cpdef object get_trades_report(self, dict positions):
        """
        Return a trades report dataframe.
        
        :param positions: The list of position objects.
        :return: pd.DataFrame.
        """
        cdef list trades = [self._position_to_dict(p) for p in positions.values() if p.is_exited]

        return pd.DataFrame(data=trades).set_index('position_id')

    cdef dict _order_to_dict(self, Order order):
        return {'order_id': order.id.value,
                'timestamp': order.last_event.timestamp,
                'symbol': order.symbol.code,
                'side': order_side_string(order.side),
                'type': order_type_string(order.type),
                'quantity': order.quantity.value,
                'avg_price': order.average_price.value,
                'slippage': order.slippage}

    cdef dict _position_to_dict(self, Position position):
        return {'position_id': position.id.value,
                'symbol': position.symbol.code,
                'direction': order_side_string(position.entry_direction),
                'peak_quantity': position.peak_quantity.value,
                'entry_time': position.entry_time,
                'exit_time': position.exit_time,
                'avg_entry_price': position.average_entry_price.value,
                'avg_exit_price': position.average_exit_price.value,
                'points': position.points_realized,
                'return': position.return_realized}
