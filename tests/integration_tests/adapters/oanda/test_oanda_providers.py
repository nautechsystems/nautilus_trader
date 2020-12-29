# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import os
import unittest

import oandapyV20

from nautilus_trader.adapters.oanda.providers import OandaInstrumentProvider
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instrument import Instrument


# Requirements:
#    - An internet connection
#    - Environment variable OANDA_API_TOKEN with a valid practice account api token
#    - Environment variable OANDA_ACCOUNT_ID with a valid practice `accountID`


class OandaInstrumentProviderTests(unittest.TestCase):

    def test_load_all(self):
        # Arrange
        oanda_api_token = os.getenv("OANDA_API_TOKEN")
        oanda_account_id = os.getenv("OANDA_ACCOUNT_ID")

        client = oandapyV20.API(access_token=oanda_api_token)
        provider = OandaInstrumentProvider(client=client, account_id=oanda_account_id)

        # Act
        provider.load_all()

        # Assert
        self.assertTrue(provider.count > 0)  # No exceptions raised

    def test_get_all_when_not_loaded_returns_empty_dict(self):
        # Arrange
        oanda_api_token = os.getenv("OANDA_API_TOKEN")
        oanda_account_id = os.getenv("OANDA_ACCOUNT_ID")

        client = oandapyV20.API(access_token=oanda_api_token)
        provider = OandaInstrumentProvider(client=client, account_id=oanda_account_id)

        # Act
        instruments = provider.get_all()

        # Assert
        self.assertTrue(len(instruments) == 0)

    def test_get_all_when_loaded_returns_instruments(self):
        # Arrange
        oanda_api_token = os.getenv("OANDA_API_TOKEN")
        oanda_account_id = os.getenv("OANDA_ACCOUNT_ID")

        client = oandapyV20.API(access_token=oanda_api_token)
        provider = OandaInstrumentProvider(client=client, account_id=oanda_account_id, load_all=True)

        # Act
        instruments = provider.get_all()

        # Assert
        self.assertTrue(len(instruments) > 0)
        self.assertEqual(dict, type(instruments))
        self.assertEqual(Symbol, type(next(iter(instruments))))

    def test_get_audusd_when_not_loaded_returns_none(self):
        # Arrange
        oanda_api_token = os.getenv("OANDA_API_TOKEN")
        oanda_account_id = os.getenv("OANDA_ACCOUNT_ID")

        client = oandapyV20.API(access_token=oanda_api_token)
        provider = OandaInstrumentProvider(client=client, account_id=oanda_account_id)

        symbol = Symbol("AUD/USD", Venue("OANDA"))

        # Act
        instrument = provider.get(symbol)

        # Assert
        self.assertIsNone(instrument)

    def test_get_audusd_when_loaded_returns_expected_instrument(self):
        # Arrange
        oanda_api_token = os.getenv("OANDA_API_TOKEN")
        oanda_account_id = os.getenv("OANDA_ACCOUNT_ID")

        client = oandapyV20.API(access_token=oanda_api_token)
        provider = OandaInstrumentProvider(client=client, account_id=oanda_account_id, load_all=True)

        symbol = Symbol("AUD/USD", Venue("OANDA"))

        # Act
        instrument = provider.get(symbol)

        # Assert
        self.assertEqual(Instrument, type(instrument))
        self.assertEqual(AssetClass.FX, instrument.asset_class)
        self.assertEqual(AssetType.SPOT, instrument.asset_type)
        self.assertEqual(AUD, instrument.base_currency)
        self.assertEqual(USD, instrument.quote_currency)
        self.assertEqual(USD, instrument.settlement_currency)
