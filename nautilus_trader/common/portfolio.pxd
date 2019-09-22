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
from nautilus_trader.model.objects cimport Money


cdef class Portfolio:
    cdef LoggerAdapter _log
    cdef Clock _clock
    cdef GuidFactory _guid_factory

    cpdef void reset(self)
