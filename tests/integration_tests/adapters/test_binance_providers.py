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

import unittest

import ccxt

from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instrument import Instrument


# Requirements:
#    - An internet connection

class BinanceInstrumentProviderTests(unittest.TestCase):

    def test_instantiate_with_none_binance_client_raises_value_error(self):
        # Arrange
        client = ccxt.bitfinex

        # Act
        # Assert
        self.assertRaises(ValueError, BinanceInstrumentProvider, client)

    def test_load_all(self):
        # Arrange
        provider = BinanceInstrumentProvider()

        # Act
        provider.load_all()

        # Assert
        self.assertTrue(provider.count > 0)  # No exceptions raised

    def test_get_all_when_not_loaded_returns_empty_dict(self):
        # Arrange
        provider = BinanceInstrumentProvider()

        # Act
        instruments = provider.get_all()

        # Assert
        self.assertTrue(len(instruments) == 0)

    def test_get_all_when_loaded_returns_instruments(self):
        # Arrange
        provider = BinanceInstrumentProvider()
        provider.load_all()

        # Act
        instruments = provider.get_all()

        # Assert
        self.assertTrue(len(instruments) > 0)
        self.assertEqual(dict, type(instruments))
        self.assertEqual(Symbol, type(next(iter(instruments))))

    def test_get_btcusdt_when_not_loaded_returns_none(self):
        # Arrange
        provider = BinanceInstrumentProvider()

        symbol = Symbol("BTC/USDT", Venue("BINANCE"))

        # Act
        instrument = provider.get(symbol)

        # Assert
        self.assertIsNone(instrument)

    def test_get_btcusdt_when_loaded_returns_expected_instrument(self):
        # Arrange
        provider = BinanceInstrumentProvider()
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
