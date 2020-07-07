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
from datetime import datetime
from pandas import Timestamp

from nautilus_trader.trading.filters import EconomicNewsEventFilter
from tests.test_kit.stubs import UNIX_EPOCH


class EconomicNewsEventFilterTests(unittest.TestCase):

    def test_can_initialize_filter(self):
        # Arrange
        currencies = ['USD', 'GBP']
        impacts = ['HIGH', 'MEDIUM']
        news_filter = EconomicNewsEventFilter(currencies=currencies, impacts=impacts)

        # Act
        # Assert
        self.assertEqual(currencies, news_filter.currencies)
        self.assertEqual(impacts, news_filter.impacts)

    def test_next_event_given_impossible_date_returns_none(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        self.assertIsNone(news_filter.next_event(datetime(2050, 1, 1, 1, 1)))

    def test_prev_event_given_impossible_date_returns_none(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        self.assertIsNone(news_filter.prev_event(UNIX_EPOCH))

    def test_next_event_given_unix_epoch_returns_first_event_in_data(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        event = news_filter.next_event(UNIX_EPOCH)
        self.assertEqual(Timestamp('2015-01-02 15:00:00+0000', tz='UTC'), event.timestamp)

    def test_prev_event_given_valid_date_returns_expected_news_event(self):
        # Arrange
        news_filter = EconomicNewsEventFilter(currencies=['USD'], impacts=['HIGH'])

        # Act
        event = news_filter.prev_event(Timestamp('2017-08-10 15:00:00+0000', tz='UTC'))
        self.assertEqual(Timestamp('2017-08-04 12:30:00+0000', tz='UTC'), event.timestamp)
