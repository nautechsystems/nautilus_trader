# -------------------------------------------------------------------------------------------------
# <copyright file="test_core_concurrency.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.core.concurrency import ConcurrentDictionary, ObjectCache
from nautilus_trader.serialization.common import parse_symbol


class ConcurrentDictionaryTests(unittest.TestCase):

    def test_can_get_length_of_empty_dict(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary()

        # Act
        result = len(concurrent_dict)

        # Assert
        self.assertEqual(0, result)

    def test_can_get_length_of_filled_dict(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary()
        concurrent_dict['1'] = 1
        concurrent_dict['2'] = 2
        concurrent_dict['3'] = 3

        # Act
        result = len(concurrent_dict)

        # Assert
        self.assertEqual(3, result)

    def test_add_item_to_dict(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary()

        # Act
        concurrent_dict['key'] = 1

        # Assert
        self.assertEqual(1, concurrent_dict['key'])

    def test_get_when_item_exists(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary()
        concurrent_dict['key'] = 1

        # Act
        result = concurrent_dict.get('key')

        # Assert
        self.assertEqual(1, result)


class ObjectCacheTests(unittest.TestCase):

    def test_can_get_from_empty_cache(self):
        # Arrange
        cache = ObjectCache(parse_symbol)
        symbol = 'AUDUSD.FXCM'

        # Act
        result = cache.get(symbol)

        # Assert
        self.assertEqual(symbol, str(result))

    def test_can_get_from_cache(self):
        # Arrange
        cache = ObjectCache(parse_symbol)
        symbol = 'AUDUSD.FXCM'
        cache.get(symbol)

        # Act
        cache.get(symbol)
        result1 = cache.get(symbol)
        result2 = cache.get(symbol)

        # Assert
        self.assertEqual(symbol, str(result1))
        self.assertEqual(id(result1), id(result2))
