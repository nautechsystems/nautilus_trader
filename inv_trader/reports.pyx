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
from inv_trader.model.identifiers cimport OrderId, PositionId


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
        order_fills_report = pd.DataFrame(columns=['timestamp', 'symbol', 'side', 'type', 'quantity', 'avg_price', 'slippage'] )
        order_fills_report.index.name = 'order_id'

        cdef dict filled_orders = {k:v for k, v in orders.items() if v.status == OrderStatus.FILLED}
        [self._add_order_to_df(k, v, order_fills_report) for k, v in filled_orders.items()]

        return order_fills_report

    cpdef object get_trades_report(self, dict positions):
        """
        Return a trades report dataframe.
        
        :param positions: The list of position objects.
        :return: pd.DataFrame.
        """
        trades_report = pd.DataFrame(columns=['symbol', 'direction', 'peak_quantity', 'entry_time', 'exit_time', 'avg_entry_price', 'avg_exit_price', 'points', 'return'] )
        trades_report.index.name = 'position_id'

        cdef dict completed_trades = {k:v for k, v in positions.items() if v.is_exited}
        [self._add_position_to_df(k, v, trades_report) for k, v in completed_trades.items()]

        return trades_report

    cdef void _add_order_to_df(self, OrderId order_id, Order order, dataframe):
        dataframe.loc[order_id.value] = {
            'timestamp': order.last_event.timestamp,
            'symbol': order.symbol.code,
            'side': order_side_string(order.side),
            'type': order_type_string(order.type),
            'quantity': order.quantity.value,
            'avg_price': order.average_price.value,
            'slippage': order.slippage
        }

    cdef void _add_position_to_df(self, PositionId position_id, Position position, dataframe):
        dataframe.loc[position_id.value] = {
            'symbol': position.symbol.code,
            'direction': order_side_string(position.entry_direction),
            'peak_quantity': position.peak_quantity.value,
            'entry_time': position.entry_time,
            'exit_time': position.exit_time,
            'avg_entry_price': position.average_entry_price.value,
            'avg_exit_price': position.average_exit_price.value,
            'points': position.points_realized,
            'return': position.return_realized
        }
