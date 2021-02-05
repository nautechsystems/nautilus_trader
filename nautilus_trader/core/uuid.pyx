# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

# Refactored from the original CPython implementation found at
# https://github.com/python/cpython/blob/master/Lib/uuid.py
# Full credit to the original author 'Ka-Ping Yee <ping@zesty.ca>' and contributors.

# This type follows the standard CPython UUID class very closely however not exactly
# https://docs.python.org/3/library/uuid.html

"""
UUID objects (universally unique identifiers) according to RFC 4122.

This module provides immutable UUID objects (class UUID) and the function
for generating version 4 random UUIDs as specified in RFC 4122.

Typical usage:
    >>> import uuid
    # make a random UUID
    >>> x = uuid.uuid4()
    >>> str(x)
    '00010203-0405-0607-0809-0a0b0c0d0e0f'
"""

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid_rs cimport c_uuid_str_free
from nautilus_trader.core.uuid_rs cimport c_uuid_str_new


cdef class UUID:
    """
    Represent a UUID version 4 as specified in RFC 4122.
    UUID objects are immutable, hashable, and usable as dictionary keys.
    Converting a UUID to a string with str() yields something in the form
    '12345678-1234-1234-1234-123456789abc'.
    """

    def __cinit__(self, str value=None):
        if value is not None:
            if len(value.replace('-', '')) != 32:
                raise ValueError("badly formed hexadecimal UUID string")
            self.value = value
            return

        cdef char* raw_value = c_uuid_str_new()
        self.value = raw_value.decode("utf-8")
        c_uuid_str_free(raw_value)  # Return to rust to dealloc

    def __eq__(self, UUID other) -> bool:
        return self.value == other.value

    def __ne__(self, UUID other) -> bool:
        return self.value != other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    def __str__(self) -> str:
        return self.value

    @staticmethod
    cdef UUID from_str_c(str value):
        Condition.not_none(value, "value")

        return UUID(value)

    @staticmethod
    def from_str(str value):
        """
        Create a UUID parsed from the given hexadecimal UUID string value.

        Parameters
        ----------
        value : str
            The string value.

        Returns
        -------
        UUID

        Raises
        ------
        ValueError
            If value is badly formed (length != 32).

        """
        return UUID.from_str_c(value)


cpdef UUID uuid4():
    """Generate a random UUID version 4."""
    return UUID.__new__(UUID)
