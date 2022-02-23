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

import pickle
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.ib.providers import IBInstrumentProvider
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/ib/responses/"


@pytest.mark.skip(reason="WIP")
class TestIBInstrumentProvider:
    def test_load_futures_contract_instrument(self):
        # Arrange
        mock_client = MagicMock()

        with open(TEST_PATH + "contract_details_cl.pickle", "rb") as file:
            details = pickle.load(file)  # noqa (S301 possible security issue)

        mock_client.reqContractDetails.return_value = [details]

        provider = IBInstrumentProvider(client=mock_client)
        provider.connect()

        instrument_id = InstrumentId(
            symbol=Symbol("CL"),
            venue=Venue("NYMEX"),
        )

        details = {
            "asset_type": "FUTURE",
            "asset_class": "COMMODITY",
            "expiry": "20211119",
            "currency": "USD",
            "multiplier": 1000,
        }

        # Act
        provider.load(instrument_id, details)
        future = provider.find(instrument_id)

        # Assert
        assert instrument_id == future.id
        assert 1000, future.multiplier
        assert Price.from_str("0.01") == future.price_increment
        assert 2, future.price_precision
        # TODO: Test all properties

    def test_load_equity_contract_instrument(self):
        # Arrange
        mock_client = MagicMock()

        with open(TEST_PATH + "contract_details_aapl_contract.pickle", "rb") as file:
            contract = pickle.load(file)  # noqa (S301 possible security issue)

        with open(TEST_PATH + "contract_details_aapl_details.pickle", "rb") as file:
            details = pickle.load(file)  # noqa (S301 possible security issue)

        mock_client.reqContractDetails.return_value = [details]
        mock_client.qualifyContracts.return_value = [contract]

        provider = IBInstrumentProvider(client=mock_client)
        provider.connect()

        instrument_id = InstrumentId(
            symbol=Symbol("AAPL"),
            venue=Venue("NASDAQ"),
        )

        details = {"asset_type": "SPOT"}

        # Act
        provider.load(instrument_id, details)
        equity = provider.find(instrument_id)

        # Assert
        assert InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ")) == equity.id
        assert equity.asset_class == AssetClass.EQUITY
        assert equity.asset_type == AssetType.SPOT
        assert 100 == equity.multiplier
        assert Price.from_str("0.01") == equity.price_increment
        assert 2, equity.price_precision
        # TODO: Test all properties
