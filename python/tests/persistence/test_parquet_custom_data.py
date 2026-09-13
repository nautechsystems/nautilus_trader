# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Parquet catalog regression tests.
"""

from pathlib import Path

from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import RustTestCustomData


def test_python_custom_data_query_applies_where_clause(tmp_path: Path) -> None:
    """
    Verify python custom data query applies where clause.
    """
    register_custom_data_class(RustTestCustomData)
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    instrument_id = InstrumentId.from_str("RUST.WHERE")
    data_type = DataType("RustTestCustomData", None, str(instrument_id))
    original = [
        RustTestCustomData(instrument_id, 1.23, True, 1, 1),
        RustTestCustomData(instrument_id, 4.56, False, 2, 2),
    ]
    catalog.write_custom_data([CustomData(data_type, item) for item in original])

    result = catalog.query(
        "RustTestCustomData",
        [str(instrument_id)],
        None,
        None,
        "value = 4.56",
        None,
        True,
    )

    assert len(result) == 1
    inner = result[0].data
    assert isinstance(inner, RustTestCustomData)
    assert inner.instrument_id == instrument_id
    assert inner.value == 4.56
    assert inner.flag is False
    assert inner.ts_event == 2
    assert inner.ts_init == 2
