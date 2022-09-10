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

from cpython.object cimport PyObject

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.rust.core cimport UUID4_t
from nautilus_trader.core.rust.core cimport uuid4_eq
from nautilus_trader.core.rust.core cimport uuid4_free
from nautilus_trader.core.rust.core cimport uuid4_from_pystr
from nautilus_trader.core.rust.core cimport uuid4_hash
from nautilus_trader.core.rust.core cimport uuid4_new
from nautilus_trader.core.rust.core cimport uuid4_to_pystr


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

    def __init__(self, str value = None):
        if value is None:
            # Create a new UUID4 from Rust
            self._mem = uuid4_new()  # `UUID4_t` owned from Rust
        else:
            Condition.true(_UUID_REGEX.match(value), "value is not a valid UUID")
            self._mem = self._uuid4_from_pystr(value)

    cdef UUID4_t _uuid4_from_pystr(self, str value) except *:
        return uuid4_from_pystr(<PyObject *>value)  # `value` borrowed by Rust, `UUID4_t` owned from Rust

    cdef str to_str(self):
        return <str>uuid4_to_pystr(&self._mem)

    def __del__(self) -> None:
        uuid4_free(self._mem)  # `self._uuid4` moved to Rust (then dropped)

    def __getstate__(self):
        return self.to_str()

    def __setstate__(self, state):
        self._mem = self._uuid4_from_pystr(state)

    def __eq__(self, UUID4 other) -> bool:
        return uuid4_eq(&self._mem, &other._mem)

    def __hash__(self) -> int:
        return uuid4_hash(&self._mem)

    def __str__(self) -> str:
        return self.to_str()

    def __repr__(self) -> str:
        return f"{type(self).__name__}('{self}')"

    @property
    def value(self) -> str:
        return self.to_str()

    @staticmethod
    cdef UUID4 from_raw_c(UUID4_t raw):
        cdef UUID4 uuid4 = UUID4.__new__(UUID4)
        uuid4._mem = raw
        return uuid4
