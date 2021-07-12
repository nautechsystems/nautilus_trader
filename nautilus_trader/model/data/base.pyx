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

from nautilus_trader.core.type cimport DataType


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
