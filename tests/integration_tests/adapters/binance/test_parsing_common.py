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

from nautilus_trader.adapters.binance.common.parsing.data import parse_bar_ws
from nautilus_trader.adapters.binance.common.schemas import BinanceCandlestick
from nautilus_trader.adapters.binance.spot.enums import BinanceSpotOrderType
from nautilus_trader.adapters.binance.spot.parsing.execution import parse_order_type
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PriceType


BTCUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestBinanceCommonParsing:
    @pytest.mark.parametrize(
        "order_type, expected",
        [
            [BinanceSpotOrderType.MARKET, OrderType.MARKET],
            [BinanceSpotOrderType.LIMIT, OrderType.LIMIT],
            [BinanceSpotOrderType.STOP, OrderType.STOP_MARKET],
            [BinanceSpotOrderType.STOP_LOSS, OrderType.STOP_MARKET],
            [BinanceSpotOrderType.TAKE_PROFIT, OrderType.LIMIT],
            [BinanceSpotOrderType.TAKE_PROFIT_LIMIT, OrderType.STOP_LIMIT],
        ],
    )
    def test_parse_order_type(self, order_type, expected):
        # Arrange, # Act
        result = parse_order_type(order_type)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "resolution, expected_type",
        [
            [
                "1m",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "3m",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(3, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "5m",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(5, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "15m",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(15, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "30m",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(30, BarAggregation.MINUTE, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "1h",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "2h",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(2, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "4h",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(4, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "6h",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(6, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "8h",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(8, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "12h",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(12, BarAggregation.HOUR, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "1d",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.DAY, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "3d",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(3, BarAggregation.DAY, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "1w",
                BarType(
                    BTCUSDT_BINANCE.id,
                    BarSpecification(1, BarAggregation.WEEK, PriceType.LAST),
                    AggregationSource.EXTERNAL,
                ),
            ],
            [
                "1M",
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
        bar = parse_bar_ws(
            instrument_id=BTCUSDT_BINANCE.id,
            data=candle,
            ts_init=millis_to_nanos(1638747720000),
        )

        # Assert
        assert bar.bar_type == expected_type
