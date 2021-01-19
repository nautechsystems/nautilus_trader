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

from datetime import timedelta
import unittest

from nautilus_trader.adapters.ccxt.exchanges.bitmex import BitmexOrderRequestBuilder
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.stubs import UNIX_EPOCH


BINANCE = Venue("BINANCE")
BTCUSDT = Symbol("BTC/USDT", BINANCE)


class BitmexOrderBuilderTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_factory = OrderFactory(
            trader_id=TraderId("TESTER", "000"),
            strategy_id=StrategyId("S", "001"),
            clock=TestClock(),
        )

    def test_order_with_gtd_tif_raises_value_error(self):
        # Arrange
        order = self.order_factory.limit(
            symbol=BTCUSDT,
            order_side=OrderSide.BUY,
            quantity=Quantity("1.0"),
            price=Price("50000"),
            time_in_force=TimeInForce.GTD,
            expire_time=UNIX_EPOCH + timedelta(minutes=1),
            post_only=True,
        )

        self.assertRaises(ValueError, BitmexOrderRequestBuilder.build_py, order)

    def test_market_order(self):
        # Arrange
        order = self.order_factory.market(
            symbol=BTCUSDT,
            order_side=OrderSide.BUY,
            quantity=Quantity("0.10000000"),
        )

        # Act
        result = BitmexOrderRequestBuilder.build_py(order)

        # Assert
        expected = {
            'clOrdID': 'O-19700101-000000-000-001-1',
            'timeInForce': 'GoodTillCancel',
            'type': 'Market',
        }
        self.assertEqual(expected, result)

    def test_limit_buy_post_only_reduce_only_order(self):
        # Arrange
        order = self.order_factory.limit(
            symbol=BTCUSDT,
            order_side=OrderSide.BUY,
            quantity=Quantity("1.0"),
            price=Price("50000"),
            post_only=True,
            reduce_only=True,
        )

        # Act
        result = BitmexOrderRequestBuilder.build_py(order)

        # Assert
        expected = {
            'clOrdID': 'O-19700101-000000-000-001-1',
            'execInst': 'ParticipateDoNotInitiate,ReduceOnly',
            'timeInForce': 'GoodTillCancel',
            'type': 'Limit',
        }
        self.assertEqual(expected, result)

    def test_limit_sell_hidden_reduce_only_order(self):
        # Arrange
        order = self.order_factory.limit(
            symbol=BTCUSDT,
            order_side=OrderSide.BUY,
            quantity=Quantity("1.0"),
            price=Price("50000"),
            post_only=False,
            reduce_only=True,
            hidden=True,
        )

        # Act
        result = BitmexOrderRequestBuilder.build_py(order)

        # Assert
        expected = {
            'clOrdID': 'O-19700101-000000-000-001-1',
            'displayQty': 0,
            'execInst': 'ReduceOnly',
            'timeInForce': 'GoodTillCancel',
            'type': 'Limit',
        }
        self.assertEqual(expected, result)

    def test_limit_sell_hidden(self):
        # Arrange
        order = self.order_factory.limit(
            symbol=BTCUSDT,
            order_side=OrderSide.BUY,
            quantity=Quantity("1.0"),
            price=Price("50000"),
            time_in_force=TimeInForce.IOC,
            post_only=False,
            hidden=True,
        )

        # Act
        result = BitmexOrderRequestBuilder.build_py(order)

        # Assert
        expected = {
            'clOrdID': 'O-19700101-000000-000-001-1',
            'displayQty': 0,
            'timeInForce': 'ImmediateOrCancel',
            'type': 'Limit',
        }
        self.assertEqual(expected, result)

    def test_limit_buy_ioc(self):
        # Arrange
        order = self.order_factory.limit(
            symbol=BTCUSDT,
            order_side=OrderSide.BUY,
            quantity=Quantity("1.0"),
            price=Price("50000"),
            time_in_force=TimeInForce.IOC,
            post_only=False,
        )

        # Act
        result = BitmexOrderRequestBuilder.build_py(order)

        # Assert
        expected = {
            'clOrdID': 'O-19700101-000000-000-001-1',
            'timeInForce': 'ImmediateOrCancel',
            'type': 'Limit',
        }
        self.assertEqual(expected, result)

    def test_limit_sell_fok_order(self):
        # Arrange
        order = self.order_factory.limit(
            symbol=BTCUSDT,
            order_side=OrderSide.SELL,
            quantity=Quantity("1.0"),
            price=Price("50000"),
            time_in_force=TimeInForce.FOK,
            post_only=False,
        )

        # Act
        result = BitmexOrderRequestBuilder.build_py(order)

        # Assert
        expected = {
            'clOrdID': 'O-19700101-000000-000-001-1',
            'timeInForce': 'FillOrKill',
            'type': 'Limit',
        }
        self.assertEqual(expected, result)

    def test_stop_market_buy_order(self):
        # Arrange
        order = self.order_factory.stop_market(
            symbol=BTCUSDT,
            order_side=OrderSide.SELL,
            quantity=Quantity("1.0"),
            price=Price("100000"),
            time_in_force=TimeInForce.GTC,
        )

        # Act
        result = BitmexOrderRequestBuilder.build_py(order)

        # Assert
        expected = {
            'clOrdID': 'O-19700101-000000-000-001-1',
            'stopPx': '100000',
            'timeInForce': 'GoodTillCancel',
            'type': 'StopMarket',
        }
        self.assertEqual(expected, result)

    def test_stop_market_sell_reduce_only_order(self):
        # Arrange
        order = self.order_factory.stop_market(
            symbol=BTCUSDT,
            order_side=OrderSide.SELL,
            quantity=Quantity("1.0"),
            price=Price("100000"),
            time_in_force=TimeInForce.GTC,
        )

        # Act
        result = BitmexOrderRequestBuilder.build_py(order)

        # Assert
        expected = {
            'clOrdID': 'O-19700101-000000-000-001-1',
            'stopPx': '100000',
            'timeInForce': 'GoodTillCancel',
            'type': 'StopMarket',
        }
        self.assertEqual(expected, result)
