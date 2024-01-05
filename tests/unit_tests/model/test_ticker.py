# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data import Ticker
from nautilus_trader.test_kit.providers import TestInstrumentProvider


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestTicker:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert Ticker.fully_qualified_name() == "nautilus_trader.model.data:Ticker"

    def test_ticker_hash_str_and_repr(self):
        # Arrange
        ticker = Ticker(
            ETHUSDT_BINANCE.id,
            0,
            0,
        )

        # Act, Assert
        assert isinstance(hash(ticker), int)
        assert str(ticker) == "Ticker(instrument_id=ETHUSDT.BINANCE, ts_event=0)"
        assert repr(ticker) == "Ticker(instrument_id=ETHUSDT.BINANCE, ts_event=0)"

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        ticker = Ticker(
            ETHUSDT_BINANCE.id,
            0,
            0,
        )

        # Act
        result = Ticker.to_dict(ticker)

        # Assert
        assert result == {
            "type": "Ticker",
            "instrument_id": "ETHUSDT.BINANCE",
            "ts_event": 0,
            "ts_init": 0,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        ticker = Ticker(
            ETHUSDT_BINANCE.id,
            0,
            0,
        )

        # Act
        result = Ticker.from_dict(Ticker.to_dict(ticker))

        # Assert
        assert result == ticker
