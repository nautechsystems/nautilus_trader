# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.pycapsule cimport PyCapsule_GetPointer
from libc.stdint cimport uint64_t

from nautilus_trader.core.data cimport Data
from nautilus_trader.core.rust.model cimport Data_t
from nautilus_trader.core.rust.model cimport Data_t_Tag
from nautilus_trader.model.data.bar cimport Bar
from nautilus_trader.model.data.book cimport OrderBookDelta
from nautilus_trader.model.data.tick cimport QuoteTick
from nautilus_trader.model.data.tick cimport TradeTick


# SAFETY: Do NOT deallocate the capsule here
cpdef list capsule_to_list(capsule):
    cdef CVec* data = <CVec*>PyCapsule_GetPointer(capsule, NULL)
    cdef Data_t* ptr = <Data_t*>data.ptr
    cdef list objects = []

    cdef uint64_t i
    for i in range(0, data.len):
        if ptr[i].tag == Data_t_Tag.DELTA:
            objects.append(OrderBookDelta.from_mem_c(ptr[i].delta))
        elif ptr[i].tag == Data_t_Tag.QUOTE:
            objects.append(QuoteTick.from_mem_c(ptr[i].quote))
        elif ptr[i].tag == Data_t_Tag.TRADE:
            objects.append(TradeTick.from_mem_c(ptr[i].trade))
        elif ptr[i].tag == Data_t_Tag.BAR:
            objects.append(Bar.from_mem_c(ptr[i].bar))

    return objects


cdef class DataType:
    """
    Represents a data type including metadata.

    Parameters
    ----------
    type : type
        The `Data` type of the data.
    metadata : dict
        The data types metadata.

    Raises
    ------
    ValueError
        If `type` is not a subclass of `Data`.
    TypeError
        If `metadata` contains a key or value which is not hashable.

    Warnings
    --------
    This class may be used as a key in hash maps throughout the system, thus
    the key and value contents of metadata must themselves be hashable.
    """

    def __init__(self, type type not None, dict metadata = None):  # noqa (shadows built-in type)
        if not issubclass(type, Data):
            raise TypeError("`type` was not a subclass of `Data`")

        self.type = type
        self.metadata = metadata or {}
        self.topic = self.type.__name__ + '.' + '.'.join([
            f'{k}={v if v is not None else "*"}' for k, v in self.metadata.items()
        ]) if self.metadata else self.type.__name__ + "*"

        self._key = frozenset(self.metadata.items())
        self._hash = hash((self.type, self._key))  # Assign hash for improved time complexity

    def __eq__(self, DataType other) -> bool:
        return self.type == other.type and self._key == other._key  # noqa

    def __lt__(self, DataType other) -> bool:
        return str(self) < str(other)

    def __le__(self, DataType other) -> bool:
        return str(self) <= str(other)

    def __gt__(self, DataType other) -> bool:
        return str(self) > str(other)

    def __ge__(self, DataType other) -> bool:
        return str(self) >= str(other)

    def __hash__(self) -> int:
        return self._hash

    def __str__(self) -> str:
        return f"{self.type.__name__}{self.metadata if self.metadata else ''}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}(type={self.type.__name__}, metadata={self.metadata})"


cdef class GenericData(Data):
    """
    Provides a generic data wrapper which includes data type information.

    Parameters
    ----------
    data_type : DataType
        The data type.
    data : Data
        The data object to wrap.
    """

    def __init__(
        self,
        DataType data_type not None,
        Data data not None,
    ):
        self.data_type = data_type
        self.data = data

    def __repr__(self) -> str:
        return f"{type(self).__name__}(data_type={self.data_type}, data={self.data})"

    @property
    def ts_event(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self.data.ts_event

    @property
    def ts_init(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self.data.ts_init
