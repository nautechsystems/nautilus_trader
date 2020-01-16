# -------------------------------------------------------------------------------------------------
# <copyright file="test_perf_experiments.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.identifiers import Symbol, Venue

from test_kit.performance import PerformanceProfiler


_AUDUSD = Symbol('AUDUSD', Venue('IDEALPRO'))


class Experiments:

    @staticmethod
    def built_in_arithmetic():
        x = 1 + 1

    @staticmethod
    def class_name():
        x = '123'
        assign = x.__class__.__name__

    @staticmethod
    def str_assignment():
        x = '123'
        x = '123'


class ExperimentsPerformanceTests(unittest.TestCase):

    def test_builtin_decimal_size(self):
        result = PerformanceProfiler.profile_function(Experiments.built_in_arithmetic, 3, 1000000)
        # ~51ms (51648μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 1.0)

    def test_class_name(self):
        result = PerformanceProfiler.profile_function(Experiments.class_name, 3, 1000000)
        # ~130ms (130037μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 1.0)

    def test_str_assignment(self):
        result = PerformanceProfiler.profile_function(Experiments.str_assignment, 3, 1000000)
        # ~53ms (53677μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 1.0)