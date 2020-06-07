# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.core.cache import ObjectCache
from nautilus_trader.model.identifiers import Symbol


class ObjectCacheTests(unittest.TestCase):

    def test_cache_initialization(self):
        # Arrange
        cache = ObjectCache(Symbol, Symbol.py_from_string)

        # Act
        # Assert
        self.assertEqual(str, cache.type_key)
        self.assertEqual(Symbol, cache.type_value)
        self.assertEqual([], cache.keys())

    def test_can_get_from_empty_cache(self):
        # Arrange
        cache = ObjectCache(Symbol, Symbol.py_from_string)
        symbol = 'AUDUSD.FXCM'

        # Act
        result = cache.get(symbol)

        # Assert
        self.assertEqual(symbol, str(result))
        self.assertEqual(['AUDUSD.FXCM'], cache.keys())

    def test_can_get_from_cache(self):
        # Arrange
        cache = ObjectCache(Symbol, Symbol.py_from_string)
        symbol = 'AUDUSD.FXCM'
        cache.get(symbol)

        # Act
        cache.get(symbol)
        result1 = cache.get(symbol)
        result2 = cache.get(symbol)

        # Assert
        self.assertEqual(symbol, str(result1))
        self.assertEqual(id(result1), id(result2))
        self.assertEqual(['AUDUSD.FXCM'], cache.keys())

    def test_can_clear_cache(self):
        # Arrange
        cache = ObjectCache(Symbol, Symbol.py_from_string)
        symbol = 'AUDUSD.FXCM'
        cache.get(symbol)

        # Act
        cache.clear()

        # Assert
        self.assertEqual([], cache.keys())
