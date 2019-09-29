# -------------------------------------------------------------------------------------------------
# <copyright file="reports.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.model.events cimport AccountStateEvent
from nautilus_trader.model.order cimport Order
from nautilus_trader.model.position cimport Position


cdef class ReportProvider:
    cpdef object generate_orders_report(self, dict orders)
    cpdef object generate_order_fills_report(self, dict orders)
    cpdef object generate_positions_report(self, dict positions)
    cpdef object generate_account_report(self, list events, datetime start=*, datetime end=*)

    cdef dict _order_to_dict(self, Order order)
    cdef dict _position_to_dict(self, Position position)
    cdef dict _account_state_to_dict(self, AccountStateEvent event)
