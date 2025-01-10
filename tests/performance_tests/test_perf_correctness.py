# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    benchmark(PyCondition.none, None, "param")


def test_condition_true(benchmark):
    benchmark(PyCondition.is_true, True, "this should be true")


def test_condition_valid_string(benchmark):
    benchmark(PyCondition.valid_string, "abc123", "string_param")


def test_condition_type_or_none(benchmark):
    benchmark(PyCondition.type_or_none, "hello", str, "world")
