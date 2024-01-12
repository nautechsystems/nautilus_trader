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

from nautilus_trader.core.correctness import PyCondition


def test_condition_none(benchmark):
    benchmark.pedantic(
        target=PyCondition.none,
        args=(None, "param"),
        iterations=100_000,
        rounds=1,
    )
    # ~0.0ms / ~0.1μs / 142ns minimum of 100,000 runs @ 1 iteration each run.


def test_condition_true(benchmark):
    benchmark.pedantic(
        target=PyCondition.true,
        args=(True, "this should be true"),
        iterations=100_000,
        rounds=1,
    )
    # ~0.0ms / ~0.1μs / 149ns minimum of 100,000 runs @ 1 iteration each run.

    # 100000 iterations @ 12ms with boolean except returning False
    # 100000 iterations @ 12ms with void except returning * !


def test_condition_valid_string(benchmark):
    benchmark.pedantic(
        target=PyCondition.valid_string,
        args=("abc123", "string_param"),
        iterations=100_000,
        rounds=1,
    )
    # ~0.0ms / ~0.2μs / 205ns minimum of 100,000 runs @ 1 iteration each run.


def test_condition_type_or_none(benchmark):
    benchmark.pedantic(
        target=PyCondition.type_or_none,
        args=("hello", str, "world"),
        iterations=100_000,
        rounds=1,
    )
    # ~0.0ms / ~0.2μs / 224ns minimum of 100,000 runs @ 1 iteration each run.
