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
import fsspec
import pytest

from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDeltas
from nautilus_trader.adapters.betfair.historic import make_betfair_reader
from nautilus_trader.persistence.external.core import RawFile
from nautilus_trader.persistence.external.core import process_raw_file
from nautilus_trader.serialization.arrow.serializer import ParquetSerializer
from nautilus_trader.test_kit.mocks.data import data_catalog_setup
from tests import TEST_DATA_DIR
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument


@pytest.mark.skip(reason="Reimplementing")
class TestBetfairPersistence:
    def setup(self):
        self.catalog = data_catalog_setup(protocol="memory")
        self.fs = self.catalog.fs
        self.instrument = betting_instrument()

    def test_bsp_delta_serialize(self):
        # Arrange
        bsp_delta = BSPOrderBookDelta.from_dict(
            {
                "type": "BSPOrderBookDelta",
                "instrument_id": self.instrument.id.value,
                "action": "UPDATE",
                "price": 0.990099,
                "size": 60.07,
                "side": "BUY",
                "order_id": 1635313844283000000,
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )

        # Act
        values = bsp_delta.to_dict(bsp_delta)

        # Assert
        assert bsp_delta.from_dict(values) == bsp_delta
        assert values["type"] == "BSPOrderBookDelta"

    def test_betfair_starting_price_to_from_dict(self):
        # Arrange
        bsp = BetfairStartingPrice.from_dict(
            {
                "type": "BetfairStartingPrice",
                "instrument_id": self.instrument.id.value,
                "bsp": 1.20,
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )

        # Act
        values = bsp.to_dict()
        result = bsp.from_dict(values)

        # Assert
        assert values["type"] == "BetfairStartingPrice"
        assert result.bsp == bsp.bsp

    def test_betfair_starting_price_serialization(self):
        # Arrange
        bsp = BetfairStartingPrice.from_dict(
            {
                "type": "BetfairStartingPrice",
                "instrument_id": self.instrument.id.value,
                "bsp": 1.20,
                "ts_event": 1635313844283000000,
                "ts_init": 1635313844283000000,
            },
        )

        # Act
        serialized = ParquetSerializer.serialize(bsp)
        [result] = ParquetSerializer.deserialize(BetfairStartingPrice, [serialized])

        # Assert
        assert result.bsp == bsp.bsp

    @pytest.mark.skip("Broken due to parquet writing")
    def test_bsp_deltas(self):
        # Arrange
        rf = RawFile(
            open_file=fsspec.open(f"{TEST_DATA_DIR}/betfair/1.206064380.bz2", compression="infer"),
            block_size=None,
        )

        # Act
        process_raw_file(catalog=self.catalog, reader=make_betfair_reader(), raw_file=rf)

        # Act
        data = self.catalog.query(BSPOrderBookDeltas)

        # Assert
        assert len(data) == 2824
