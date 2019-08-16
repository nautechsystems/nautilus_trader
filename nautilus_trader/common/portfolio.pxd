# -------------------------------------------------------------------------------------------------
# <copyright file="portfolio.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.guid cimport GuidFactory
from nautilus_trader.common.logger cimport LoggerAdapter
from nautilus_trader.common.execution cimport ExecutionEngine
from nautilus_trader.model.events cimport AccountEvent
from nautilus_trader.model.objects cimport Money
from nautilus_trader.trade.performance cimport PerformanceAnalyzer


cdef class Portfolio:
    """
    Provides a trading portfolio.
    """
    cdef LoggerAdapter _log
    cdef Clock _clock
    cdef GuidFactory _guid_factory
    cdef ExecutionEngine _exec_engine
    cdef Money _account_capital
    cdef bint _account_initialized

    cdef readonly PerformanceAnalyzer analyzer

    cpdef void handle_transaction(self, AccountEvent event)
    cpdef void reset(self)
