# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.core.message import Message
from nautilus_trader.core.message import MessageType
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.model.commands import SubmitOrder
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from tests.test_kit.performance import PerformanceTestCase


AUDUSD = InstrumentId(Symbol("AUDUSD"), Venue("IDEALPRO"))
MESSAGE = Message(MessageType.COMMAND, uuid4(), 0)


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
    def is_instance():
        x = isinstance(MESSAGE, SubmitOrder)
        return x

    @staticmethod
    def is_message_type():
        x = 0 == MESSAGE.type
        return x


class ExperimentsPerformanceTests(PerformanceTestCase):
    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    @staticmethod
    def test_builtin_arithmetic(benchmark):
        benchmark.pedantic(
            Experiments.built_in_arithmetic, iterations=100_000, rounds=1
        )
        # ~0.0ms / ~0.1μs / 106ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    @staticmethod
    def test_class_name(benchmark):
        benchmark.pedantic(Experiments.class_name, iterations=100_000, rounds=1)
        # ~0.0ms / ~0.2μs / 161ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    @staticmethod
    def test_str_assignment(benchmark):
        benchmark.pedantic(Experiments.str_assignment, iterations=100_000, rounds=1)
        # ~0.0ms / ~0.1μs / 103ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    @staticmethod
    def test_is_instance(benchmark):
        benchmark.pedantic(Experiments.is_instance, iterations=100_000, rounds=1)
        # ~0.0ms / ~0.2μs / 153ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(disable_gc=True, warmup=True)
    @staticmethod
    def test_is_message_type(benchmark):
        benchmark.pedantic(Experiments.is_message_type, iterations=100_000, rounds=1)
        # ~0.0ms / ~0.2μs / 150ns minimum of 100,000 runs @ 1 iteration each run.
