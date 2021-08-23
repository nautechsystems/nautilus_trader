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

# This type follows the standard CPython UUID4 class very closely however not exactly
# https://docs.python.org/3/library/uuid.html

"""
UUID4 objects (universally unique identifiers) according to RFC 4122.
This module provides immutable UUID4 objects and the function
for generating version 4 random UUIDs as specified in RFC 4122.
"""

import os


cpdef UUID4 uuid4():
    """Generate a random UUID version 4."""
    # Construct hex string from random integer value
    cdef str hex_str = "%032x" % int.from_bytes(os.urandom(16), byteorder="big")

    # # Parse final UUID value
    return f"{hex_str[:8]}-{hex_str[8:12]}-{hex_str[12:16]}-{hex_str[16:20]}-{hex_str[20:]}"
