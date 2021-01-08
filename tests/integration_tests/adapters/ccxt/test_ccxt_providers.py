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

import asyncio
import json
import unittest
from unittest.mock import MagicMock

from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instrument import Instrument
from tests import PACKAGE_ROOT


# import ccxt  # uncomment to test against real API


TEST_PATH = PACKAGE_ROOT + "/integration_tests/adapters/ccxt/"


# Monkey patch magic mock
# This allows the stubbing of calls to coroutines
MagicMock.__await__ = lambda x: async_magic().__await__()


# Dummy method for above
async def async_magic():
    return


class CCXTInstrumentProviderTests(unittest.TestCase):

    # Uncomment to test real API
    # def test_real_api(self):
    #     client = ccxt.bitfinex()
    #     provider = CCXTInstrumentProvider(client=client)
    #
    #     # Act
    #     provider.load_all()
    #
    #     # Assert
    #     self.assertTrue(provider.count > 0)  # No exceptions raised

    def test_load_all(self):
        # Arrange
        mock_client = MagicMock()
        mock_client.name = "Binance"

        with open(TEST_PATH + "res_instruments.json") as response:
            instruments = json.load(response)

        mock_client.markets = instruments

        provider = CCXTInstrumentProvider(client=mock_client)

        # Act
        provider.load_all()

        # Assert
        self.assertTrue(provider.count > 0)  # No exceptions raised

    def test_load_all_async(self):
        # Fresh isolated loop testing pattern
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)

        async def run_test():
            # Arrange
            with open(TEST_PATH + "res_instruments.json") as response:
                instruments = json.load(response)

            mock_client = MagicMock()
            mock_client.name = "Binance"
            mock_client.markets = instruments

            provider = CCXTInstrumentProvider(client=mock_client)

            # Act
            await provider.load_all_async()
            await asyncio.sleep(0.5)

            # Assert
            self.assertTrue(provider.count > 0)  # No exceptions raised

        loop.run_until_complete(run_test())
        loop.stop()
        loop.close()

    def test_get_all_when_not_loaded_returns_empty_dict(self):
        # Arrange
        mock_client = MagicMock()
        mock_client.name = "Binance"

        provider = CCXTInstrumentProvider(client=mock_client)

        # Act
        instruments = provider.get_all()

        # Assert
        self.assertTrue(len(instruments) == 0)

    def test_get_all_when_loaded_returns_instruments(self):
        # Arrange
        mock_client = MagicMock()
        mock_client.name = "Binance"

        with open(TEST_PATH + "res_instruments.json") as response:
            instruments = json.load(response)

        mock_client.markets = instruments

        provider = CCXTInstrumentProvider(client=mock_client)
        provider.load_all()

        # Act
        instruments = provider.get_all()

        # Assert
        self.assertTrue(len(instruments) > 0)
        self.assertEqual(dict, type(instruments))
        self.assertEqual(Symbol, type(next(iter(instruments))))

    def test_get_btcusdt_when_not_loaded_returns_none(self):
        # Arrange
        mock_client = MagicMock()
        mock_client.name = "Binance"

        provider = CCXTInstrumentProvider(client=mock_client)

        symbol = Symbol("BTC/USDT", Venue("BINANCE"))

        # Act
        instrument = provider.get(symbol)

        # Assert
        self.assertIsNone(instrument)

    def test_get_btcusdt_when_loaded_returns_expected_instrument(self):
        # Arrange
        mock_client = MagicMock()
        mock_client.name = "Binance"

        with open(TEST_PATH + "res_instruments.json") as response:
            instruments = json.load(response)

        mock_client.markets = instruments

        provider = CCXTInstrumentProvider(client=mock_client)
        provider.load_all()

        symbol = Symbol("BTC/USDT", Venue("BINANCE"))

        # Act
        instrument = provider.get(symbol)

        # Assert
        self.assertEqual(Instrument, type(instrument))
        self.assertEqual(AssetClass.CRYPTO, instrument.asset_class)
        self.assertEqual(AssetType.SPOT, instrument.asset_type)
        self.assertEqual(BTC, instrument.base_currency)
        self.assertEqual(USDT, instrument.quote_currency)
        self.assertEqual(USDT, instrument.settlement_currency)
