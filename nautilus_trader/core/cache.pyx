# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

"""
Defines a generic object cache.

The intended use is to reduce the creation of duplicate objects such as
identifiers, which represent the same thing.
"""

from nautilus_trader.core.correctness cimport Condition


cdef class ObjectCache:
    """
    Provides an object cache with strings as keys.
    """

    def __init__(self, type type_value not None, parser not None: callable):
        """
        Initialize a new instance of the `ObjectCache` class.

        Parameters
        ----------
        type_value : type
            The type of the cached objects.
        parser : callable
            The parser function to created an object for the cache.

        """
        self._type_key = str
        self._type_value = type_value
        self._cache = {}
        self._parser = parser

    @property
    def type_key(self):
        """
        The caches key type.

        Returns
        -------
        type


        """
        return self._type_key

    @property
    def type_value(self):
        """
        The caches value type.

        Returns
        -------
        type

        """
        return self._type_value

    cpdef list keys(self):
        """
        The keys held in the cache.

        Returns
        -------
        list[str]

        """
        return list(self._cache.keys())

    cpdef object get(self, str key):
        """
        Return the cached object for the given key otherwise cache and return
        the parsed key.

        Parameters
        ----------
        key : str
            The key of the cached object to get.

        Returns
        -------
        object
            The cached object.

        """
        Condition.valid_string(key, "key")

        parsed = self._cache.get(key)
        if parsed is None:
            parsed = self._parser(key)
            self._cache[key] = parsed

        return parsed

    cpdef void clear(self) except *:
        """
        Clear all cached values.
        """
        self._cache.clear()
