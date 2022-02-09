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

import pickle
from unittest.mock import MagicMock
from unittest.mock import patch

from nautilus_trader.adapters.interactive_brokers.providers import (
    InteractiveBrokersInstrumentProvider,
)
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/ib/responses/"


class TestIBInstrumentProvider:
    def setup(self):
        self.ib = MagicMock()
        self.provider = InteractiveBrokersInstrumentProvider(client=self.ib)
        self.provider.connect()

    def test_load_equity_contract_instrument(self):
        # Arrange
        instrument_id = InstrumentId(
            symbol=Symbol("AAPL"),
            venue=Venue("NASDAQ"),
        )
        contract_details = pickle.load(open(TEST_PATH + "contracts/AAPL.pkl", "rb"))  # noqa S301

        # Act
        with patch.object(
            self.provider._client, "reqContractDetails", return_value=[contract_details]
        ):
            self.provider.load("AAPL", "NASDAQ")
            equity = self.provider.find(instrument_id)

        # Assert
        assert InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ")) == equity.id
        assert equity.asset_class == AssetClass.EQUITY
        assert equity.asset_type == AssetType.SPOT
        assert 100 == equity.multiplier
        assert Price.from_str("0.01") == equity.price_increment
        assert 2, equity.price_precision

    def test_load_futures_contract_instrument(self):
        # Arrange
        instrument_id = InstrumentId(
            symbol=Symbol("CLZ2"),
            venue=Venue("NYMEX"),
        )
        contract_details = pickle.load(open(TEST_PATH + "contracts/CLZ2.pkl", "rb"))  # noqa S301

        # Act
        with patch.object(
            self.provider._client, "reqContractDetails", return_value=[contract_details]
        ):
            self.provider.load("CLZ2", "NYMEX")
            future = self.provider.find(instrument_id)

        # Assert
        assert future.id == instrument_id
        assert future.asset_class == AssetClass.INDEX
        assert future.multiplier == 1000
        assert future.price_increment == Price.from_str("0.01")
        assert future.price_precision == 2
