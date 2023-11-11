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

import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderStatus
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.test_kit.providers import TestInstrumentProvider


ETHUSDT_BYBIT = TestInstrumentProvider.ethusdt_binance()


class TestBybitParsing:
    def setup(self):
        self._enum_parser = BybitEnumParser()
        self.instrument: str = "ETHUSDT.BINANCE"

    @pytest.mark.parametrize(
        ("bar_type", "bybit_kline_interval"),
        [
            ["ETHUSDT.BYBIT-1-MINUTE-LAST-EXTERNAL", "1"],
            ["ETHUSDT.BYBIT-3-MINUTE-LAST-EXTERNAL", "3"],
            ["ETHUSDT.BYBIT-5-MINUTE-LAST-EXTERNAL", "5"],
            ["ETHUSDT.BYBIT-15-MINUTE-LAST-EXTERNAL", "15"],
            ["ETHUSDT.BYBIT-30-MINUTE-LAST-EXTERNAL", "30"],
            ["ETHUSDT.BYBIT-1-HOUR-LAST-EXTERNAL", "60"],
            ["ETHUSDT.BYBIT-2-HOUR-LAST-EXTERNAL", "120"],
            ["ETHUSDT.BYBIT-4-HOUR-LAST-EXTERNAL", "240"],
            ["ETHUSDT.BYBIT-6-HOUR-LAST-EXTERNAL", "360"],
            ["ETHUSDT.BYBIT-12-HOUR-LAST-EXTERNAL", "720"],
            ["ETHUSDT.BYBIT-1-DAY-LAST-EXTERNAL", "D"],
            ["ETHUSDT.BYBIT-1-WEEK-LAST-EXTERNAL", "W"],
            ["ETHUSDT.BYBIT-1-MONTH-LAST-EXTERNAL", "M"],
        ],
    )
    def test_parse_bybit_kline_correct(self, bar_type, bybit_kline_interval):
        bar_type = BarType.from_str(bar_type)
        result = self._enum_parser.parse_bybit_kline(bar_type)
        assert result.value == bybit_kline_interval

    def test_parse_bybit_kline_incorrect(self):
        # MINUTE
        with pytest.raises(ValueError):
            self._enum_parser.parse_bybit_kline(
                BarType.from_str("ETHUSDT.BYBIT-2-MINUTE-LAST-EXTERNAL"),
            )
        # HOUR
        with pytest.raises(ValueError):
            self._enum_parser.parse_bybit_kline(
                BarType.from_str("ETHUSDT.BYBIT-3-HOUR-LAST-EXTERNAL"),
            )
        # DAY
        with pytest.raises(ValueError):
            result = self._enum_parser.parse_bybit_kline(
                BarType.from_str("ETHUSDT.BYBIT-3-DAY-LAST-EXTERNAL"),
            )
            print(result)
        # WEEK
        with pytest.raises(ValueError):
            self._enum_parser.parse_bybit_kline(
                BarType.from_str("ETHUSDT.BYBIT-2-WEEK-LAST-EXTERNAL"),
            )
        # MONTH
        with pytest.raises(ValueError):
            self._enum_parser.parse_bybit_kline(
                BarType.from_str("ETHUSDT.BYBIT-4-MONTH-LAST-EXTERNAL"),
            )

    @pytest.mark.parametrize(
        ("bybit_order_side", "order_side"),
        [
            [BybitOrderSide.BUY, OrderSide.BUY],
            [BybitOrderSide.SELL, OrderSide.SELL],
        ],
    )
    def test_parse_bybit_order_side(self, bybit_order_side, order_side):
        result = self._enum_parser.parse_bybit_order_side(bybit_order_side)
        assert result == order_side

    @pytest.mark.parametrize(
        ("order_side", "bybit_order_side"),
        [
            [OrderSide.BUY, BybitOrderSide.BUY],
            [OrderSide.SELL, BybitOrderSide.SELL],
        ],
    )
    def test_parse_nautilus_order_side(self, order_side, bybit_order_side):
        result = self._enum_parser.parse_nautilus_order_side(order_side)
        assert result == bybit_order_side

    @pytest.mark.parametrize(
        ("bybit_order_status", "order_status"),
        [
            [BybitOrderStatus.CREATED, OrderStatus.SUBMITTED],
            [BybitOrderStatus.NEW, OrderStatus.ACCEPTED],
            [BybitOrderStatus.FILLED, OrderStatus.FILLED],
            [BybitOrderStatus.CANCELED, OrderStatus.CANCELED],
        ],
    )
    def test_parse_bybit_order_status(self, bybit_order_status, order_status):
        result = self._enum_parser.parse_bybit_order_status(bybit_order_status)
        assert result == order_status

    @pytest.mark.parametrize(
        ("order_status", "bybit_order_status"),
        [
            [OrderStatus.SUBMITTED, BybitOrderStatus.CREATED],
            [OrderStatus.ACCEPTED, BybitOrderStatus.NEW],
            [OrderStatus.FILLED, BybitOrderStatus.FILLED],
            [OrderStatus.CANCELED, BybitOrderStatus.CANCELED],
        ],
    )
    def test_parse_nautilus_order_status(self, order_status, bybit_order_status):
        result = self._enum_parser.parse_nautilus_order_status(order_status)
        assert result == bybit_order_status
