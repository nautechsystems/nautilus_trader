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

from nautilus_trader.indicators.bid_ask_min_max import BidAskMinMax
from nautilus_trader.indicators.bid_ask_min_max import WindowedMinMaxPrices
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick


class BidAskMinMaxTests(unittest.TestCase):
    instrument_id = InstrumentId(Symbol("SPY"), Venue("NYSE"))

    def test_instantiate(self):
        # Arrange
        indicator = BidAskMinMax(self.instrument_id, timedelta(minutes=5))

        # Act
        # Assert
        self.assertEqual(None, indicator.bids.min_price)
        self.assertEqual(None, indicator.bids.max_price)
        self.assertEqual(None, indicator.asks.min_price)
        self.assertEqual(None, indicator.asks.max_price)
        self.assertEqual(False, indicator.initialized)

    def test_handle_quote_tick(self):
        # Arrange
        indicator = BidAskMinMax(self.instrument_id, timedelta(minutes=5))

        # Act
        indicator.handle_quote_tick(
            QuoteTick(
                self.instrument_id,
                Price.from_str("1.0"),
                Price.from_str("2.0"),
                Quantity.from_int(1),
                Quantity.from_int(1),
                0,
            )
        )
        # 5 min later (still in the window)
        indicator.handle_quote_tick(
            QuoteTick(
                self.instrument_id,
                Price.from_str("0.9"),
                Price.from_str("2.1"),
                Quantity.from_int(1),
                Quantity.from_int(1),
                3e11,
            )
        )

        # Assert
        self.assertEqual(Price.from_str("0.9"), indicator.bids.min_price)
        self.assertEqual(Price.from_str("1.0"), indicator.bids.max_price)
        self.assertEqual(Price.from_str("2.1"), indicator.asks.min_price)
        self.assertEqual(Price.from_str("2.1"), indicator.asks.max_price)

    def test_reset(self):
        # Arrange
        indicator = BidAskMinMax(self.instrument_id, timedelta(minutes=5))

        indicator.handle_quote_tick(
            QuoteTick(
                self.instrument_id,
                Price.from_str("0.9"),
                Price.from_str("2.1"),
                Quantity.from_int(1),
                Quantity.from_int(1),
                0,
            )
        )

        # Act
        indicator.reset()

        # Assert
        self.assertIsNone(indicator.bids.min_price)
        self.assertIsNone(indicator.asks.min_price)


class WindowedMinMaxPricesTests(unittest.TestCase):
    def test_instantiate(self):
        # Arrange
        instance = WindowedMinMaxPrices(timedelta(minutes=5))

        # Act
        # Assert
        self.assertEqual(None, instance.min_price)
        self.assertEqual(None, instance.max_price)

    def test_add_price(self):
        # Arrange
        instance = WindowedMinMaxPrices(timedelta(minutes=5))

        # Act
        instance.add_price(
            datetime(2020, 1, 1, 0, 0, 0, tzinfo=pytz.utc),
            Price.from_str("1.0"),
        )
        # Assert
        self.assertEqual(Price.from_str("1.0"), instance.min_price)
        self.assertEqual(Price.from_str("1.0"), instance.max_price)

    def test_add_multiple_prices(self):
        # Arrange
        instance = WindowedMinMaxPrices(timedelta(minutes=5))

        # Act
        instance.add_price(
            datetime(2020, 1, 1, 0, 0, 0, tzinfo=pytz.utc),
            Price.from_str("1.0"),
        )
        # 5 min later (still in the window)
        instance.add_price(
            datetime(2020, 1, 1, 0, 5, 0, tzinfo=pytz.utc),
            Price.from_str("0.9"),
        )

        # Assert
        self.assertEqual(Price.from_str("0.9"), instance.min_price)
        self.assertEqual(Price.from_str("1.0"), instance.max_price)

    def test_expire_items(self):
        # Arrange
        instance = WindowedMinMaxPrices(timedelta(minutes=5))

        # Act
        instance.add_price(
            datetime(2020, 1, 1, 0, 0, 0, tzinfo=pytz.utc),
            Price.from_str("1.0"),
        )
        # 5 min later (still in the window)
        instance.add_price(
            datetime(2020, 1, 1, 0, 5, 0, tzinfo=pytz.utc),
            Price.from_str("0.9"),
        )
        # Allow the first item to expire out
        # This also tests that the new tick is the new min/max
        instance.add_price(
            datetime(2020, 1, 1, 0, 5, 1, tzinfo=pytz.utc),
            Price.from_str("0.95"),
        )

        # Assert
        self.assertEqual(Price.from_str("0.95"), instance.min_price)
        self.assertEqual(Price.from_str("0.95"), instance.max_price)

    def test_reset(self):
        # Arrange
        instance = WindowedMinMaxPrices(timedelta(minutes=5))

        # Act
        instance.add_price(
            datetime(2020, 1, 1, 0, 0, 0, tzinfo=pytz.utc),
            Price.from_str("1"),
        )
        instance.reset()

        # Assert
        self.assertEqual(None, instance.min_price)
        self.assertEqual(None, instance.max_price)
