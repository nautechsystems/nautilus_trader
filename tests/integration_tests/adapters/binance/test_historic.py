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
import datetime
from unittest import mock

import pandas as pd
import pytest
import pytz

from nautilus_trader.adapters.binance.historic import back_fill_catalog
from nautilus_trader.adapters.binance.historic import parse_historic_bars
from nautilus_trader.adapters.binance.historic import parse_historic_trade_ticks
from nautilus_trader.adapters.binance.historic import parse_response_datetime
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from tests.integration_tests.adapters.binance.test_kit import BinanceTestStubs
from tests.test_kit.mocks.data import data_catalog_setup


class TestBinanceHistoric:
    def setup(self):
        data_catalog_setup()
        self.catalog = ParquetDataCatalog.from_env()
        self.client = mock.Mock()

    @pytest.mark.asyncio
    async def test_back_fill_catalog_ticks(self, mocker):
        # Arrange
        mocker.patch.object(self.client, "agg_trades", return_value=[])
        mock_trades = mocker.patch.object(self.client, "historic_trades", return_value=[])

        # Act
        # ERROR: async code not being run by pytest
        await back_fill_catalog(
            client=self.client,
            catalog=self.catalog,
            instruments=[BinanceTestStubs.instrument("BTCUSDT")],
            start_date=datetime.date(2020, 1, 1),
            end_date=datetime.date(2020, 1, 2),
            tz_name="UTC",
            kinds=("TRADES",),
        )

        # Assert
        expected = [
            dict(symbol="BTCUSDT"),
            dict(symbol="BTCUSDT"),
        ]

        result = [call.kwargs for call in mock_trades.call_args_list]
        assert result == expected

    @pytest.mark.asyncio
    async def test_back_fill_catalog_bars(self, mocker):
        # Arrange
        mock_bars = mocker.patch.object(self.client, "klines", return_value=[])

        # Act
        await back_fill_catalog(
            client=self.client,
            catalog=self.catalog,
            instruments=[BinanceTestStubs.instrument("BTCUSDT")],
            start_date=datetime.date(2020, 1, 1),
            end_date=datetime.date(2020, 1, 2),
            tz_name="UTC",
            kinds=("BARS-1-MINUTE-LAST",),
        )

        # Assert
        expected = [
            dict(symbol="BTCUSDT", interval="1m", start_time_ms=1640995200, end_time_ms=1641081599),
            dict(symbol="BTCUSDT", interval="1m", start_time_ms=1641081600, end_time_ms=1641167999),
        ]

        result = [call.kwargs for call in mock_bars.call_args_list]
        assert result == expected

    def test_parse_historic_bar(self):
        # Arrange
        raw = BinanceTestStubs.historic_bars()
        instrument = BinanceTestStubs.instrument(symbol="BTCUSDT")

        # Act
        ticks = parse_historic_bars(
            historic_bars=raw, instrument=instrument, kind="BARS-1-MINUTE-LAST"
        )

        # Assert
        assert all([isinstance(t, Bar) for t in ticks])

        expected = Bar.from_dict(
            {
                "type": "Bar",
                "bar_type": "BTCUSDT.BINANCE-1-MINUTE-LAST-EXTERNAL",
                "open": "60685.22",
                "high": "60729.84",
                "low": "60670.90",
                "close": "60719.27",
                "volume": "21.63272",
                "ts_event": 1634943780000000000,
                "ts_init": 1634943780000000000,
            }
        )
        assert ticks[0] == expected

    def test_parse_historic_trade_ticks(self):
        # Arrange
        raw = BinanceTestStubs.historic_trades()
        instrument_id = BinanceTestStubs.instrument(symbol="BTCUSDT").id

        # Act
        ticks = parse_historic_trade_ticks(historic_ticks=raw, instrument_id=instrument_id)

        # Assert
        assert all([isinstance(t, TradeTick) for t in ticks])

        expected = TradeTick.from_dict(
            {
                "type": "TradeTick",
                "instrument_id": "BTCUSDT.BINANCE",
                "price": "60814.78",
                "size": "0.00034",
                "aggressor_side": "BUY",
                "trade_id": "1111471896",
                "ts_event": 1634943219887000000,
                "ts_init": 1634943219887000000,
            }
        )
        assert ticks[0] == expected

    @pytest.mark.parametrize(
        "dt",
        [
            datetime.datetime(2019, 12, 31, 10, 5, 40),
            pd.Timestamp("2019-12-31 10:05:40"),
            pd.Timestamp("2019-12-31 10:05:40", tz="UTC"),
        ],
    )
    def test_parse_response_datetime(self, dt):
        result = parse_response_datetime(dt, tz_name="UTC")
        tz = pytz.timezone("UTC")
        expected = tz.localize(datetime.datetime(2019, 12, 31, 10, 5, 40))
        assert result == expected
