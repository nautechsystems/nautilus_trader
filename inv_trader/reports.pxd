#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="reports.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.model.order cimport Order
from inv_trader.model.position cimport Position
from inv_trader.model.identifiers cimport OrderId, PositionId


cdef class ReportProvider:
    """
    Provides order fill and trade reports.
    """
    cpdef object get_order_fills_report(self, dict orders)
    cpdef object get_trades_report(self, dict positions)

    cdef void _add_order_to_df(self, OrderId order_id, Order order, dataframe)
    cdef void _add_position_to_df(self, PositionId position_id, Position position, dataframe)
