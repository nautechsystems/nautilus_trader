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

from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.providers import TestInstrumentProvider


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestQuoteTick:
    def test_ticker_hash_str_and_repr(self):
        # Arrange
        ticker = Ticker(
            ETHUSDT_BINANCE.id,
            0,
            0,
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            Price.from_str("10000.00000000"),
            Price.from_str("10000.00000000"),
            Quantity.from_str("100"),
            Quantity.from_str("100"),
            Price.from_str("10000.00000000"),
            Quantity.from_str("50"),
        )

        # Act, Assert
        assert isinstance(hash(ticker), int)
        assert (
            str(ticker)
            == "Ticker(instrument_id=ETH/USDT.BINANCE, volume_quote=100000, volume_base=100000, bid=10000.00000000, ask=10000.00000000, bid_size=100, ask_size=100, last_px=10000.00000000, last_qty=50, ts_event=0, info=None)"  # noqa
        )
        assert (
            repr(ticker)
            == "Ticker(instrument_id=ETH/USDT.BINANCE, volume_quote=100000, volume_base=100000, bid=10000.00000000, ask=10000.00000000, bid_size=100, ask_size=100, last_px=10000.00000000, last_qty=50, ts_event=0, info=None)"  # noqa
        )

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        ticker = Ticker(
            ETHUSDT_BINANCE.id,
            0,
            0,
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            Price.from_str("10000.00000000"),
            Price.from_str("10000.00000000"),
            Quantity.from_str("100"),
            Quantity.from_str("100"),
            Price.from_str("10000.00000000"),
            Quantity.from_str("50"),
        )

        # Act
        result = Ticker.to_dict(ticker)

        # Assert
        assert result == {
            "type": "Ticker",
            "instrument_id": "ETH/USDT.BINANCE",
            "volume_quote": "100000",
            "volume_base": "100000",
            "bid": "10000.00000000",
            "ask": "10000.00000000",
            "bid_size": "100",
            "ask_size": "100",
            "last_px": "10000.00000000",
            "last_qty": "50",
            "ts_event": 0,
            "ts_init": 0,
            "info": None,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        ticker = Ticker(
            ETHUSDT_BINANCE.id,
            0,
            0,
            Quantity.from_int(100000),
            Quantity.from_int(100000),
            Price.from_str("10000.00000000"),
            Price.from_str("10000.00000000"),
            Quantity.from_str("100"),
            Quantity.from_str("100"),
            Price.from_str("10000.00000000"),
            Quantity.from_str("50"),
        )

        # Act
        result = Ticker.from_dict(Ticker.to_dict(ticker))

        # Assert
        assert ticker == result
