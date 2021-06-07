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

from libc.stdint cimport int64_t

from nautilus_trader.core.correctness cimport Condition


cdef class Data:
    """
    The abstract base class for all data.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, int64_t timestamp_origin_ns, int64_t timestamp_ns):
        """
        Initialize a new instance of the ``Data`` class.

        Parameters
        ----------
        timestamp_origin_ns : int64
            The UNIX timestamp (nanos) when originally occurred.
        timestamp_ns : int64
            The UNIX timestamp (nanos) when received by the Nautilus system.

        """
        # Design-time assert correct ordering of timestamps
        assert timestamp_ns >= timestamp_origin_ns
        self.timestamp_origin_ns = timestamp_origin_ns
        self.timestamp_ns = timestamp_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"timestamp_origin_ns={self.timestamp_origin_ns}, "
                f"timestamp_ns{self.timestamp_ns})")


cdef class DataType:
    """
    Represents a data type including its metadata.
    """

    def __init__(self, type data_type not None, dict metadata=None):
        """
        Initialize a new instance of the ``DataType`` class.

        Parameters
        ----------
        data_type : type
            The PyObject type of the data.
        metadata : dict
            The data types metadata.

        Raises
        ------
        TypeError
            If metadata contains a key or value which is not hashable.

        Warnings
        --------
        This class may be used as a key in hash maps throughout the system, thus
        the key and value contents of metadata must themselves be hashable.

        """
        if metadata is None:
            metadata = {}

        self._key = frozenset(metadata.items())
        self._hash = hash(self._key)  # Assign hash for improved time complexity
        self.type = data_type
        self.metadata = metadata

    def __eq__(self, DataType other) -> bool:
        return self.type == other.type and self.metadata == other.metadata

    def __ne__(self, DataType other) -> bool:
        return self.type != other.type or self.metadata != other.metadata

    def __hash__(self) -> int:
        return self._hash

    def __str__(self) -> str:
        return f"<{self.type.__name__}> {self.metadata}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}(type={self.type.__name__}, metadata={self.metadata})"


cdef class GenericData(Data):
    """
    Provides a generic data wrapper which includes data type information.
    """

    def __init__(
        self,
        DataType data_type not None,
        data not None,
        int64_t timestamp_origin_ns,
        int64_t timestamp_ns,
    ):
        """
        Initialize a new instance of the ``GenericData`` class.

        Parameters
        ----------
        data_type : DataType
            The data type.
        data : object
            The data object to wrap.
        timestamp_ns : int64
            The UNIX timestamp (nanos) of the data.

        Raises
        ------
        ValueError
            If type(data) is not of type data_type.type.

        """
        Condition.type(data, data_type.type, "data")
        super().__init__(timestamp_origin_ns, timestamp_ns)

        self.data_type = data_type
        self.data = data
