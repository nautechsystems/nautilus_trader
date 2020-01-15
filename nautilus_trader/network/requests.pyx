# -------------------------------------------------------------------------------------------------
# <copyright file="requests.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.core.types cimport GUID
from nautilus_trader.core.message cimport Request


cdef class DataRequest(Request):
    """
    Represents a request for data.
    """

    def __init__(self,
                 dict query not None,
                 GUID request_id not None,
                 datetime request_timestamp not None):
        """
        Initializes a new instance of the DataRequest class.

        :param query: The data query.
        :param request_id: The request identifier.
        :param request_timestamp: The request timestamp.
        """
        super().__init__(request_id, request_timestamp)
        self.query = query
