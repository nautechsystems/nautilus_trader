# -------------------------------------------------------------------------------------------------
# <copyright file="test_perf_experiments.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import numpy as np
import unittest
import uuid

from nautilus_trader.core.types import GUID
from nautilus_trader.core.message import Message, MessageType
from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.core.functions import fast_mean
from test_kit.performance import PerformanceProfiler
from test_kit.stubs import UNIX_EPOCH


_AUDUSD = Symbol('AUDUSD', Venue('IDEALPRO'))
_TEST_LIST = [0.0, 1.1, 2.2, 3.3, 4.4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
_MESSAGE = Message(MessageType.COMMAND, GUID(uuid.uuid4()), UNIX_EPOCH)


class Experiments:

    @staticmethod
    def built_in_arithmetic():
        x = 1 + 1

    @staticmethod
    def class_name():
        x = '123'.__class__.__name__

    @staticmethod
    def str_assignment():
        x = '123'

    @staticmethod
    def np_mean():
        x = np.mean(_TEST_LIST)

    @staticmethod
    def fast_mean():
        x = fast_mean(_TEST_LIST)

    @staticmethod
    def is_instance():
        x = isinstance(_MESSAGE, SubmitOrder)

    @staticmethod
    def is_message_type():
        x = 0 == _MESSAGE.message_type


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

    def test_np_mean(self):
        result = PerformanceProfiler.profile_function(Experiments.np_mean, 3, 10000)
        # ~53ms (53677μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 1.0)

    def test_fast_mean(self):
        result = PerformanceProfiler.profile_function(Experiments.fast_mean, 3, 10000)
        # ~53ms (53677μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.01)

    def test_is_instance(self):
        result = PerformanceProfiler.profile_function(Experiments.is_instance, 3, 100000)
        # ~53ms (53677μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.1)

    def test_is_message_type(self):
        result = PerformanceProfiler.profile_function(Experiments.is_message_type, 3, 100000)
        # ~53ms (53677μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.1)
