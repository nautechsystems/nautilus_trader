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
Test retained allocations from persistence query workloads.
"""

import gc

import pytest
from tests.stubs import TestDataProviderPyo3

from nautilus_trader.model import InstrumentId
from nautilus_trader.persistence import ParquetDataCatalog


pytest.importorskip("pytest_memray")

_QUERIES = 128
_INSTRUMENT_ID = InstrumentId.from_str("AUD/USD.SIM")
_QUOTES = [
    TestDataProviderPyo3.quote_tick(
        instrument_id=_INSTRUMENT_ID,
        ts_event=ts,
        ts_init=ts,
    )
    for ts in range(64)
]


@pytest.fixture(name="catalog", scope="module")
def fixture_catalog(tmp_path_factory: pytest.TempPathFactory) -> ParquetDataCatalog:
    """
    Fixture catalog initialized outside Memray's tracked test call.
    """
    catalog = ParquetDataCatalog(str(tmp_path_factory.mktemp("memray-catalog")))
    catalog.write_quote_ticks(_QUOTES)
    return catalog


@pytest.fixture(scope="module", autouse=True)
def _warm_up_catalog_query(catalog: ParquetDataCatalog) -> None:
    assert _query_quotes(catalog) == (len(_QUOTES), 0, len(_QUOTES) - 1)


@pytest.mark.limit_leaks("32 KB")
def test_repeated_catalog_queries_release_native_allocations(
    catalog: ParquetDataCatalog,
) -> None:
    """
    Test repeated Arrow and Parquet query results do not accumulate.
    """
    for _ in range(_QUERIES):
        assert _query_quotes(catalog) == (len(_QUOTES), 0, len(_QUOTES) - 1)

    gc.collect()


def _query_quotes(catalog: ParquetDataCatalog) -> tuple[int, int, int]:
    quotes = catalog.query_quote_ticks([str(_INSTRUMENT_ID)])
    return len(quotes), quotes[0].ts_init, quotes[-1].ts_init
