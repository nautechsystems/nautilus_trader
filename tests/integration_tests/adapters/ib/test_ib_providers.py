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

from nautilus_trader.adapters.ib.providers import IBInstrumentProvider
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import Exchange
from nautilus_trader.model.identifiers import FutureSecurity
from nautilus_trader.model.identifiers import Symbol
from tests import TESTS_PACKAGE_ROOT

TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/ib/responses/"


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

        security = FutureSecurity(
            symbol=Symbol("CL"),
            exchange=Exchange("NYMEX"),
            asset_class=AssetClass.COMMODITY,
            expiry="20211119",
            currency=Currency.from_str("USD"),
            multiplier=1000,
        )

        # Act
        future = provider.load_future(security)

        # Assert
        assert security == future.security
        assert 1000, future.multiplier
        assert Decimal("0.01") == future.tick_size
        assert 2, future.price_precision
        # TODO: Test all properties
