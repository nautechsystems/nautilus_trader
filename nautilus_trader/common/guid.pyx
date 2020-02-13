# -------------------------------------------------------------------------------------------------
# <copyright file="guid.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import uuid

from nautilus_trader.core.types cimport GUID


cdef class GuidFactory:
    """
    The base class for all GUID factories.
    """

    cpdef GUID generate(self):
        """
        Return a generated GUID.

        :return GUID.
        """
        # Raise exception if not overridden in implementation
        raise NotImplementedError("Method must be implemented in the subclass.")


cdef class TestGuidFactory(GuidFactory):
    """
    Provides a fake GUID factory for testing purposes.
    """

    def __init__(self):
        """
        Initializes a new instance of the TestGuidFactory class.
        """
        super().__init__()

        self._guid = GUID(uuid.uuid4())

    cpdef GUID generate(self):
        """
        Return the single test GUID instance.
        
        :return GUID.
        """
        return self._guid


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
