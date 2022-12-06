# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
from nautilus_trader.persistence.external.core import RawFile
from nautilus_trader.persistence.external.core import process_raw_file
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


class TestBetfairPersistence:
    def setup(self):
        data_catalog_setup()
        self.catalog = ParquetDataCatalog.from_env()
        self.fs = self.catalog.fs
        self.reader = BetfairTestStubs.betfair_reader()
        self.instrument = TestInstrumentProvider.betting_instrument()

    def test_bsp_delta_serialize(self):
        # Arrange
        bsp_delta = BSPOrderBookDelta.from_dict(
            {
                "type": "BSPOrderBookDelta",
                "instrument_id": self.instrument.id.value,
                "book_type": "L2_MBP",
                "action": "UPDATE",
                "order_price": 0.990099,
                "order_size": 60.07,
                "order_side": "BUY",
                "order_id": "f7ed1f20-8c1d-40c6-9d63-bd45f7cc0a86",
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )
        values = bsp_delta.to_dict(bsp_delta)
        assert bsp_delta.from_dict(values) == bsp_delta
        assert values["type"] == "BSPOrderBookDelta"

    @pytest.mark.skip("compression broken in github ci")
    def test_bsp_deltas(self):
        rf = RawFile(
            open_file=fsspec.open(
                f"{TEST_DATA_DIR}/betfair/1.170258150.bz2",
                compression="infer",
            ),
        )
        process_raw_file(catalog=self.catalog, raw_file=rf, reader=self.reader)
        data = self.catalog.query(BSPOrderBookDelta)
        assert len(data) == 443
