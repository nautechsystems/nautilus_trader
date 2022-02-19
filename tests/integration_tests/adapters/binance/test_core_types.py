# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.binance.core.types import BinanceBar
from nautilus_trader.adapters.binance.core.types import BinanceSpotTicker
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import TestStubs


class TestBinanceDataTypes:
    def test_binance_ticker_repr(self):
        # Arrange
        ticker = BinanceSpotTicker(
            instrument_id=TestStubs.btcusdt_binance_id(),
            price_change=Decimal("-94.99999800"),
            price_change_percent=Decimal("-95.960"),
            weighted_avg_price=Decimal("0.29628482"),
            prev_close_price=Decimal("0.10002000"),
            last_price=Decimal("4.00000200"),
            last_qty=Decimal("200.00000000"),
            bid_price=Decimal("4.00000000"),
            ask_price=Decimal("4.00000200"),
            open_price=Decimal("99.00000000"),
            high_price=Decimal("100.00000000"),
            low_price=Decimal("0.10000000"),
            volume=Decimal("8913.30000000"),
            quote_volume=Decimal("15.30000000"),
            open_time_ms=1499783499040,
            close_time_ms=1499869899040,
            first_id=28385,
            last_id=28460,
            count=76,
            ts_event=1500000000000,
            ts_init=1500000000000,
        )

        # Act, Assert
        assert (
            repr(ticker)
            == "BinanceSpotTicker(instrument_id=BTC/USDT.BINANCE, price_change=-94.99999800, price_change_percent=-95.960, weighted_avg_price=0.29628482, prev_close_price=0.10002000, last_price=4.00000200, last_qty=200.00000000, bid_price=4.00000000, ask_price=4.00000200, open_price=99.00000000, high_price=100.00000000, low_price=0.10000000, volume=8913.30000000, quote_volume=15.30000000, open_time_ms=1499783499040, close_time_ms=1499869899040, first_id=28385, last_id=28460, count=76, ts_event=1500000000000, ts_init=1500000000000)"  # noqa
        )

    def test_binance_ticker_to_and_from_dict(self):
        # Arrange
        ticker = BinanceSpotTicker(
            instrument_id=TestStubs.btcusdt_binance_id(),
            price_change=Decimal("-94.99999800"),
            price_change_percent=Decimal("-95.960"),
            weighted_avg_price=Decimal("0.29628482"),
            prev_close_price=Decimal("0.10002000"),
            last_price=Decimal("4.00000200"),
            last_qty=Decimal("200.00000000"),
            bid_price=Decimal("4.00000000"),
            ask_price=Decimal("4.00000200"),
            open_price=Decimal("99.00000000"),
            high_price=Decimal("100.00000000"),
            low_price=Decimal("0.10000000"),
            volume=Decimal("8913.30000000"),
            quote_volume=Decimal("15.30000000"),
            open_time_ms=1499783499040,
            close_time_ms=1499869899040,
            first_id=28385,
            last_id=28460,
            count=76,
            ts_event=1500000000000,
            ts_init=1500000000000,
        )

        # Act
        values = ticker.to_dict(ticker)

        # Assert
        BinanceSpotTicker.from_dict(values)
        assert values == {
            "type": "BinanceSpotTicker",
            "instrument_id": "BTC/USDT.BINANCE",
            "price_change": "-94.99999800",
            "price_change_percent": "-95.960",
            "weighted_avg_price": "0.29628482",
            "prev_close_price": "0.10002000",
            "last_price": "4.00000200",
            "last_qty": "200.00000000",
            "bid_price": "4.00000000",
            "ask_price": "4.00000200",
            "open_price": "99.00000000",
            "high_price": "100.00000000",
            "low_price": "0.10000000",
            "volume": "8913.30000000",
            "quote_volume": "15.30000000",
            "open_time_ms": 1499783499040,
            "close_time_ms": 1499869899040,
            "first_id": 28385,
            "last_id": 28460,
            "count": 76,
            "ts_event": 1500000000000,
            "ts_init": 1500000000000,
        }

    def test_binance_bar_repr(self):
        # Arrange
        bar = BinanceBar(
            bar_type=BarType(
                instrument_id=TestStubs.btcusdt_binance_id(),
                bar_spec=TestStubs.bar_spec_1min_last(),
            ),
            open=Price.from_str("0.01634790"),
            high=Price.from_str("0.80000000"),
            low=Price.from_str("0.01575800"),
            close=Price.from_str("0.01577100"),
            volume=Quantity.from_str("148976.11427815"),
            quote_volume=Quantity.from_str("2434.19055334"),
            count=100,
            taker_buy_base_volume=Quantity.from_str("1756.87402397"),
            taker_buy_quote_volume=Quantity.from_str("28.46694368"),
            ts_event=1500000000000,
            ts_init=1500000000000,
        )

        # Act, Assert
        assert (
            repr(bar)
            == "BinanceBar(bar_type=BTC/USDT.BINANCE-1-MINUTE-LAST-EXTERNAL, open=0.01634790, high=0.80000000, low=0.01575800, close=0.01577100, volume=148976.11427815, quote_volume=2434.19055334, count=100, taker_buy_base_volume=1756.87402397, taker_buy_quote_volume=28.46694368, taker_sell_base_volume=147219.24025418, taker_sell_quote_volume=2405.72360966, ts_event=1500000000000,ts_init=1500000000000)"  # noqa
        )

    def test_binance_bar_to_from_dict(self):
        # Arrange
        bar = BinanceBar(
            bar_type=BarType(
                instrument_id=TestStubs.btcusdt_binance_id(),
                bar_spec=TestStubs.bar_spec_1min_last(),
            ),
            open=Price.from_str("0.01634790"),
            high=Price.from_str("0.80000000"),
            low=Price.from_str("0.01575800"),
            close=Price.from_str("0.01577100"),
            volume=Quantity.from_str("148976.11427815"),
            quote_volume=Quantity.from_str("2434.19055334"),
            count=100,
            taker_buy_base_volume=Quantity.from_str("1756.87402397"),
            taker_buy_quote_volume=Quantity.from_str("28.46694368"),
            ts_event=1500000000000,
            ts_init=1500000000000,
        )

        # Act
        values = bar.to_dict(bar)

        # Assert
        BinanceBar.from_dict(values)
        assert values == {
            "type": "BinanceBar",
            "bar_type": "BTC/USDT.BINANCE-1-MINUTE-LAST-EXTERNAL",
            "open": "0.01634790",
            "high": "0.80000000",
            "low": "0.01575800",
            "close": "0.01577100",
            "volume": "148976.11427815",
            "quote_volume": "2434.19055334",
            "count": 100,
            "taker_buy_base_volume": "1756.87402397",
            "taker_buy_quote_volume": "28.46694368",
            "ts_event": 1500000000000,
            "ts_init": 1500000000000,
        }
