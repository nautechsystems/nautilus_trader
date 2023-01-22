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

import os

from nautilus_trader import PACKAGE_ROOT
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReader
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetReaderType
from nautilus_trader.core.nautilus_pyo3.persistence import ParquetType
from nautilus_trader.model.data.tick import QuoteTick


def test_python_parquet_reader():
    parquet_data_path = os.path.join(PACKAGE_ROOT, "tests/test_data/quote_tick_data.parquet")
    reader = ParquetReader(
        parquet_data_path,
        100,
        ParquetType.QuoteTick,
        ParquetReaderType.File,
    )

    total_count = 0
    for chunk in reader:
        tick_list = QuoteTick.list_from_capsule(chunk)
        total_count += len(tick_list)

    reader.drop()

    assert total_count == 9500
    # test on last chunk tick i.e. 9500th record
    assert str(tick_list[-1]) == "EUR/USD.SIM,1.12130,1.12132,0,0,1577919652000000125"


if __name__ == "__main__":
    test_python_parquet_reader()
