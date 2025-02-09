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

import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.betfair.parsing.core import betting_instruments_from_file
from nautilus_trader.adapters.betfair.parsing.core import parse_betfair_file
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.objects import Money
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.test_kit.mocks.data import setup_catalog


@pytest.fixture(name="catalog_memory")
def fixture_catalog_memory() -> ParquetDataCatalog:
    return setup_catalog(protocol="memory")


@pytest.fixture(name="catalog")
def fixture_catalog() -> ParquetDataCatalog:
    return setup_catalog(protocol="file")


@pytest.fixture(name="catalog_betfair")
def fixture_catalog_betfair(catalog: ParquetDataCatalog) -> ParquetDataCatalog:
    filename = TEST_DATA_DIR / "betfair" / "1-166564490.bz2"

    # Write betting instruments
    instruments = betting_instruments_from_file(
        filename,
        currency="GBP",
        ts_event=0,
        ts_init=0,
        min_notional=Money(1, GBP),
    )
    catalog.write_data(instruments)

    # Write data
    data = list(
        parse_betfair_file(
            filename,
            currency="GBP",
            min_notional=Money(1, GBP),
        ),
    )
    catalog.write_data(data)

    return catalog
