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


cdef class TypeKey:
    """
    Represents a generic immutable type key.

    The base class for all type keys. A type key is a type with a specification.
    """

    def __init__(self, type type not None, dict spec=None):  # noqa (shadows built-in type)
        """
        Initialize a new instance of the ``TypeKey`` class.

        Parameters
        ----------
        type : type
            The type of message.
        spec : dict
            The type keys specification.

        """
        if spec is None:
            spec = {}

        self.type = type
        self.key = frozenset(copy.deepcopy(spec).items())
        self._hash = hash((self.type, self.key))  # Assign hash for improved time complexity

    def __eq__(self, TypeKey other) -> bool:
        return self.type == other.type and self.key == other.key

    def __hash__(self) -> int:
        return self._hash


cdef class MessageType(TypeKey):
    """
    Represents an immutable message type including a header.
    """

    def __init__(self, type type not None, dict header=None):  # noqa (shadows built-in type)
        """
        Initialize a new instance of the ``MessageType`` class.

        Parameters
        ----------
        type : type
            The type of message.
        header : dict
            The message header.

        """
        if header is None:
            header = {}
        super().__init__(type=type, spec=header)

        self.header = <dict>frozendict(copy.deepcopy(header))

    def __str__(self) -> str:
        return f"<{self.type.__name__}> {str(self.header)[11:-1]}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}(type={self.type.__name__}, header={str(self.header)[11:-1]})"


cdef class DataType(TypeKey):
    """
    Represents an immutable data type including metadata.
    """

    def __init__(self, type type not None, dict metadata=None):  # noqa (shadows built-in type)
        """
        Initialize a new instance of the ``DataType`` class.

        Parameters
        ----------
        type : type
            The ``Data`` type of the data.
        metadata : dict
            The data types metadata.

        """
        if metadata is None:
            metadata = {}
        super().__init__(type=type, spec=metadata)

        self.metadata = <dict>frozendict(copy.deepcopy(metadata))

    def __str__(self) -> str:
        return f"<{self.type.__name__}> {str(self.metadata)[11:-1]}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}(type={self.type.__name__}, metadata={str(self.metadata)[11:-1]})"
