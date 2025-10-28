# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerDirection
from nautilus_trader.adapters.bybit.schemas.account.balance import BybitCoinBalance
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.test_kit.providers import TestInstrumentProvider


ETHUSDT_BYBIT = TestInstrumentProvider.ethusdt_binance()


class TestBybitParsing:
    def setup(self) -> None:
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
    def test_parse_bybit_kline_correct(self, bar_type: str, bybit_kline_interval: str) -> None:
        bar_type = BarType.from_str(bar_type)
        result = self._enum_parser.parse_bybit_kline(bar_type)
        assert result.value == bybit_kline_interval

    def test_parse_bybit_kline_incorrect(self) -> None:
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
    def test_parse_bybit_order_side(
        self,
        bybit_order_side: BybitOrderSide,
        order_side: OrderSide,
    ) -> None:
        result = self._enum_parser.parse_bybit_order_side(bybit_order_side)
        assert result == order_side

    @pytest.mark.parametrize(
        ("order_side", "bybit_order_side"),
        [
            [OrderSide.BUY, BybitOrderSide.BUY],
            [OrderSide.SELL, BybitOrderSide.SELL],
        ],
    )
    def test_parse_nautilus_order_side(
        self,
        order_side: OrderSide,
        bybit_order_side: BybitOrderSide,
    ) -> None:
        result = self._enum_parser.parse_nautilus_order_side(order_side)
        assert result == bybit_order_side

    # WIP: Reimplementing
    # @pytest.mark.parametrize(
    #     ("bybit_order_status", "order_status"),
    #     [
    #         [BybitOrderStatus.CREATED, OrderStatus.SUBMITTED],
    #         [BybitOrderStatus.NEW, OrderStatus.ACCEPTED],
    #         [BybitOrderStatus.FILLED, OrderStatus.FILLED],
    #         [BybitOrderStatus.CANCELED, OrderStatus.CANCELED],
    #     ],
    # )
    # def test_parse_bybit_order_status(
    #     self,
    #     bybit_order_status: BybitOrderStatus,
    #     order_status: OrderStatus,
    # ) -> None:
    #     result = self._enum_parser.parse_bybit_order_status(bybit_order_status)
    #     assert result == order_status
    #
    # @pytest.mark.parametrize(
    #     ("order_status", "bybit_order_status"),
    #     [
    #         [OrderStatus.SUBMITTED, BybitOrderStatus.CREATED],
    #         [OrderStatus.ACCEPTED, BybitOrderStatus.NEW],
    #         [OrderStatus.FILLED, BybitOrderStatus.FILLED],
    #         [OrderStatus.CANCELED, BybitOrderStatus.CANCELED],
    #     ],
    # )
    # def test_parse_nautilus_order_status(
    #     self,
    #     order_status: OrderStatus,
    #     bybit_order_status: BybitOrderStatus,
    # ) -> None:
    #     result = self._enum_parser.parse_nautilus_order_status(order_status)
    #     assert result == bybit_order_status

    @pytest.mark.parametrize(
        ("order_type", "expected_direction_buy"),
        [
            (OrderType.STOP_MARKET, BybitTriggerDirection.RISES_TO),
            (OrderType.STOP_LIMIT, BybitTriggerDirection.RISES_TO),
            (OrderType.MARKET_IF_TOUCHED, BybitTriggerDirection.RISES_TO),
            (OrderType.TRAILING_STOP_MARKET, BybitTriggerDirection.RISES_TO),
            (OrderType.LIMIT_IF_TOUCHED, BybitTriggerDirection.FALLS_TO),
            (OrderType.MARKET, None),
            (OrderType.LIMIT, None),
        ],
    )
    def test_parse_trigger_direction_buy_orders(self, order_type, expected_direction_buy):
        # Arrange & Act
        result = self._enum_parser.parse_trigger_direction(
            order_type=order_type,
            order_side=OrderSide.BUY,
        )

        # Assert
        assert result == expected_direction_buy

    @pytest.mark.parametrize(
        ("order_type", "expected_direction_sell"),
        [
            (OrderType.STOP_MARKET, BybitTriggerDirection.FALLS_TO),
            (OrderType.STOP_LIMIT, BybitTriggerDirection.FALLS_TO),
            (OrderType.MARKET_IF_TOUCHED, BybitTriggerDirection.FALLS_TO),
            (OrderType.TRAILING_STOP_MARKET, BybitTriggerDirection.FALLS_TO),
            (OrderType.LIMIT_IF_TOUCHED, BybitTriggerDirection.RISES_TO),
            (OrderType.MARKET, None),
            (OrderType.LIMIT, None),
        ],
    )
    def test_parse_trigger_direction_sell_orders(self, order_type, expected_direction_sell):
        # Arrange & Act
        result = self._enum_parser.parse_trigger_direction(
            order_type=order_type,
            order_side=OrderSide.SELL,
        )

        # Assert
        assert result == expected_direction_sell

    def test_parse_trigger_direction_unsupported_order_types(self):
        # Arrange & Act
        result_buy = self._enum_parser.parse_trigger_direction(
            order_type=OrderType.MARKET,
            order_side=OrderSide.BUY,
        )
        result_sell = self._enum_parser.parse_trigger_direction(
            order_type=OrderType.MARKET,
            order_side=OrderSide.SELL,
        )

        # Assert
        assert result_buy is None
        assert result_sell is None


def test_parse_wallet_balance_with_spot_borrow():
    """
    Test that spotBorrow is correctly subtracted from walletBalance.
    """
    coin_data = {
        "availableToBorrow": "5000",
        "bonus": "0",
        "accruedInterest": "0.50",
        "availableToWithdraw": "800.00",
        "totalOrderIM": "0",
        "equity": "1000.00",
        "usdValue": "1000.00",
        "borrowAmount": "200.00",
        "totalPositionMM": "0",
        "totalPositionIM": "0",
        "walletBalance": "1200.00",
        "unrealisedPnl": "0",
        "cumRealisedPnl": "100.00",
        "locked": "0",
        "collateralSwitch": True,
        "marginCollateral": True,
        "coin": "USDT",
        "spotBorrow": "200.00",
    }

    coin_balance = BybitCoinBalance(**coin_data)
    account_balance = coin_balance.parse_to_account_balance()

    # Verify: actual_balance = walletBalance - spotBorrow = 1200 - 200 = 1000
    assert account_balance.total.as_decimal() == Decimal("1000.00")
    assert account_balance.locked.as_decimal() == Decimal("0")
    assert account_balance.free.as_decimal() == Decimal("1000.00")

    # Verify invariant: total = locked + free
    assert account_balance.total.raw == account_balance.locked.raw + account_balance.free.raw
