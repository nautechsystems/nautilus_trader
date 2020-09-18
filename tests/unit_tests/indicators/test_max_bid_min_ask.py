# -------------------------------------------------------------------------------------------------
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
from datetime import datetime
from datetime import timedelta
import unittest

import pytz

from nautilus_trader.indicators.max_bid_min_ask import MaxBidMinAsk
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick


class MaxBidMinAskTests(unittest.TestCase):
    symbol = Symbol('SPY', Venue('TD Ameritrade'))

    def test_can_instantiate(self):
        # Arrange
        indicator = MaxBidMinAsk(self.symbol, timedelta(minutes=5))

        # Act
        # Assert
        self.assertEqual(None, indicator.max_bid)
        self.assertEqual(None, indicator.min_ask)
        self.assertEqual(False, indicator.initialized)

    def test_can_expire_items(self):
        # Arrange
        indicator = MaxBidMinAsk(self.symbol, timedelta(minutes=5))

        # Act + Assert
        indicator.handle_quote_tick(
            QuoteTick(
                self.symbol,
                Price(1.0, 0),
                Price(2.0, 0),
                Quantity(1),
                Quantity(1),
                datetime(2020, 1, 1, 0, 0, 0, tzinfo=pytz.utc),
            )
        )
        self.assertEqual(Price(1.0, 0), indicator.max_bid)
        self.assertEqual(Price(2.0, 0), indicator.min_ask)

        # 5 min later (still in the window)
        indicator.handle_quote_tick(
            QuoteTick(
                self.symbol,
                Price(0.9, 0),
                Price(2.1, 0),
                Quantity(1),
                Quantity(1),
                datetime(2020, 1, 1, 0, 5, 0, tzinfo=pytz.utc),
            )
        )
        self.assertEqual(Price(1.0, 0), indicator.max_bid)
        self.assertEqual(Price(2.0, 0), indicator.min_ask)

        # Allow the first item to expire out
        # This also tests that the new tick is the new min/max
        indicator.handle_quote_tick(
            QuoteTick(
                self.symbol,
                Price(0.95, 0),
                Price(2.05, 0),
                Quantity(1),
                Quantity(1),
                datetime(2020, 1, 1, 0, 5, 1, tzinfo=pytz.utc),
            )
        )
        self.assertEqual(Price(0.95, 0), indicator.max_bid)
        self.assertEqual(Price(2.05, 0), indicator.min_ask)
