# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import itertools
import os
import time

import pytest

from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReader
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReaderType
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetType
from nautilus_trader.model.data.tick import QuoteTick
from tests import TEST_DATA_DIR


@pytest.mark.benchmark(
    group="parquet-reader",
    min_rounds=5,
    timer=time.time,
    disable_gc=True,
    warmup=True,
)
def test_pyo3_benchmark_parquet_buffer_reader(benchmark):
    parquet_data_path = os.path.join(TEST_DATA_DIR, "quote_tick_data.parquet")
    file_data = None
    with open(parquet_data_path, "rb") as f:
        file_data = f.read()

    @benchmark
    def run():
        reader = ParquetReader(
            "",
            1000,
            ParquetType.QuoteTick,
            ParquetReaderType.Buffer,
            file_data,
        )
        data = map(QuoteTick.list_from_capsule, reader)
        ticks = list(itertools.chain(*data))
        print(len(ticks))
