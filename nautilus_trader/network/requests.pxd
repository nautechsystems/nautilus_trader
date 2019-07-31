# -------------------------------------------------------------------------------------------------
# <copyright file="requests.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.message cimport Request


cdef class DataRequest(Request):
    """
    Represents a request for historical tick data.
    """
    cdef readonly dict query
