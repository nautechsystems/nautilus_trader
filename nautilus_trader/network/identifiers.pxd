# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.core.types cimport Identifier


cdef class ClientId(Identifier):
    pass


cdef class ServerId(Identifier):
    pass


cdef class SessionId(Identifier):
    pass
