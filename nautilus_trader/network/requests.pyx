# -------------------------------------------------------------------------------------------------
# <copyright file="requests.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.message cimport Request
from nautilus_trader.model.identifiers cimport GUID


cdef class DataRequest(Request):
    """
    Represents a request for historical tick data.
    """

    def __init__(self,
                 dict query,
                 GUID request_id,
                 datetime request_timestamp):
        """
        Initializes a new instance of the TickDataRequest class.

        :param query: The data query.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)
        self.query = query
