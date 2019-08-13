# -------------------------------------------------------------------------------------------------
# <copyright file="test_core_cache.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.core.cache import ObjectCache
from nautilus_trader.model.objects import Symbol
from nautilus_trader.serialization.common import parse_symbol


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
