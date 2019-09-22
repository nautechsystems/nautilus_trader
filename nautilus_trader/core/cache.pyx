# -------------------------------------------------------------------------------------------------
# <copyright file="cache.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cdef class ObjectCache:
    """
    Provides an object cache with strings as keys.
    """

    def __init__(self, type type_value, parser):
        """
        Initializes a new instance of the ObjectCache class.
        """
        assert type_value is not None, 'The type_value type cannot be None'
        assert type_value is not type(None), 'The type_value type cannot be NoneType'

        self.type_key = str
        self.type_value = type_value
        self._cache = {}
        self._parser = parser

    cpdef object get(self, str key):
        """
        Return the cached object for the given key otherwise cache and return
        the parsed key.

        :param key: The key to check.
        :return object.
        """
        parsed = self._cache.get(key, None)

        if parsed is None:
            parsed = self._parser(key)
            self._cache[key] = parsed

        return parsed

    cpdef list keys(self):
        """
        Return a list of the keys held in the cache.
        
        :return: List[str].
        """
        return list(self._cache.keys())

    cpdef void clear(self):
        """
        Clears all cached values.
        """
        self._cache.clear()
