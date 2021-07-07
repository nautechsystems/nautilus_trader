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

import copy

from frozendict import frozendict

from libc.stdint cimport int64_t


cdef class Data:
    """
    The abstract base class for all data.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self, int64_t ts_event_ns, int64_t ts_recv_ns):
        """
        Initialize a new instance of the ``Data`` class.

        Parameters
        ----------
        ts_event_ns : int64
            The UNIX timestamp (nanoseconds) when data event occurred.
        ts_recv_ns : int64
            The UNIX timestamp (nanoseconds) when received by the Nautilus system.

        """
        # Design-time invariant: correct ordering of timestamps
        assert ts_recv_ns >= ts_event_ns
        self.ts_event_ns = ts_event_ns
        self.ts_recv_ns = ts_recv_ns

    def __repr__(self) -> str:
        return (f"{type(self).__name__}("
                f"ts_event_ns={self.ts_event_ns}, "
                f"ts_recv_ns{self.ts_recv_ns})")


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
            The ``Data`` type of the data.
        metadata : dict
            The data types metadata.

        """
        if metadata is None:
            metadata = {}

        self.type = data_type
        self.metadata = <dict>frozendict(copy.deepcopy(metadata))
        self._hash = hash((self.type, self.metadata))  # Assign hash for improved time complexity

    def __eq__(self, DataType other) -> bool:
        return self.type == other.type and self.metadata == other.metadata

    def __hash__(self) -> int:
        return self._hash

    def __str__(self) -> str:
        return f"<{self.type.__name__}> {str(self.metadata)[11:-1]}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}(type={self.type.__name__}, metadata={str(self.metadata)[11:-1]})"


cdef class GenericData(Data):
    """
    Provides a generic data wrapper which includes data type information.
    """

    def __init__(
        self,
        DataType data_type not None,
        Data data not None,
    ):
        """
        Initialize a new instance of the ``GenericData`` class.

        Parameters
        ----------
        data_type : DataType
            The data type.
        data : Data
            The data object to wrap.

        """
        super().__init__(data.ts_event_ns, data.ts_recv_ns)
        self.data_type = data_type
        self.data = data
