# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport GUID


cdef class GuidFactory:
    cpdef GUID generate(self)


cdef class TestGuidFactory(GuidFactory):
    cdef GUID _guid
