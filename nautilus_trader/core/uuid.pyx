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

import os

from nautilus_trader.core.correctness cimport Condition


cdef class UUID:
    """
    Represent a UUID version 4 as specified in RFC 4122.
    UUID objects are immutable, hashable, and usable as dictionary keys.
    Converting a UUID to a string with str() yields something in the form
    '12345678-1234-1234-1234-123456789abc'.
    """

    def __init__(self, bytes value not None):
        """
        Initialize a new instance of the `UUID` class.

        Parameters
        ----------
        value : bytes
            The 16 bytes value to generate the UUID with.

        Raises
        ------
        ValueError
            If value length != 16.

        """
        if len(value) != 16:
            raise ValueError("bytes is not a 16-char string")

        # Set UUID 128-bit integer value
        self.int_val = int.from_bytes(value, byteorder="big")
        assert 0 <= self.int_val < 1 << 128, "int is out of range (need a 128-bit value)"

        # Construct hex string from integer value
        cdef str hex_str = '%032x' % self.int_val

        # Parse final UUID value
        self.value = '%s-%s-%s-%s-%s' % (hex_str[:8], hex_str[8:12], hex_str[12:16], hex_str[16:20], hex_str[20:])

    def __eq__(self, UUID other) -> bool:
        return self.value == other.value

    def __ne__(self, UUID other) -> bool:
        return self.value != other.value

    # Q. What's the value of being able to sort UUIDs?
    # A. Use them as keys in a B-Tree or similar mapping.
    def __lt__(self, UUID other) -> bool:
        return self.int_val < other.int_val

    def __gt__(self, UUID other) -> bool:
        return self.int_val > other.int_val

    def __le__(self, UUID other) -> bool:
        return self.int_val <= other.int_val

    def __ge__(self, UUID other) -> bool:
        return self.int_val >= other.int_val

    def __hash__(self) -> int:
        return hash(self.value)

    def __int__(self) -> int:
        return self.int_val

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    def __str__(self) -> str:
        return self.value

    @staticmethod
    cdef UUID from_str_c(str value):
        Condition.not_none(value, "value")

        cdef hex_str = value.replace('-', '')
        if len(hex_str) != 32:
            raise ValueError("badly formed hexadecimal UUID string")

        return UUID(bytes.fromhex(hex_str))

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

    @property
    def bytes(self):
        """
        The UUID as a 16-byte string (containing the six integer fields in
        big-endian byte order).

        Returns
        -------
        str

        """
        return self.int_val.to_bytes(16, byteorder='big')

    @property
    def bytes_le(self):
        """
        The UUID as a 16-byte string (with time_low, time_mid,
        and time_hi_version in little-endian byte order).

        Returns
        -------
        str

        """
        cdef bytes bytes_val = self.bytes
        return bytes_val[4 - 1:: - 1] \
            + bytes_val[6 - 1:4 - 1:-1] \
            + bytes_val[8 - 1:6 - 1:-1] \
            + bytes_val[8:]

    @property
    def fields(self):
        """
        A tuple of the six integer fields of the UUID, which are also available
        as six individual attributes and two derived attributes.

        Returns
        -------
        tuple

        """
        return (
            self.time_low,
            self.time_mid,
            self.time_hi_version,
            self.clock_seq_hi_variant,
            self.clock_seq_low,
            self.node,
        )

    @property
    def time_low(self):
        """
        The first 32 bits of the UUID.

        Returns
        -------
        int

        """
        return self.int_val >> 96

    @property
    def time_mid(self):
        """
        The next 16 bits of the UUID.

        Returns
        -------
        int

        """
        return (self.int_val >> 80) & 0xffff

    @property
    def time_hi_version(self):
        """
        The next 16 bits of the UUID.

        Returns
        -------
        int

        """
        return (self.int_val >> 64) & 0xffff

    @property
    def clock_seq_hi_variant(self):
        """
        The next 8 bits of the UUID.

        Returns
        -------
        int

        """
        return (self.int_val >> 56) & 0xff

    @property
    def clock_seq_low(self):
        """
        The 60-bit timestamp.

        Returns
        -------
        int

        """
        return (self.int_val >> 48) & 0xff

    @property
    def time(self):
        """
        The 60-bit timestamp.

        Returns
        -------
        int

        """
        return ((self.time_hi_version & 0x0fff) << 48) | (self.time_mid << 32) | self.time_low

    @property
    def clock_seq(self):
        """
        The 14-bit sequence number.

        Returns
        -------
        int

        """
        return ((self.clock_seq_hi_variant & 0x3f) << 8) | self.clock_seq_low

    @property
    def node(self):
        """
        The last 48 bits of the UUID.

        Returns
        -------
        int

        """
        return self.int_val & 0xffffffffffff

    @property
    def hex(self):
        """
        The UUID as a 32-character hexadecimal string.

        Returns
        -------
        str

        """
        return '%032x' % self.int_val

    @property
    def urn(self):
        """
        The UUID as a URN as specified in RFC 4122.

        Returns
        -------
        str

        """
        return 'urn:uuid:' + str(self)


cpdef UUID uuid4():
    """Generate a random UUID version 4."""
    return UUID(value=os.urandom(16))
