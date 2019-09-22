# -------------------------------------------------------------------------------------------------
# <copyright file="trader.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.common.execution cimport ExecutionEngine
from nautilus_trader.common.portfolio cimport Portfolio
from nautilus_trader.model.identifiers cimport TraderId, AccountId
from nautilus_trader.trade.reports cimport ReportProvider


cdef class Trader:
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef LoggerAdapter _log
    cdef DataClient _data_client
    cdef ExecutionEngine _exec_engine
    cdef ReportProvider _report_provider

    cdef readonly TraderId id
    cdef readonly AccountId account_id
    cdef readonly Portfolio portfolio
    cdef readonly list strategies
    cdef readonly list started_datetimes
    cdef readonly list stopped_datetimes
    cdef readonly bint is_running

    cpdef void initialize_strategies(self, list strategies) except *
    cpdef void start(self) except *
    cpdef void stop(self) except *
    cpdef void save(self) except *
    cpdef void load(self) except *
    cpdef void reset(self) except *
    cpdef void dispose(self) except *

    cpdef void account_inquiry(self) except *

    cpdef dict strategy_status(self)
    cpdef object get_orders_report(self)
    cpdef object get_order_fills_report(self)
    cpdef object get_positions_report(self)
