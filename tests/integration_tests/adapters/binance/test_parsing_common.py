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

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceFuturesPositionSide
from nautilus_trader.adapters.binance.common.enums import BinanceKlineInterval
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.schemas.market import BinanceCandlestick
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesEnumParser
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesBalanceInfo
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotEnumParser
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.test_kit.providers import TestInstrumentProvider


BTCUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestBinanceCommonParsing:
    def setup(self):
        self._spot_enum_parser = BinanceSpotEnumParser()
        self._futures_enum_parser = BinanceFuturesEnumParser()

    @pytest.mark.parametrize(
        ("order_type", "expected"),
        [
            [BinanceOrderType.LIMIT, OrderType.LIMIT],
            [BinanceOrderType.MARKET, OrderType.MARKET],
            [BinanceOrderType.STOP, OrderType.STOP_MARKET],
            [BinanceOrderType.STOP_LOSS, OrderType.STOP_MARKET],
            [BinanceOrderType.STOP_LOSS_LIMIT, OrderType.STOP_LIMIT],
            [BinanceOrderType.TAKE_PROFIT, OrderType.LIMIT],
            [BinanceOrderType.TAKE_PROFIT_LIMIT, OrderType.STOP_LIMIT],
            [BinanceOrderType.LIMIT_MAKER, OrderType.LIMIT],
        ],
    )
    def test_parse_order_type(self, order_type, expected):
        # Arrange, # Act
        result = self._spot_enum_parser.parse_binance_order_type(order_type)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        ("resolution", "expected_type"),
        [
            [
                BinanceKlineInterval("1s"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.SECOND, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("1m"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("3m"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(3, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("5m"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(5, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("15m"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(15, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("30m"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(30, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("1h"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("2h"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(2, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("4h"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(4, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("6h"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(6, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("8h"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(8, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("12h"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(12, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("1d"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.DAY, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("3d"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(3, BarAggregation.DAY, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("1w"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.WEEK, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                BinanceKlineInterval("1M"),
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.MONTH, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
        ],
    )
    def test_parse_parse_bar_ws(self, resolution, expected_type):
        # Arrange
        candle = BinanceCandlestick(
            t=1638747660000,
            T=1638747719999,
            s="BTCUSDT",
            i=resolution,
            f=100,
            L=200,
            o="0.0015",
            c="0.0020",
            h="0.0025",
            l="0.0015",
            v="1000",
            n=100,
            x=False,
            q="1.0000",
            V="500",
            Q="0.500",
            B="123456",
        )

        # Act
        bar = candle.parse_to_binance_bar(
            instrument_id=BTCUSDT_BINANCE.id,
            enum_parser=self._spot_enum_parser,
            ts_init=millis_to_nanos(1638747720000),
        )

        # Assert
        assert bar.bar_type == expected_type

    @pytest.mark.parametrize(
        ("position_id", "expected"),
        [
            [PositionId("P-20240817-BTCUSDT-LONG"), BinanceFuturesPositionSide.LONG],
            [PositionId("P-20240817-BTCUSDT-SHORT"), BinanceFuturesPositionSide.SHORT],
            [PositionId("P-20240817-BTCUSDT-BOTH"), BinanceFuturesPositionSide.BOTH],
        ],
    )
    def test_parse_position_id_to_binance_futures_position_side(self, position_id, expected):
        # Arrange, Act
        result = self._futures_enum_parser.parse_position_id_to_binance_futures_position_side(
            position_id,
        )

        # Assert
        assert result == expected


def test_binance_futures_parse_to_balances() -> None:
    # Arrange
    balance_infos = [
        BinanceFuturesBalanceInfo(
            asset="FDUSD",
            walletBalance="0.00000000",
            unrealizedProfit="0.00000000",
            marginBalance="0.00000000",
            maintMargin="0.00000000",
            initialMargin="0.00000000",
            positionInitialMargin="0.00000000",
            openOrderInitialMargin="0.00000000",
            crossWalletBalance="0.00000000",
            crossUnPnl="0.00000000",
            availableBalance="145.00731942",
            maxWithdrawAmount="0.00000000",
            marginAvailable=True,
            updateTime=0,
        ),
        BinanceFuturesBalanceInfo(
            asset="BNB",
            walletBalance="0.00000000",
            unrealizedProfit="0.00000000",
            marginBalance="0.00000000",
            maintMargin="0.00000000",
            initialMargin="0.00000000",
            positionInitialMargin="0.00000000",
            openOrderInitialMargin="0.00000000",
            crossWalletBalance="0.00000000",
            crossUnPnl="0.00000000",
            availableBalance="0.26632926",
            maxWithdrawAmount="0.00000000",
            marginAvailable=True,
            updateTime=0,
        ),
        BinanceFuturesBalanceInfo(
            asset="USDT",
            walletBalance="0.00000000",
            unrealizedProfit="0.00000000",
            marginBalance="0.00000000",
            maintMargin="0.00000000",
            initialMargin="2.19077500",
            positionInitialMargin="0.00000000",
            openOrderInitialMargin="2.19077500",
            crossWalletBalance="0.00000000",
            crossUnPnl="0.00000000",
            availableBalance="144.65930217",
            maxWithdrawAmount="0.00000000",
            marginAvailable=True,
            updateTime=1709962270029,
        ),
        BinanceFuturesBalanceInfo(
            asset="USDC",
            walletBalance="0.00000000",
            unrealizedProfit="0.00000000",
            marginBalance="0.00000000",
            maintMargin="0.00000000",
            initialMargin="0.00000000",
            positionInitialMargin="0.00000000",
            openOrderInitialMargin="0.00000000",
            crossWalletBalance="0.00000000",
            crossUnPnl="0.00000000",
            availableBalance="141.87086959",
            maxWithdrawAmount="0.00000000",
            marginAvailable=True,
            updateTime=0,
        ),
        BinanceFuturesBalanceInfo(
            asset="BUSD",
            walletBalance="0.00000000",
            unrealizedProfit="0.00000000",
            marginBalance="0.00000000",
            maintMargin="0.00000000",
            initialMargin="0.00000000",
            positionInitialMargin="0.00000000",
            openOrderInitialMargin="0.00000000",
            crossWalletBalance="0.00000000",
            crossUnPnl="0.00000000",
            availableBalance="142.12699008",
            maxWithdrawAmount="0.00000000",
            marginAvailable=True,
            updateTime=0,
        ),
        BinanceFuturesBalanceInfo(
            asset="ETH",
            walletBalance="0.03856162",
            unrealizedProfit="0.00000000",
            marginBalance="0.03856162",
            maintMargin="0.00000000",
            initialMargin="0.00000000",
            positionInitialMargin="0.00000000",
            openOrderInitialMargin="0.00000000",
            crossWalletBalance="0.03856162",
            crossUnPnl="0.00000000",
            availableBalance="0.03436859",
            maxWithdrawAmount="0.03436859",
            marginAvailable=True,
            updateTime=1709962270029,
        ),
        BinanceFuturesBalanceInfo(
            asset="BTC",
            walletBalance="0.00000000",
            unrealizedProfit="0.00000000",
            marginBalance="0.00000000",
            maintMargin="0.00000000",
            initialMargin="0.00000000",
            positionInitialMargin="0.00000000",
            openOrderInitialMargin="0.00000000",
            crossWalletBalance="0.00000000",
            crossUnPnl="0.00000000",
            availableBalance="0.00192305",
            maxWithdrawAmount="0.00000000",
            marginAvailable=True,
            updateTime=0,
        ),
    ]

    # Act, Assert (`AccountBalance` asserts invariants)
    for info in balance_infos:
        info.parse_to_account_balance()
