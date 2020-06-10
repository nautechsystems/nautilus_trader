# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
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
