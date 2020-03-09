# -------------------------------------------------------------------------------------------------
# <copyright file="identifiers.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import hashlib
from cpython.datetime cimport datetime

from nautilus_trader.core.datetime cimport format_iso8601


cdef class ClientId(Identifier):
    """
    Represents a unique client identifier
    """

    def __init__(self, str value not None):
        """
        Initializes a new instance of the ClientId class.

        :param value: The client identifier value.
        """
        super().__init__(value)


cdef class ServerId(Identifier):
    """
    Represents a unique server identifier
    """

    def __init__(self, str value not None):
        """
        Initializes a new instance of the ServerId class.

        :param value: The server identifier value.
        """
        super().__init__(value)


cdef class SessionId(Identifier):
    """
    Represents a unique network session identifier
    """

    def __init__(self, str value not None):
        """
        Initializes a new instance of the SessionId class.

        :param value: The session identifier value.
        """
        super().__init__(value)

    @staticmethod
    cdef SessionId create(ClientId client_id, datetime now, str secret):
        cdef bytes hashable = f'{client_id.value}-{format_iso8601(now)}-{secret}'.encode('utf-8')
        return SessionId(hashlib.sha256(hashable).hexdigest())

    @staticmethod
    def py_create(ClientId client_id, datetime now, str secret):
        return SessionId.create(client_id, now, secret)
