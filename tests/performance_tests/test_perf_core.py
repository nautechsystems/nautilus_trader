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

import pandas as pd

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.datetime import format_iso8601
from nautilus_trader.core.datetime import unix_nanos_to_iso8601


def test_nautilus_convert_to_snake_case(benchmark) -> None:
    benchmark(nautilus_pyo3.convert_to_snake_case, "PascalCase")


def test_unix_nanos_to_iso8601(benchmark) -> None:
    benchmark(lambda: unix_nanos_to_iso8601(0))


def test_format_iso8601(benchmark) -> None:
    dt = pd.Timestamp(0)

    benchmark(lambda: format_iso8601(dt))


def test_format_iso8601_millis(benchmark) -> None:
    dt = pd.Timestamp(0)

    benchmark(lambda: format_iso8601(dt, nanos_precision=False))
