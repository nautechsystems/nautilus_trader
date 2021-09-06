# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import fsspec
import pytest

from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.persistence.catalog import DataCatalog
from nautilus_trader.persistence.external.core import RawFile
from nautilus_trader.persistence.external.core import process_raw_file
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.mocks import data_catalog_setup


class TestBetfairPersistence:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.fs = self.catalog.fs
        self.reader = BetfairTestStubs.betfair_reader()

    @pytest.mark.skip("compression broken in github ci")
    def test_bsp_deltas(self):
        rf = RawFile(
            open_file=fsspec.open(
                f"{PACKAGE_ROOT}/data/betfair/1.170258150.bz2", compression="infer"
            )
        )
        process_raw_file(catalog=self.catalog, raw_file=rf, reader=self.reader)
        data = self.catalog.query(BSPOrderBookDelta)
        assert len(data) == 443
