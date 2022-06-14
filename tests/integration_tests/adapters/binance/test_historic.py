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
from nautilus_trader.adapters.binance.historic import parse_response_datetime
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.persistence.catalog import DataCatalog
from tests.integration_tests.adapters.binance.test_kit import BinanceTestStubs
from tests.test_kit.mocks.data import data_catalog_setup


class TestBinanceHistoric:
    def setup(self):
        data_catalog_setup()
        self.catalog = DataCatalog.from_env()
        self.client = mock.Mock()

    def test_back_fill_catalog_bars(self, mocker):
        # Arrange
        instrument = BinanceTestStubs.instrument()
        mocker.patch.object(self.client, "reqContractDetails", return_value=[instrument])
        mock_ticks = mocker.patch.object(self.client, "reqHistoricalData", return_value=[])

        # Act
        back_fill_catalog(
            client=self.client,
            catalog=self.catalog,
            instruments=[BinanceTestStubs.instument()],
            start_date=datetime.date(2020, 1, 1),
            end_date=datetime.date(2020, 1, 2),
            tz_name="UTC",
            kinds=("BARS-1-MINUTE-LAST",),
        )

        # Assert
        shared = {
            "barSizeSetting": "1 min",
            "durationStr": "1 D",
            "useRTH": False,
            "whatToShow": "TRADES",
            "formatDate": 2,
        }
        expected = [
            dict(instrument=instrument, endDateTime="20200102 05:00:00 UTC", **shared),
            dict(instrument=instrument, endDateTime="20200103 05:00:00 UTC", **shared),
        ]
        result = [call.kwargs for call in mock_ticks.call_args_list]
        assert result == expected

    def test_parse_historic_bar(self):
        # Arrange
        raw = BinanceTestStubs.historic_bars()
        instrument = BinanceTestStubs.instrument(symbol="AAPL")

        # Act
        ticks = parse_historic_bars(
            historic_bars=raw, instrument=instrument, kind="BARS-1-MINUTE-LAST"
        )

        # Assert
        assert all([isinstance(t, Bar) for t in ticks])

        expected = Bar.from_dict(
            {
                "type": "Bar",
                "bar_type": "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL",
                "open": "219.00",
                "high": "219.00",
                "low": "219.00",
                "close": "219.00",
                "volume": "1",
                "ts_event": 1609838880000000000,
                "ts_init": 1609838880000000000,
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
