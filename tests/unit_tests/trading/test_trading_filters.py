# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import os
from datetime import datetime

import pandas as pd
import pytest
import pytz

from nautilus_trader.core.datetime import as_utc_index
from nautilus_trader.test_kit.stubs import UNIX_EPOCH
from nautilus_trader.trading.filters import EconomicNewsEventFilter
from nautilus_trader.trading.filters import ForexSession
from nautilus_trader.trading.filters import ForexSessionFilter
from tests import TEST_DATA_DIR


class TestForexSessionFilter:
    def setup(self):
        # Fixture Setup
        self.session_filter = ForexSessionFilter()

    @pytest.mark.parametrize(
        ("session", "expected"),
        [
            [ForexSession.SYDNEY, "1970-01-01 10:00:00+10:00"],
            [ForexSession.TOKYO, "1970-01-01 09:00:00+09:00"],
            [ForexSession.LONDON, "1970-01-01 01:00:00+01:00"],
            [ForexSession.NEW_YORK, "1969-12-31 19:00:00-05:00"],
        ],
    )
    def test_local_from_utc_given_various_sessions_returns_expected_datetime(
        self,
        session,
        expected,
    ):
        # Arrange, Act
        result = self.session_filter.local_from_utc(session, UNIX_EPOCH)

        # Assert
        assert str(result) == expected

    @pytest.mark.parametrize(
        ("session", "expected"),
        [
            [ForexSession.SYDNEY, datetime(1970, 1, 1, 21, 0, tzinfo=pytz.utc)],
            [ForexSession.TOKYO, datetime(1970, 1, 1, 0, 0, tzinfo=pytz.utc)],
            [ForexSession.LONDON, datetime(1970, 1, 1, 7, 0, tzinfo=pytz.utc)],
            [ForexSession.NEW_YORK, datetime(1970, 1, 1, 13, 0, tzinfo=pytz.utc)],
        ],
    )
    def test_next_start_given_various_sessions_returns_expected_datetime(self, session, expected):
        # Arrange, Act
        result = self.session_filter.next_start(session, UNIX_EPOCH)

        # Assert
        assert result == expected

    def test_next_start_on_weekend_returns_expected_datetime_monday(self):
        # Arrange, Act
        time_now = datetime(2020, 7, 12, 9, 0, tzinfo=pytz.utc)
        result = self.session_filter.next_start(ForexSession.TOKYO, time_now)

        # Assert
        assert result == datetime(2020, 7, 13, 0, 0, tzinfo=pytz.utc)

    def test_next_in_session_returns_expected_datetime_next_day(self):
        # Arrange, Act
        time_now = datetime(2020, 7, 13, 1, 0, tzinfo=pytz.utc)
        result = self.session_filter.next_start(ForexSession.TOKYO, time_now)

        # Assert
        assert result == datetime(2020, 7, 14, 0, 0, tzinfo=pytz.utc)

    @pytest.mark.parametrize(
        ("session", "expected"),
        [
            [ForexSession.SYDNEY, datetime(1969, 12, 31, 21, 0, tzinfo=pytz.utc)],
            [ForexSession.TOKYO, datetime(1970, 1, 1, 0, 0, tzinfo=pytz.utc)],
            [ForexSession.LONDON, datetime(1969, 12, 31, 7, 0, tzinfo=pytz.utc)],
            [ForexSession.NEW_YORK, datetime(1969, 12, 31, 13, 0, tzinfo=pytz.utc)],
        ],
    )
    def test_prev_start_given_various_sessions_returns_expected_datetime(self, session, expected):
        # Arrange, Act
        result = self.session_filter.prev_start(session, UNIX_EPOCH)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("session", "expected"),
        [
            [ForexSession.SYDNEY, datetime(1970, 1, 1, 6, 0, tzinfo=pytz.utc)],
            [ForexSession.TOKYO, datetime(1970, 1, 1, 9, 0, tzinfo=pytz.utc)],
            [ForexSession.LONDON, datetime(1970, 1, 1, 15, 0, tzinfo=pytz.utc)],
            [ForexSession.NEW_YORK, datetime(1970, 1, 1, 22, 0, tzinfo=pytz.utc)],
        ],
    )
    def test_next_end_given_various_sessions_returns_expected_datetime(self, session, expected):
        # Arrange, Act
        result = self.session_filter.next_end(session, UNIX_EPOCH)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("session", "expected"),
        [
            [ForexSession.SYDNEY, datetime(1969, 12, 31, 6, 0, tzinfo=pytz.utc)],
            [ForexSession.TOKYO, datetime(1969, 12, 31, 9, 0, tzinfo=pytz.utc)],
            [ForexSession.LONDON, datetime(1969, 12, 31, 15, 0, tzinfo=pytz.utc)],
            [ForexSession.NEW_YORK, datetime(1969, 12, 31, 22, 0, tzinfo=pytz.utc)],
        ],
    )
    def test_prev_end_given_various_sessions_returns_expected_datetime(self, session, expected):
        # Arrange, Act
        result = self.session_filter.prev_end(session, UNIX_EPOCH)

        # Assert
        assert result == expected


class TestEconomicNewsEventFilter:
    def setup(self):
        # Fixture Setup
        news_csv_path = os.path.join(TEST_DATA_DIR, "news_events.csv")
        self.news_data = as_utc_index(pd.read_csv(news_csv_path, parse_dates=True, index_col=0))

    def test_initialize_filter(self):
        # Arrange
        currencies = ["USD", "GBP"]
        impacts = ["HIGH", "MEDIUM"]
        news_filter = EconomicNewsEventFilter(
            currencies=currencies,
            impacts=impacts,
            news_data=self.news_data,
        )

        # Act, Assert
        assert (
            pd.Timestamp("2008-01-01 10:00:00+0000", tz="UTC") == news_filter.unfiltered_data_start
        )
        assert pd.Timestamp("2020-12-31 23:00:00+0000", tz="UTC") == news_filter.unfiltered_data_end
        assert news_filter.currencies == currencies
        assert news_filter.impacts == impacts

    def test_initialize_filter_with_no_currencies_or_impacts_returns_none(self):
        # Arrange
        currencies = []
        impacts = []
        news_filter = EconomicNewsEventFilter(
            currencies=currencies,
            impacts=impacts,
            news_data=self.news_data,
        )

        # Act
        event_next = news_filter.next_event(datetime(2012, 3, 15, 12, 0, tzinfo=pytz.utc))
        event_prev = news_filter.next_event(datetime(2012, 3, 15, 12, 0, tzinfo=pytz.utc))

        # Assert
        assert event_next is None
        assert event_prev is None

    def test_next_event_given_time_now_before_data_raises_value_error(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(
            currencies=["USD"],
            impacts=["HIGH"],
            news_data=self.news_data,
        )

        # Act, Assert
        with pytest.raises(ValueError):
            news_filter.next_event(UNIX_EPOCH)

    def test_next_event_given_time_now_after_data_raises_value_error(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(
            currencies=["USD"],
            impacts=["HIGH"],
            news_data=self.news_data,
        )

        # Act, Assert
        with pytest.raises(ValueError):
            news_filter.next_event(datetime(2050, 1, 1, 1, 1, tzinfo=pytz.utc))

    def test_prev_event_given_time_now_before_data_raises_value_error(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(
            currencies=["USD"],
            impacts=["HIGH"],
            news_data=self.news_data,
        )

        # Act, Assert
        with pytest.raises(ValueError):
            news_filter.prev_event(UNIX_EPOCH)

    def test_prev_event_given_time_now_after_data_raises_value_error(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(
            currencies=["USD"],
            impacts=["HIGH"],
            news_data=self.news_data,
        )

        # Act, Assert
        with pytest.raises(ValueError):
            news_filter.prev_event(datetime(2050, 1, 1, 1, 1, tzinfo=pytz.utc))

    def test_next_event_given_valid_date_returns_expected_news_event(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(
            currencies=["USD"],
            impacts=["HIGH"],
            news_data=self.news_data,
        )

        # Act
        event = news_filter.prev_event(datetime(2015, 5, 10, 12, 0, tzinfo=pytz.utc))
        assert event.ts_event == 1431088200000000000

    def test_prev_event_given_valid_date_returns_expected_news_event(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(
            currencies=["USD"],
            impacts=["HIGH"],
            news_data=self.news_data,
        )

        # Act
        event = news_filter.prev_event(datetime(2017, 8, 10, 15, 0, tzinfo=pytz.utc))
        assert event.ts_event == 1501849800000000000
