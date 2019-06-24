#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="reports.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position


cdef class ReportProvider:
    """
    Provides order fill and trade reports.
    """
    cpdef object get_orders_report(self, dict orders)
    cpdef object get_order_fills_report(self, dict orders)
    cpdef object get_positions_report(self, dict positions)

    cdef dict _order_to_dict(self, Order order)
    cdef dict _position_to_dict(self, Position position)
