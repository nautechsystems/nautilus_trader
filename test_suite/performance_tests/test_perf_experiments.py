# -------------------------------------------------------------------------------------------------
# <copyright file="test_perf_experiments.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from test_kit.performance import PerformanceProfiler


class Experiments:

    @staticmethod
    def built_in_arithmetic():
        x = 1 + 1


class ExperimentsPerformanceTests(unittest.TestCase):

    def test_builtin_decimal_size(self):
        result = PerformanceProfiler.profile_function(Experiments.built_in_arithmetic, 3, 1000000)
        # ~51ms (51648μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 1.0)
