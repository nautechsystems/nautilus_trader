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
from unittest.mock import MagicMock

import pytest

from nautilus_trader.adapters.ccxt.providers import CCXTInstrumentProvider
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.currency import CurrencySpot
from tests import TESTS_PACKAGE_ROOT


# import ccxt  # uncomment to test against real API


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/ccxt/responses/"


# Monkey patch magic mock
# This allows the stubbing of calls to coroutines
MagicMock.__await__ = lambda x: async_magic().__await__()


# Dummy method for above
async def async_magic():
    return


class TestCCXTInstrumentProvider:
    @pytest.mark.skip  # Tests real API
    def test_real_api(self):
        import ccxt

        client = ccxt.binance()
        provider = CCXTInstrumentProvider(client=client)

        # Act
        provider.load_all()

        # Assert
        assert provider.count > 0  # No exceptions raised

    def test_load_all_when_decimal_precision_mode_exchange(self):
        # Arrange
        with open(TEST_PATH + "markets.json") as response:
            markets = json.load(response)

        with open(TEST_PATH + "currencies.json") as response:
            currencies = json.load(response)

        mock_client = MagicMock()
        mock_client.name = "Binance"
        mock_client.precisionMode = 2
        mock_client.markets = markets
        mock_client.currencies = currencies

        provider = CCXTInstrumentProvider(client=mock_client)

        # Act
        provider.load_all()

        # Assert
        assert provider.count == 1236  # No exceptions raised

    def test_load_all_when_tick_size_precision_mode_exchange(self):
        # Arrange
        with open(TEST_PATH + "markets2.json") as response:
            markets = json.load(response)

        with open(TEST_PATH + "currencies2.json") as response:
            currencies = json.load(response)

        mock_client = MagicMock()
        mock_client.name = "BitMEX"
        mock_client.precisionMode = 4
        mock_client.markets = markets
        mock_client.currencies = currencies

        provider = CCXTInstrumentProvider(client=mock_client)

        # Act
        provider.load_all()

        # Assert
        assert provider.count == 120  # No exceptions raised

    def test_load_all_async(self):
        # Fresh isolated loop testing pattern
        loop = asyncio.new_event_loop()
        asyncio.set_event_loop(loop)

        async def run_test():
            # Arrange
            with open(TEST_PATH + "markets.json") as response:
                markets = json.load(response)

            with open(TEST_PATH + "currencies.json") as response:
                currencies = json.load(response)

            mock_client = MagicMock()
            mock_client.name = "Binance"
            mock_client.precisionMode = 2
            mock_client.markets = markets
            mock_client.currencies = currencies

            provider = CCXTInstrumentProvider(client=mock_client)

            # Act
            await provider.load_all_async()
            await asyncio.sleep(0.5)

            # Assert
            assert provider.count > 0  # No exceptions raised

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
        assert instruments == {}

    def test_get_all_when_loaded_returns_instruments(self):
        # Arrange
        with open(TEST_PATH + "markets.json") as response:
            markets = json.load(response)

        with open(TEST_PATH + "currencies.json") as response:
            currencies = json.load(response)

        mock_client = MagicMock()
        mock_client.name = "Binance"
        mock_client.precisionMode = 2
        mock_client.markets = markets
        mock_client.currencies = currencies

        provider = CCXTInstrumentProvider(client=mock_client)
        provider.load_all()

        # Act
        instruments = provider.get_all()

        # Assert
        assert len(instruments) > 0
        assert type(instruments) == dict
        assert type(next(iter(instruments))) == InstrumentId

    def test_get_all_when_load_all_is_true_returns_expected_instruments(self):
        # Arrange
        with open(TEST_PATH + "markets.json") as response:
            markets = json.load(response)

        with open(TEST_PATH + "currencies.json") as response:
            currencies = json.load(response)

        mock_client = MagicMock()
        mock_client.name = "Binance"
        mock_client.precisionMode = 2
        mock_client.markets = markets
        mock_client.currencies = currencies

        provider = CCXTInstrumentProvider(client=mock_client, load_all=True)

        # Act
        instruments = provider.get_all()

        # Assert
        assert len(instruments) > 0
        assert type(instruments) == dict
        assert type(next(iter(instruments))) == InstrumentId

    def test_get_btcusdt_when_not_loaded_returns_none(self):
        # Arrange
        mock_client = MagicMock()
        mock_client.name = "Binance"

        provider = CCXTInstrumentProvider(client=mock_client)

        instrument_id = InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE"))

        # Act
        instrument = provider.find(instrument_id)

        # Assert
        assert instrument is None

    def test_get_btcusdt_when_loaded_returns_expected_instrument(self):
        # Arrange
        with open(TEST_PATH + "markets.json") as response:
            markets = json.load(response)

        with open(TEST_PATH + "currencies.json") as response:
            currencies = json.load(response)

        mock_client = MagicMock()
        mock_client.name = "Binance"
        mock_client.precisionMode = 2
        mock_client.markets = markets
        mock_client.currencies = currencies

        provider = CCXTInstrumentProvider(client=mock_client)
        provider.load_all()

        instrument_id = InstrumentId(Symbol("BTC/USDT"), Venue("BINANCE"))

        # Act
        instrument = provider.find(instrument_id)

        # Assert
        assert type(instrument) == CurrencySpot
        assert instrument.asset_class == AssetClass.CRYPTO
        assert instrument.asset_type == AssetType.SPOT
        assert instrument.base_currency == BTC
        assert instrument.quote_currency == USDT

    def test_get_btc_currency_when_loaded_returns_expected_currency(self):
        # Arrange
        with open(TEST_PATH + "markets.json") as response:
            markets = json.load(response)

        with open(TEST_PATH + "currencies.json") as response:
            currencies = json.load(response)

        mock_client = MagicMock()
        mock_client.name = "Binance"
        mock_client.precisionMode = 2
        mock_client.markets = markets
        mock_client.currencies = currencies

        provider = CCXTInstrumentProvider(client=mock_client)
        provider.load_all()

        # Act
        currency = provider.currency("BTC")

        # Assert
        assert type(currency) == Currency
        assert currency.code == "BTC"
        assert currency.precision == 8

    def test_get_random_currency_when_not_loaded_returns_none(self):
        # Arrange
        mock_client = MagicMock()
        mock_client.name = "Binance"
        mock_client.precisionMode = 2

        provider = CCXTInstrumentProvider(client=mock_client)
        provider.load_all()

        # Act
        currency = provider.currency("ZZZ")

        # Assert
        assert currency is None
