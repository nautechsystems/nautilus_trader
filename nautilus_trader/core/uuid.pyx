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

import os


cdef class UUID4:
    """
    Represents a UUID version 4.
    """

    def __init__(self, str value not None):
        """
        Initialize a new instance of the ``UUID4`` class.

        Parameters
        ----------
        value : str

        Raises
        ------
        ValueError
            If len(value) != 36.

        """
        if len(value) != 36:
            raise ValueError("value is not a 36-char string")

        self.value = value

    def __eq__(self, UUID4 other) -> bool:
        return self.value == other.value

    # Q. What's the value of being able to sort UUIDs?
    # A. Use them as keys in a B-Tree or similar mapping.
    def __lt__(self, UUID4 other) -> bool:
        return self.value < other.value

    def __gt__(self, UUID4 other) -> bool:
        return self.value > other.value

    def __le__(self, UUID4 other) -> bool:
        return self.value <= other.value

    def __ge__(self, UUID4 other) -> bool:
        return self.value >= other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    def __str__(self) -> str:
        return self.value


cpdef UUID4 uuid4():
    """
    Generate a random UUID (universally unique identifier) version 4 as
    specified in RFC 4122.

    Returns
    -------
    UUID4

    """
    # Construct hex string from a random integer value
    cdef str hex_str = "%032x" % int.from_bytes(os.urandom(16), byteorder="big")

    # Parse final UUID value
    return UUID4(f"{hex_str[:8]}-{hex_str[8:12]}-{hex_str[12:16]}-{hex_str[16:20]}-{hex_str[20:]}")
