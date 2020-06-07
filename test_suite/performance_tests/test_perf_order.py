# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.generators import OrderIdGenerator
from test_kit.performance import PerformanceHarness
from test_kit.stubs import TestStubs


AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()


class OrderPerformanceTests(unittest.TestCase):

    def setUp(self):
        self.generator = OrderIdGenerator(IdTag('001'), IdTag('001'))

    def test_order_id_generator(self):
        result = PerformanceHarness.profile_function(self.generator.generate, 3, 10000)
        # ~18ms (18831μs) minimum of 5 runs @ 10000 iterations
        self.assertTrue(result < 0.03)
