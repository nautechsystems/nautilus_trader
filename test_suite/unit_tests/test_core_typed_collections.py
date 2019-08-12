# -------------------------------------------------------------------------------------------------
# <copyright file="test_core_typed_collections.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.core.typed_collections import (
    TypedList,
    TypedDictionary,
    ConcurrentDictionary,
    ObjectCache
)
from nautilus_trader.model.objects import Symbol
from nautilus_trader.serialization.common import parse_symbol


class TypedListTests(unittest.TestCase):

    def test_can_be_initialized_with_strong_typing(self):
        # Arrange
        typed_list = TypedList(str)

        # Act
        # Assert
        self.assertEqual(str, typed_list.type_value)

    def test_append_adds_item_to_list(self):
        # Arrange
        typed_list = TypedList(str)
        item = 'abc'

        # Act
        typed_list.append(item)

        # Assert
        self.assertEqual(1, len(typed_list))
        self.assertEqual(item, typed_list[0])

    def test_item_can_be_deleted(self):
        # Arrange
        typed_list = TypedList(str)
        item = 'abc'
        typed_list.append(item)

        # Act
        del typed_list[0]

        # Assert
        self.assertEqual(0, len(typed_list))

    def test_len_when_empty_returns_zero(self):
        # Arrange
        typed_list = TypedList(str)

        # Act
        result = len(typed_list)

        # Assert
        self.assertEqual(0, result)

    def test_len_with_some_elements_returns_correct_length(self):
        # Arrange
        typed_list = TypedList(str)
        typed_list.append('1')
        typed_list.append('2')
        typed_list.append('3')

        # Act
        result = len(typed_list)

        # Assert
        self.assertEqual(3, result)


class TypedDictionaryTests(unittest.TestCase):

    def test_can_be_initialized_with_strong_typing(self):
        # Arrange
        typed_list = TypedList(str)

        # Act
        # Assert
        self.assertEqual(str, typed_list.type_value)

    def test_len_when_empty_returns_zero(self):
        # Arrange
        dictionary = TypedDictionary(str, int)

        # Act
        result = len(dictionary)

        # Assert
        self.assertEqual(0, result)

    def test_len_with_some_elements_returns_correct_length(self):
        # Arrange
        dictionary = TypedDictionary(str, int)
        dictionary['1'] = 1
        dictionary['2'] = 2
        dictionary['3'] = 3

        # Act
        result = len(dictionary)

        # Assert
        self.assertEqual(3, result)

    def test_add_item_to_dict(self):
        # Arrange
        dictionary = TypedDictionary(str, int)

        # Act
        dictionary['key'] = 1

        # Assert
        self.assertEqual(1, dictionary['key'])

    def test_get_when_item_exists(self):
        # Arrange
        dictionary = TypedDictionary(str, int)
        dictionary['key'] = 1

        # Act
        result = dictionary.get('key')

        # Assert
        self.assertEqual(1, result)

    def test_get_with_empty_dict_returns_none(self):
        # Arrange
        dictionary = TypedDictionary(str, int)

        # Act
        result = dictionary.get('key')

        # Assert
        self.assertEqual(None, result)

    def test_setdefault_with_empty_dict_returns_none(self):
        # Arrange
        dictionary = TypedDictionary(str, int)

        # Act
        result = dictionary.setdefault('key')

        # Assert
        self.assertEqual(None, result)

    def test_setdefault_when_key_exists_returns_value(self):
        # Arrange
        dictionary = TypedDictionary(str, int)
        dictionary['key'] = 1

        # Act
        result = dictionary.setdefault('key')

        # Assert
        self.assertEqual(1, result)

    def test_pop_dict_when_key_exists_returns_item(self):
        # Arrange
        dictionary = TypedDictionary(str, int)
        dictionary['key'] = 1

        # Act
        result = dictionary.pop('key')

        # Assert
        self.assertEqual(1, result)
        self.assertEqual(0, len(dictionary))

    def test_pop_dict_when_no_key_exists_returns_default(self):
        # Arrange
        dictionary = TypedDictionary(str, int)
        dictionary['key'] = 1

        # Act
        result = dictionary.pop('another')

        # Assert
        self.assertEqual(None, result)
        self.assertEqual(1, len(dictionary))

    def test_popitem(self):
        # Arrange
        dictionary = TypedDictionary(str, int)
        dictionary['key'] = 1

        # Act
        result = dictionary.popitem()

        # Assert
        self.assertEqual(('key', 1), result)
        self.assertEqual(0, len(dictionary))

    def test_copy_dict(self):
        # Arrange
        dictionary = TypedDictionary(str, int)
        dictionary['key'] = 1

        # Act
        result = dictionary.copy()

        # Assert
        self.assertEqual({'key': 1}, result)

    def test_clear_dict(self):
        # Arrange
        dictionary = TypedDictionary(str, int)
        dictionary['key'] = 1

        # Act
        dictionary.clear()

        # Assert
        self.assertEqual(0, len(dictionary))


class ConcurrentDictionaryTests(unittest.TestCase):

    def test_can_get_length_of_empty_dict(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)

        # Act
        result = len(concurrent_dict)

        # Assert
        self.assertEqual(str, concurrent_dict.type_key)
        self.assertEqual(int, concurrent_dict.type_value)
        self.assertEqual(0, result)

    def test_can_get_length_of_filled_dict(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)
        concurrent_dict['1'] = 1
        concurrent_dict['2'] = 2
        concurrent_dict['3'] = 3

        # Act
        result = len(concurrent_dict)

        # Assert
        self.assertEqual(3, result)

    def test_add_item_to_dict(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)

        # Act
        concurrent_dict['key'] = 1

        # Assert
        self.assertEqual(1, concurrent_dict['key'])

    def test_get_when_item_exists(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)
        concurrent_dict['key'] = 1

        # Act
        result = concurrent_dict.get('key')

        # Assert
        self.assertEqual(1, result)

    def test_get_with_empty_dict_returns_none(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)

        # Act
        result = concurrent_dict.get('key')

        # Assert
        self.assertEqual(None, result)

    def test_setdefault_with_empty_dict_returns_none(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)

        # Act
        result = concurrent_dict.setdefault('key')

        # Assert
        self.assertEqual(None, result)

    def test_setdefault_when_key_exists_returns_value(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)
        concurrent_dict['key'] = 1

        # Act
        result = concurrent_dict.setdefault('key')

        # Assert
        self.assertEqual(1, result)

    def test_pop_dict_when_key_exists_returns_item(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)
        concurrent_dict['key'] = 1

        # Act
        result = concurrent_dict.pop('key')

        # Assert
        self.assertEqual(1, result)
        self.assertEqual(0, len(concurrent_dict))

    def test_pop_dict_when_no_key_exists_returns_default(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)
        concurrent_dict['key'] = 1

        # Act
        result = concurrent_dict.pop('another')

        # Assert
        self.assertEqual(None, result)
        self.assertEqual(1, len(concurrent_dict))

    def test_popitem(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)
        concurrent_dict['key'] = 1

        # Act
        result = concurrent_dict.popitem()

        # Assert
        self.assertEqual(('key', 1), result)
        self.assertEqual(0, len(concurrent_dict))

    def test_copy_dict(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)
        concurrent_dict['key'] = 1

        # Act
        result = concurrent_dict.copy()

        # Assert
        self.assertEqual({'key': 1}, result)

    def test_clear_dict(self):
        # Arrange
        concurrent_dict = ConcurrentDictionary(str, int)
        concurrent_dict['key'] = 1

        # Act
        concurrent_dict.clear()

        # Assert
        self.assertEqual(0, len(concurrent_dict))


class ObjectCacheTests(unittest.TestCase):

    def test_can_get_from_empty_cache(self):
        # Arrange
        cache = ObjectCache(Symbol, parse_symbol)
        symbol = 'AUDUSD.FXCM'

        # Act
        result = cache.get(symbol)

        # Assert
        self.assertEqual(symbol, str(result))

    def test_can_get_from_cache(self):
        # Arrange
        cache = ObjectCache(Symbol, parse_symbol)
        symbol = 'AUDUSD.FXCM'
        cache.get(symbol)

        # Act
        cache.get(symbol)
        result1 = cache.get(symbol)
        result2 = cache.get(symbol)

        # Assert
        self.assertEqual(symbol, str(result1))
        self.assertEqual(id(result1), id(result2))
