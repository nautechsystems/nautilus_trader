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

import json
import unittest
from unittest.mock import MagicMock

from nautilus_trader.adapters.oanda.providers import OandaInstrumentProvider
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.cfd import CFDInstrument
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/oanda/responses/"


class OandaInstrumentProviderTests(unittest.TestCase):
    def test_load_all(self):
        # Arrange
        mock_client = MagicMock()

        with open(TEST_PATH + "instruments.json") as response:
            instruments = json.load(response)

        mock_client.request.return_value = instruments

        provider = OandaInstrumentProvider(client=mock_client, account_id="001")

        # Act
        provider.load_all()

        # Assert
        self.assertTrue(provider.count > 0)  # No exceptions raised

    def test_get_all_when_not_loaded_returns_empty_dict(self):
        # Arrange
        mock_client = MagicMock()

        provider = OandaInstrumentProvider(client=mock_client, account_id="001")

        # Act
        instruments = provider.get_all()

        # Assert
        self.assertEqual({}, instruments)

    def test_get_all_when_loaded_returns_instruments(self):
        # Arrange
        mock_client = MagicMock()

        with open(TEST_PATH + "instruments.json") as response:
            instruments = json.load(response)

        mock_client.request.return_value = instruments

        provider = OandaInstrumentProvider(client=mock_client, account_id="001")
        provider.load_all()

        # Act
        instruments = provider.get_all()

        # Assert
        self.assertTrue(len(instruments) > 0)
        self.assertEqual(dict, type(instruments))
        self.assertEqual(InstrumentId, type(next(iter(instruments))))

    def test_get_audusd_when_not_loaded_returns_none(self):
        # Arrange
        mock_client = MagicMock()

        with open(TEST_PATH + "instruments.json") as response:
            instruments = json.load(response)

        mock_client.request.return_value = instruments

        provider = OandaInstrumentProvider(client=mock_client, account_id="001")

        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("OANDA"))

        # Act
        instrument = provider.find(instrument_id)

        # Assert
        self.assertIsNone(instrument)

    def test_get_audusd_when_loaded_returns_expected_instrument(self):
        # Arrange
        mock_client = MagicMock()

        with open(TEST_PATH + "instruments.json") as response:
            instruments = json.load(response)

        mock_client.request.return_value = instruments

        provider = OandaInstrumentProvider(client=mock_client, account_id="001", load_all=True)

        instrument_id = InstrumentId(Symbol("AUD/USD"), Venue("OANDA"))

        # Act
        instrument = provider.find(instrument_id)

        # Assert
        self.assertEqual(CFDInstrument, type(instrument))
        self.assertEqual(AssetClass.FX, instrument.asset_class)
        self.assertEqual(AssetType.CFD, instrument.asset_type)
        self.assertEqual(USD, instrument.quote_currency)
