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

from pathlib import Path

import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.databento import DatabentoImbalance
from nautilus_trader.adapters.databento import DatabentoStatistics
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.test_kit.mocks.data import setup_catalog


REPO_ROOT: Path = TEST_DATA_DIR.parents[1]
DATABENTO_RUST_TEST_DATA_DIR: Path = REPO_ROOT / "crates" / "adapters" / "databento" / "test_data"


@pytest.fixture
def catalog(tmp_path) -> ParquetDataCatalog:
    return setup_catalog(protocol="memory", path=tmp_path / "catalog")


def test_imbalance_catalog_round_trip(catalog: ParquetDataCatalog) -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_RUST_TEST_DATA_DIR / "test_data.imbalance.dbn.zst"
    data = loader.from_dbn_file(path, as_legacy_cython=False)
    assert len(data) == 2

    # Act
    catalog.write_data(data)
    result = catalog.custom_data(cls=DatabentoImbalance)

    # Assert
    assert len(result) == len(data)
    for orig, retrieved in zip(data, result, strict=True):
        assert retrieved.instrument_id == orig.instrument_id
        assert retrieved.ref_price == orig.ref_price
        assert retrieved.cont_book_clr_price == orig.cont_book_clr_price
        assert retrieved.auct_interest_clr_price == orig.auct_interest_clr_price
        assert retrieved.paired_qty == orig.paired_qty
        assert retrieved.total_imbalance_qty == orig.total_imbalance_qty
        assert retrieved.side == orig.side
        assert retrieved.significant_imbalance == orig.significant_imbalance
        assert retrieved.ts_event == orig.ts_event
        assert retrieved.ts_init == orig.ts_init


def test_statistics_catalog_round_trip(catalog: ParquetDataCatalog) -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = DATABENTO_RUST_TEST_DATA_DIR / "test_data.statistics.dbn.zst"
    instrument_id = nautilus_pyo3.InstrumentId.from_str("ESM4.GLBX")
    data = loader.from_dbn_file(path, instrument_id=instrument_id, as_legacy_cython=False)
    assert len(data) == 2

    # Act
    catalog.write_data(data)
    result = catalog.custom_data(cls=DatabentoStatistics)

    # Assert
    assert len(result) == len(data)
    for orig, retrieved in zip(data, result, strict=True):
        assert retrieved.instrument_id == orig.instrument_id
        assert retrieved.stat_type == orig.stat_type
        assert retrieved.update_action == orig.update_action
        assert retrieved.price == orig.price
        assert retrieved.quantity == orig.quantity
        assert retrieved.channel_id == orig.channel_id
        assert retrieved.stat_flags == orig.stat_flags
        assert retrieved.sequence == orig.sequence
        assert retrieved.ts_ref == orig.ts_ref
        assert retrieved.ts_in_delta == orig.ts_in_delta
        assert retrieved.ts_event == orig.ts_event
        assert retrieved.ts_init == orig.ts_init
