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
import unittest
from unittest.mock import MagicMock

from nautilus_trader.adapters.ib.providers import IBInstrumentProvider
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import Exchange
#from nautilus_trader.model.identifiers import Security
from tests import TESTS_PACKAGE_ROOT

TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/ib/responses/"


class IBInstrumentProviderTests(unittest.TestCase):

    pass
    # def test_load_futures_contract_instrument(self):
    #     # Arrange
    #     mock_client = MagicMock()
    #
    #     with open(TEST_PATH + "contract_details_cl.pickle", "rb") as file:
    #         details = pickle.load(file)
    #
    #     print(details)
    #     mock_client.reqContractDetails.return_value = [details]
    #
    #     provider = IBInstrumentProvider(client=mock_client)
    #     provider.connect()
    #
    #     security = Security(
    #         symbol="CL",
    #         venue=Exchange("NYMEX"),
    #         sec_type=AssetType.FUTURE,
    #         multiplier="1000",
    #         expiry="20211119",
    #         currency="USD",
    #     )
    #
    #     # Act
    #     future = provider.load_future(security)
    #
    #     # Assert
    #     self.assertEqual(security, future.symbol)
    #     self.assertEqual(1000, future.multiplier)
    #     self.assertEqual(Decimal("0.01"), future.tick_size)
    #     self.assertEqual(2, future.price_precision)
    #     # TODO: Test all properties
