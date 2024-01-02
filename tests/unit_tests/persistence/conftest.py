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

import pytest

from nautilus_trader.adapters.betfair.parsing.core import betting_instruments_from_file
from nautilus_trader.adapters.betfair.parsing.core import parse_betfair_file
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from tests import TEST_DATA_DIR


@pytest.fixture(name="memory_data_catalog")
def fixture_memory_data_catalog() -> ParquetDataCatalog:
    return data_catalog_setup(protocol="memory")


@pytest.fixture(name="data_catalog")
def fixture_data_catalog() -> ParquetDataCatalog:
    return data_catalog_setup(protocol="file")


@pytest.fixture(name="betfair_catalog")
def fixture_betfair_catalog(data_catalog: ParquetDataCatalog) -> ParquetDataCatalog:
    filename = TEST_DATA_DIR / "betfair" / "1.166564490.bz2"

    # Write betting instruments
    instruments = betting_instruments_from_file(filename, currency="GBP")
    data_catalog.write_data(instruments)

    # Write data
    data = list(parse_betfair_file(filename, currency="GBP"))
    data_catalog.write_data(data)

    return data_catalog
