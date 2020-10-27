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

import unittest

import numpy as np

from nautilus_trader.core.functions import fast_mean
from nautilus_trader.core.message import Message
from nautilus_trader.core.message import MessageType
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.stubs import UNIX_EPOCH

_AUDUSD = Symbol("AUDUSD", Venue("IDEALPRO"))
_TEST_LIST = [0.0, 1.1, 2.2, 3.3, 4.4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
_MESSAGE = Message(MessageType.COMMAND, uuid4(), UNIX_EPOCH)


class Experiments:

    @staticmethod
    def built_in_arithmetic():
        x = 1 + 1
        return x

    @staticmethod
    def class_name():
        x = "123".__class__.__name__
        return x

    @staticmethod
    def str_assignment():
        x = "123"
        return x

    @staticmethod
    def np_mean():
        x = np.mean(_TEST_LIST)
        return x

    @staticmethod
    def fast_mean():
        x = fast_mean(_TEST_LIST)
        return x

    @staticmethod
    def is_instance():
        x = isinstance(_MESSAGE, SubmitOrder)
        return x

    @staticmethod
    def is_message_type():
        x = 0 == _MESSAGE.message_type
        return x


class ExperimentsPerformanceTests(unittest.TestCase):

    @staticmethod
    def test_builtin_decimal_size():
        PerformanceHarness.profile_function(Experiments.built_in_arithmetic, 3, 1000000)
        # ~51ms (51648μs) minimum of 3 runs @ 1,000,000 iterations each run.

    @staticmethod
    def test_class_name():
        PerformanceHarness.profile_function(Experiments.class_name, 3, 1000000)
        # ~130ms (130037μs) minimum of 3 runs @ 1,000,000 iterations each run.

    @staticmethod
    def test_str_assignment():
        PerformanceHarness.profile_function(Experiments.str_assignment, 3, 1000000)
        # ~53ms (53677μs) minimum of 3 runs @ 1,000,000 iterations each run.

    @staticmethod
    def test_np_mean():
        PerformanceHarness.profile_function(Experiments.np_mean, 3, 10000)
        # ~53ms (53677μs) minimum of 3 runs @ 10,000 iterations each run.

    @staticmethod
    def test_fast_mean():
        PerformanceHarness.profile_function(Experiments.fast_mean, 3, 10000)
        # ~53ms (53677μs) minimum of 3 runs @ 10,000 iterations each run.

    @staticmethod
    def test_is_instance():
        PerformanceHarness.profile_function(Experiments.is_instance, 3, 100000)
        # ~53ms (53677μs) minimum of 3 runs @ 100,000 iterations each run.

    @staticmethod
    def test_is_message_type():
        PerformanceHarness.profile_function(Experiments.is_message_type, 3, 100000)
        # ~53ms (53677μs) minimum of 3 runs @ 100,000 iterations each run.
