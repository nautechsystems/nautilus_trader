# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import uuid

import pytest

from nautilus_trader.core.uuid import UUID4


class TestUUIDPerformance:
    @pytest.mark.benchmark(group="core", disable_gc=True, warmup=True)
    @staticmethod
    def test_make_builtin_uuid(benchmark):
        benchmark.pedantic(
            target=uuid.uuid4,
            iterations=100_000,
            rounds=1,
        )

    def test_make_builtin_uuid_bench(self, benchmark):
        benchmark.pedantic(
            target=uuid.uuid4,
            rounds=100_000,
            iterations=1,
        )
        # ~0.0ms / ~2.1μs / 2067ns minimum of 100,000 runs @ 1 iteration each run.

    @pytest.mark.benchmark(group="core", disable_gc=True, warmup=True)
    @staticmethod
    def test_make_nautilus_uuid(benchmark):
        benchmark.pedantic(
            target=UUID4,
            iterations=100_000,
            rounds=1,
        )

    def test_make_nautilus_uuid_bench(self, benchmark):
        benchmark.pedantic(
            target=UUID4,
            rounds=100_000,
            iterations=1,
        )
        # ~0.0ms / ~0.8μs / 780ns minimum of 100,000 runs @ 1 iteration each run.

    def test_nautilus_uuid_value_bench(self, benchmark):
        uuid = UUID4()

        def get_uuid_value():
            uuid.value

        benchmark.pedantic(
            target=get_uuid_value,
            rounds=100_000,
            iterations=1,
        )
        # ~0.0ms / ~0.2μs / 152ns minimum of 100,000 runs @ 1 iteration each run. (readonly value)
        # ~0.0ms / ~0.2μs / 163ns minimum of 100,000 runs @ 1 iteration each run. (cpdef str to_str())
        # ~0.0ms / ~0.2μs / 212ns minimum of 100,000 runs @ 1 iteration each run. (property value)
