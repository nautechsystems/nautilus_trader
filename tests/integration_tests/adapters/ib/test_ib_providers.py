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

from decimal import Decimal
import pickle
from unittest.mock import MagicMock

from nautilus_trader.adaptors.ib.providers import IBInstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adaptors/ib/responses/"


class TestIBInstrumentProvider:
    def test_load_futures_contract_instrument(self):
        # Arrange
        mock_client = MagicMock()

        with open(TEST_PATH + "contract_details_cl.pickle", "rb") as file:
            details = pickle.load(file)

        print(details)
        mock_client.reqContractDetails.return_value = [details]

        provider = IBInstrumentProvider(client=mock_client)
        provider.connect()

        instrument_id = InstrumentId(
            symbol=Symbol("CL"),
            venue=Venue("NYMEX"),
        )

        details = {
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
        assert Decimal("0.01") == future.tick_size
        assert 2, future.price_precision
        # TODO: Test all properties
