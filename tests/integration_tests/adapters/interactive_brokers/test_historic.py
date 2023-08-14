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
import sys

import pandas as pd
import pytest
import pytz
from ib_insync import IB

from nautilus_trader.adapters.interactive_brokers.historic import _bar_spec_to_hist_data_request
from nautilus_trader.adapters.interactive_brokers.historic import back_fill_catalog
from nautilus_trader.adapters.interactive_brokers.historic import parse_historic_bars
from nautilus_trader.adapters.interactive_brokers.historic import parse_historic_quote_ticks
from nautilus_trader.adapters.interactive_brokers.historic import parse_historic_trade_ticks
from nautilus_trader.adapters.interactive_brokers.historic import parse_response_datetime
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


@pytest.fixture()
def ib():
    return IB()


@pytest.fixture()
def catalog():
    return ParquetDataCatalog.from_uri("memory://")


@pytest.mark.skipif(sys.platform == "win32", reason="test path broken on Windows")
def test_back_fill_catalog_ticks(mocker, ib, catalog):
    # Arrange
    contract_details = IBTestProviderStubs.aapl_equity_contract_details()
    contract = IBTestDataStubs.contract()
    mocker.patch.object(ib, "reqContractDetails", return_value=[contract_details])
    mock_ticks = mocker.patch.object(ib, "reqHistoricalTicks", return_value=[])

    # Act
    back_fill_catalog(
        ib=ib,
        catalog=catalog,
        contracts=[IBTestDataStubs.contract()],
        start_date=datetime.date(2020, 1, 1),
        end_date=datetime.date(2020, 1, 2),
        tz_name="America/New_York",
        kinds=("BID_ASK", "TRADES"),
    )

    # Assert
    shared = {"numberOfTicks": 1000, "useRth": False, "endDateTime": ""}
    expected = [
        dict(
            contract=contract,
            startDateTime="20200101 05:00:00 UTC",
            whatToShow="BID_ASK",
            **shared,
        ),
        dict(
            contract=contract,
            startDateTime="20200101 05:00:00 UTC",
            whatToShow="TRADES",
            **shared,
        ),
        dict(
            contract=contract,
            startDateTime="20200102 05:00:00 UTC",
            whatToShow="BID_ASK",
            **shared,
        ),
        dict(
            contract=contract,
            startDateTime="20200102 05:00:00 UTC",
            whatToShow="TRADES",
            **shared,
        ),
    ]
    result = [call.kwargs for call in mock_ticks.call_args_list]
    assert result == expected


@pytest.mark.skipif(sys.platform == "win32", reason="test path broken on Windows")
def test_back_fill_catalog_bars(mocker, ib, catalog):
    # Arrange
    contract_details = IBTestProviderStubs.aapl_equity_contract_details()
    contract = IBTestDataStubs.contract()
    mocker.patch.object(ib, "reqContractDetails", return_value=[contract_details])
    mock_ticks = mocker.patch.object(ib, "reqHistoricalData", return_value=[])

    # Act
    back_fill_catalog(
        ib=ib,
        catalog=catalog,
        contracts=[IBTestDataStubs.contract()],
        start_date=datetime.date(2020, 1, 1),
        end_date=datetime.date(2020, 1, 2),
        tz_name="America/New_York",
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
        dict(contract=contract, endDateTime="20200102 05:00:00 UTC", **shared),
        dict(contract=contract, endDateTime="20200103 05:00:00 UTC", **shared),
    ]
    result = [call.kwargs for call in mock_ticks.call_args_list]
    assert result == expected


def test_parse_historic_trade_ticks():
    # Arrange
    raw = IBTestDataStubs.historic_trades()
    instrument = IBTestProviderStubs.aapl_instrument()

    # Act
    ticks = parse_historic_trade_ticks(historic_ticks=raw, instrument=instrument)

    # Assert
    assert all(isinstance(t, TradeTick) for t in ticks)

    expected = TradeTick.from_dict(
        {
            "type": "TradeTick",
            "instrument_id": "AAPL.NASDAQ",
            "price": "6.20",
            "size": "30",
            "aggressor_side": "NO_AGGRESSOR",
            "trade_id": "1646185673-6.2-30.0",
            "ts_event": 1646185673000000000,
            "ts_init": 1646185673000000000,
        },
    )
    assert ticks[0] == expected


def test_parse_historic_quote_ticks():
    # Arrange
    raw = IBTestDataStubs.historic_bid_ask()
    instrument = IBTestProviderStubs.aapl_instrument()

    # Act
    ticks = parse_historic_quote_ticks(historic_ticks=raw, instrument=instrument)

    # Assert
    assert all(isinstance(t, QuoteTick) for t in ticks)
    expected = QuoteTick.from_dict(
        {
            "type": "QuoteTick",
            "instrument_id": "AAPL.NASDAQ",
            "bid_price": "0.99",
            "ask_price": "15.30",
            "bid_size": "1",
            "ask_size": "1",
            "ts_event": 1646176203000000000,
            "ts_init": 1646176203000000000,
        },
    )
    assert ticks[0] == expected


def test_parse_historic_bar():
    # Arrange
    raw = IBTestDataStubs.historic_bars()
    instrument = IBTestProviderStubs.aapl_instrument()

    # Act
    ticks = parse_historic_bars(
        historic_bars=raw,
        instrument=instrument,
        kind="BARS-1-MINUTE-LAST",
    )

    # Assert
    assert all(isinstance(t, Bar) for t in ticks)
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
        },
    )
    assert ticks[0] == expected


@pytest.mark.parametrize(
    ("spec", "expected"),
    [
        (
            "1-SECOND-BID",  # For some reason 1 = secs but 1 = min
            {"durationStr": "1 D", "barSizeSetting": "1 secs", "whatToShow": "BID"},
        ),
        (
            "5-SECOND-BID",
            {"durationStr": "1 D", "barSizeSetting": "5 secs", "whatToShow": "BID"},
        ),
        (
            "5-MINUTE-LAST",
            {"durationStr": "1 D", "barSizeSetting": "5 mins", "whatToShow": "TRADES"},
        ),
        (
            "5-HOUR-LAST",
            {"durationStr": "1 D", "barSizeSetting": "5 hours", "whatToShow": "TRADES"},
        ),
        (
            "5-HOUR-MID",
            {"durationStr": "1 D", "barSizeSetting": "5 hours", "whatToShow": "MIDPOINT"},
        ),
        (
            "5-HOUR-MID",
            {"durationStr": "1 D", "barSizeSetting": "5 hours", "whatToShow": "MIDPOINT"},
        ),
        (
            "1-DAY-LAST",
            "Loading historic bars is for intraday data, bar_spec.aggregation should be ('SECOND', 'MINUTE', 'HOUR')",
        ),
        (
            "5-VOLUME-LAST",
            "Loading historic bars is for intraday data, bar_spec.aggregation should be ('SECOND', 'MINUTE', 'HOUR')",
        ),
    ],
)
def test_bar_spec_to_hist_data_request(spec: BarSpecification, expected):
    try:
        result = _bar_spec_to_hist_data_request(BarSpecification.from_str(spec))
    except AssertionError as exc:
        result = exc.args[0]
    assert result == expected


@pytest.mark.parametrize(
    "dt",
    [
        datetime.datetime(2019, 12, 31, 10, 5, 40),
        pd.Timestamp("2019-12-31 10:05:40"),
        pd.Timestamp("2019-12-31 10:05:40", tz="America/New_York"),
    ],
)
def test_parse_response_datetime(dt):
    result = parse_response_datetime(dt, tz_name="America/New_York")
    tz = pytz.timezone("America/New_York")
    expected = tz.localize(datetime.datetime(2019, 12, 31, 10, 5, 40))
    assert result == expected
