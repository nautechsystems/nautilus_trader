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

from libc.stdint cimport uint64_t

from nautilus_trader.common.queue cimport Queue
from nautilus_trader.execution.engine cimport ExecutionEngine
from nautilus_trader.execution.reports cimport ExecutionMassStatus
from nautilus_trader.execution.reports cimport ExecutionReport
from nautilus_trader.execution.reports cimport OrderStatusReport
from nautilus_trader.execution.reports cimport PositionStatusReport
from nautilus_trader.execution.reports cimport TradeReport
from nautilus_trader.model.events.order cimport OrderFilled
from nautilus_trader.model.identifiers cimport ClientOrderId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.orders.base cimport Order


cdef class LiveExecutionEngine(ExecutionEngine):
    cdef object _loop
    cdef object _cmd_queue_task
    cdef object _evt_queue_task
    cdef object _inflight_check_task
    cdef Queue _cmd_queue
    cdef Queue _evt_queue
    cdef uint64_t _inflight_check_threshold_ns

    cdef readonly bint is_running
    """If the execution engine is running.\n\n:returns: `bool`"""
    cdef readonly bint reconciliation
    """If the execution engine reconciliation is active at start-up.\n\n:returns: `bool`"""
    cdef readonly int reconciliation_lookback_mins
    """The lookback window for reconciliation on start-up (zero for max lookback).\n\n:returns: `int`"""
    cdef readonly int inflight_check_interval_ms
    """The in-flight check interval (milliseconds).\n\n:returns: `int`"""
    cdef readonly int inflight_check_threshold_ms
    """The in-flight check threshold (milliseconds).\n\n:returns: `int`"""

# -- COMMANDS -------------------------------------------------------------------------------------

    cpdef void reconcile_report(self, ExecutionReport report) except *
    cpdef void reconcile_mass_status(self, ExecutionMassStatus report) except *

# -- RECONCILIATION -------------------------------------------------------------------------------

    cdef bint _reconcile_report(self, ExecutionReport report) except *
    cdef bint _reconcile_mass_status(self, ExecutionMassStatus report) except *
    cdef bint _reconcile_order_report(self, OrderStatusReport report, list trades) except *
    cdef bint _reconcile_trade_report_single(self, TradeReport report) except *
    cdef bint _reconcile_trade_report(self, Order order, TradeReport report, Instrument instrument) except *
    cdef bint _reconcile_position_report(self, PositionStatusReport report) except *
    cdef bint _reconcile_position_report_netting(self, PositionStatusReport report) except *
    cdef bint _reconcile_position_report_hedging(self, PositionStatusReport report) except *
    cdef ClientOrderId _generate_client_order_id(self)
    cdef OrderFilled _generate_inferred_fill(self, Order order, OrderStatusReport report, Instrument instrument)
    cdef Order _generate_external_order(self, OrderStatusReport report)
    cdef void _generate_order_rejected(self, Order order, OrderStatusReport report) except *
    cdef void _generate_order_accepted(self, Order order, OrderStatusReport report) except *
    cdef void _generate_order_triggered(self, Order order, OrderStatusReport report) except *
    cdef void _generate_order_updated(self, Order order, OrderStatusReport report) except *
    cdef void _generate_order_canceled(self, Order order, OrderStatusReport report) except *
    cdef void _generate_order_expired(self, Order order, OrderStatusReport report) except *
    cdef void _generate_order_filled(self, Order order, TradeReport trade, Instrument instrument) except *
    cdef bint _should_update(self, Order order, OrderStatusReport report) except *
