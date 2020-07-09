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
from datetime import datetime, timezone
from pandas import Timestamp

from nautilus_trader.trading.filters import ForexSession, ForexSessionFilter, EconomicNewsEventFilter
from tests.test_kit.stubs import UNIX_EPOCH


class ForexSessionFilterTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.session_filter = ForexSessionFilter()

    def test_local_from_utc_given_sydney_session_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.local_from_utc(ForexSession.SYDNEY, UNIX_EPOCH)

        # Assert
        self.assertEqual('1970-01-01 10:00:00+10:00', str(result))

    def test_local_from_utc_given_tokyo_session_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.local_from_utc(ForexSession.TOKYO, UNIX_EPOCH)

        # Assert
        self.assertEqual('1970-01-01 09:00:00+09:00', str(result))

    def test_local_from_utc_given_london_session_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.local_from_utc(ForexSession.LONDON, UNIX_EPOCH)

        # Assert
        self.assertEqual('1970-01-01 01:00:00+01:00', str(result))

    def test_local_from_utc_given_new_york_session_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.local_from_utc(ForexSession.NEW_YORK, UNIX_EPOCH)

        # Assert
        self.assertEqual('1969-12-31 19:00:00-05:00', str(result))

    def test_next_start_given_sydney_session_unix_epoch_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.next_start(ForexSession.SYDNEY, UNIX_EPOCH)

        # Assert
        self.assertEqual(datetime(1970, 1, 1, 21, 0, tzinfo=timezone.utc), result)

    def test_next_start_given_tokyo_session_unix_epoch_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.next_start(ForexSession.TOKYO, UNIX_EPOCH)

        # Assert
        self.assertEqual(datetime(1970, 1, 1, 0, 0, tzinfo=timezone.utc), result)

    def test_prev_start_given_london_session_unix_epoch_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.prev_start(ForexSession.LONDON, UNIX_EPOCH)

        # Assert
        self.assertEqual(datetime(1969, 12, 31, 7, 0, tzinfo=timezone.utc), result)

    def test_prev_start_given_new_york_session_unix_epoch_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.prev_start(ForexSession.NEW_YORK, UNIX_EPOCH)

        # Assert
        self.assertEqual(datetime(1969, 12, 31, 13, 0, tzinfo=timezone.utc), result)

    def test_next_end_given_new_york_session_unix_epoch_returns_expected_datetime(self):
        # Arrange
        # Act
        result = self.session_filter.next_end(ForexSession.NEW_YORK, UNIX_EPOCH)

        # Assert
        self.assertEqual(datetime(1970, 1, 1, 22, 0, tzinfo=timezone.utc), result)


class EconomicNewsEventFilterTests(unittest.TestCase):

    def test_can_initialize_filter(self):
        # Arrange
        currencies = ['USD', 'GBP']
        impacts = ['HIGH', 'MEDIUM']
        news_filter = EconomicNewsEventFilter(currencies=currencies, impacts=impacts)

        # Act
        # Assert
        self.assertEqual(Timestamp('2008-01-01 10:00:00+0000', tz='UTC'), news_filter.unfiltered_data_start)
        self.assertEqual(Timestamp('2020-12-31 23:00:00+0000', tz='UTC'), news_filter.unfiltered_data_end)
        self.assertEqual(currencies, news_filter.currencies)
        self.assertEqual(impacts, news_filter.impacts)

    def test_initialize_filter_with_no_currencies_or_impacts_returns_none(self):
        # Arrange
        currencies = []
        impacts = []
        news_filter = EconomicNewsEventFilter(currencies=currencies, impacts=impacts)

        # Act
        event_next = news_filter.next_event(datetime(2012, 3, 15, 12, 0, tzinfo=timezone.utc))
        event_prev = news_filter.next_event(datetime(2012, 3, 15, 12, 0, tzinfo=timezone.utc))

        # Assert
        self.assertIsNone(event_next)
        self.assertIsNone(event_prev)

    def test_next_event_given_time_now_before_data_raises_value_error(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        # Assert
        self.assertRaises(ValueError, news_filter.next_event, UNIX_EPOCH)

    def test_next_event_given_time_now_after_data_raises_value_error(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        # Assert
        self.assertRaises(ValueError, news_filter.next_event, datetime(2050, 1, 1, 1, 1, tzinfo=timezone.utc))

    def test_prev_event_given_time_now_before_data_raises_value_error(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        # Assert
        self.assertRaises(ValueError, news_filter.prev_event, UNIX_EPOCH)

    def test_prev_event_given_time_now_after_data_raises_value_error(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        # Assert
        self.assertRaises(ValueError, news_filter.prev_event, datetime(2050, 1, 1, 1, 1, tzinfo=timezone.utc))

    def test_next_event_given_valid_date_returns_expected_news_event(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        event = news_filter.prev_event(datetime(2015, 5, 10, 12, 0, tzinfo=timezone.utc))
        self.assertEqual(Timestamp('2015-05-08 12:30:00+0000', tz='UTC'), event.timestamp)

    def test_prev_event_given_valid_date_returns_expected_news_event(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        event = news_filter.prev_event(datetime(2017, 8, 10, 15, 0, tzinfo=timezone.utc))
        self.assertEqual(Timestamp('2017-08-04 12:30:00+0000', tz='UTC'), event.timestamp)
