# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import re

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport uuid4_free
from nautilus_trader.core.rust.core cimport uuid4_from_cstring
from nautilus_trader.core.rust.core cimport uuid4_new
from nautilus_trader.core.rust.core cimport uuid4_to_cstring
from nautilus_trader.core.string cimport cstring_to_pystr
from nautilus_trader.core.string cimport pystr_to_cstring


_UUID_REGEX = re.compile("[0-F]{8}-([0-F]{4}-){3}[0-F]{12}", re.I)


cdef class UUID4:
    """
    Represents a pseudo-random UUID (universally unique identifier) version 4
    based on a 128-bit label as specified in RFC 4122.

    Parameters
    ----------
    value : str, optional
        The UUID value. If ``None`` then a value will be generated.

    Raises
    ------
    ValueError
        If `value` is not ``None`` and not a valid UUID.

    References
    ----------
    https://en.wikipedia.org/wiki/Universally_unique_identifier
    """

    def __init__(self, str value=None):
        if value is None:
            # Create a new UUID4 from Rust
            self._uuid4 = uuid4_new()  # `UUID4_t` owned from Rust
            self.value = cstring_to_pystr(uuid4_to_cstring(&self._uuid4))
        else:
            Condition.true(_UUID_REGEX.match(value), "value is not a valid UUID")
            self._uuid4 = self._uuid4_from_pystring(value)
            self.value = value

    cdef UUID4_t _uuid4_from_pystring(self, str value) except *:
        return uuid4_from_cstring(pystr_to_cstring(value))  # `value` moved to Rust, `UUID4_t` owned from Rust

    def __getstate__(self):
        return self.value

    def __setstate__(self, value):
        self._uuid4 = self._uuid4_from_pystring(value)

    def __eq__(self, UUID4 other) -> bool:
        return self.value == other.value

    def __hash__(self) -> int:
        return hash(self.value)

    def __str__(self) -> str:
        return self.value

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self.value}')"

    def __del__(self) -> None:
        uuid4_free(self._uuid4)  # `self._uuid4` moved to Rust (then dropped)
