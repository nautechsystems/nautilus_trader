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


cdef class TypeKey:
    """
    Represents a generic type key.
    """

    def __init__(self, type type not None, dict definitions=None):  # noqa (shadows built-in type)
        """
        Initialize a new instance of the ``TypeKey`` class.

        Parameters
        ----------
        type : type
            The type of message.
        definitions : dict
            The type keys definitions.

        """
        if definitions is None:
            definitions = {}

        self.type = type
        self.key = frozenset(copy.deepcopy(definitions).items())
        self._hash = hash((self.type, self.key))  # Assign hash for improved time complexity

    def __eq__(self, TypeKey other) -> bool:
        return self.type == other.type and self.key == other.key

    def __hash__(self) -> int:
        return self._hash

    def __str__(self) -> str:
        return f"<{self.type.__name__}> {str(self.key)[10:-1]}"

    def __repr__(self) -> str:
        return f"{type(self).__name__}(type={self.type.__name__}, key={str(self.key)[10:-1]})"
