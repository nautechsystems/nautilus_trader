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

from nautilus_trader.indicators.utils.windowed_min_max_prices import (
    WindowedMinMaxPrices,
)
from nautilus_trader.model.objects import Price


class WindowedMinMaxPricesTests(unittest.TestCase):
    def test_can_instantiate(self):
        # Arrange
        instance = WindowedMinMaxPrices(timedelta(minutes=5))

        # Act
        # Assert
        self.assertEqual(None, instance.min_price)
        self.assertEqual(None, instance.max_price)

    def test_can_expire_items(self):
        # Arrange
        instance = WindowedMinMaxPrices(timedelta(minutes=5))
        # Act
        instance.add_price(
            datetime(2020, 1, 1, 0, 0, 0, tzinfo=pytz.utc),
            Price(1.0, 0),
        )
        # Assert
        self.assertEqual(Price(1.0, 0), instance.min_price)
        self.assertEqual(Price(1.0, 0), instance.max_price)

        # 5 min later (still in the window)
        # Act
        instance.add_price(
            datetime(2020, 1, 1, 0, 5, 0, tzinfo=pytz.utc),
            Price(0.9, 0),
        )
        # Assert
        self.assertEqual(Price(0.9, 0), instance.min_price)
        self.assertEqual(Price(1.0, 0), instance.max_price)

        # Allow the first item to expire out
        # This also tests that the new tick is the new min/max
        # Act
        instance.add_price(
            datetime(2020, 1, 1, 0, 5, 1, tzinfo=pytz.utc),
            Price(0.95, 0),
        )
        # Assert
        self.assertEqual(Price(0.90, 0), instance.min_price)
        self.assertEqual(Price(0.95, 0), instance.max_price)
