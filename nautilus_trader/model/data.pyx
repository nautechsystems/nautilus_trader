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

from nautilus_trader.core.correctness cimport Condition


cdef class Data:
    """
    The abstract base class for all data.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        datetime timestamp not None,
        double unix_timestamp=0,
    ):
        """
        Initialize a new instance of the `Data` class.

        Parameters
        ----------
        timestamp : datetime
            The data timestamp (UTC).
        unix_timestamp : double, optional
            The data object to wrap.

        Raises
        ------
        ValueError
            If type(data) is not of type data_type.type.

        """
        if unix_timestamp == 0:
            unix_timestamp = timestamp.timestamp()

        self.timestamp = timestamp
        self.unix_timestamp = unix_timestamp


cdef class DataType:
    """
    Represents a data type including its metadata.
    """

    def __init__(self, type data_type not None, dict metadata=None):
        """
        Initialize a new instance of the `DataType` class.

        Parameters
        ----------
        data_type : type
            The PyObject type of the data.
        metadata : dict
            The data types metadata.

        Warnings
        --------
        This class may be used as a key in hash maps throughout the system, thus
        the key and value contents of metadata must themselves be hashable.

        Raises
        ------
        TypeError
            If metadata contains a key or value which is not hashable.

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
        datetime timestamp not None,
        double unix_timestamp=0,
    ):
        """
        Initialize a new instance of the `GenericData` class.

        Parameters
        ----------
        data_type : DataType
            The data type.
        data : object
            The data object to wrap.
        timestamp : datetime
            The data timestamp (UTC).
        unix_timestamp : double, optional
            The data Unix timestamp (seconds).

        Raises
        ------
        ValueError
            If type(data) is not of type data_type.type.

        """
        Condition.type(data, data_type.type, "data")
        super().__init__(timestamp, unix_timestamp)

        self.data_type = data_type
        self.data = data
