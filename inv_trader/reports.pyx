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
        order_fill_report = pd.DataFrame(columns=['timestamp', 'symbol', 'side', 'type', 'quantity', 'avg_price', 'slippage'] )
        order_fill_report.index_name = 'order_id'

        cdef OrderId order_id
        cdef Order order
        for order_id, order in orders.items():
            if order.status == OrderStatus.FILLED:
                if order_id not in order_fill_report.index:
                    order_fill_report.loc[order_id.value] = 0
                order_fill_report.loc[order_id.value]['timestamp'] = order.filled_timestamp
                order_fill_report.loc[order_id.value]['symbol'] = str(order.symbol)
                order_fill_report.loc[order_id.value]['side'] = order_side_string(order.side)
                order_fill_report.loc[order_id.value]['type'] = order_type_string(order.type)
                order_fill_report.loc[order_id.value]['quantity'] = order.quantity.value
                order_fill_report.loc[order_id.value]['avg_price'] = order.average_price.value
                order_fill_report.loc[order_id.value]['slippage'] = order.slippage

        return order_fill_report

    cpdef object get_trades_report(self, dict positions):
        """
        Return a trades report dataframe.
        
        :param positions: The list of position objects.
        :return: pd.DataFrame.
        """
        trades_report = pd.DataFrame(columns=['symbol', 'direction', 'peak_quantity', 'entry_time', 'exit_time', 'avg_entry_price', 'avg_exit_price', 'points', 'return'] )
        trades_report.index_name = 'position_id'

        cdef PositionId position_id
        cdef Position position
        for position_id, position in positions.items():
            if position.is_exited:
                if position_id not in trades_report.index:
                    trades_report.loc[position_id.value] = 0
                trades_report.loc[position_id.value]['symbol'] = str(position.symbol)
                trades_report.loc[position_id.value]['direction'] = order_side_string(position.entry_direction)
                trades_report.loc[position_id.value]['peak_quantity'] = position.peak_quantity.value
                trades_report.loc[position_id.value]['entry_time'] = position.entry_time
                trades_report.loc[position_id.value]['exit_time'] = position.exit_time
                trades_report.loc[position_id.value]['avg_entry_price'] = position.average_entry_price.value
                trades_report.loc[position_id.value]['avg_exit_price'] = position.average_exit_price.value
                trades_report.loc[position_id.value]['points'] = position.points_realized
                trades_report.loc[position_id.value]['return'] = position.return_realized

        return trades_report
