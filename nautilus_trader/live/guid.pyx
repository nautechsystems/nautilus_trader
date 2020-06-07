# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import uuid

from nautilus_trader.core.types cimport GUID
from nautilus_trader.common.guid cimport GuidFactory


cdef class LiveGuidFactory(GuidFactory):
    """
    Provides a GUID factory for live trading. Generates UUID4's.
    """

    def __init__(self):
        """
        Initializes a new instance of the LiveGuidFactory class.
        """
        super().__init__()

    cpdef GUID generate(self):
        """
        Return a generated UUID1.

        :return GUID.
        """
        return GUID(uuid.uuid4())
