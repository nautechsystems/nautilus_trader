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

from nautilus_trader.core.nautilus_pyo3 import AggressorSide
from nautilus_trader.core.nautilus_pyo3 import Bar
from nautilus_trader.core.nautilus_pyo3 import BarAggregation
from nautilus_trader.core.nautilus_pyo3 import BarSpecification
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.core.nautilus_pyo3 import BookAction
from nautilus_trader.core.nautilus_pyo3 import BookOrder
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import OrderBookDelta
from nautilus_trader.core.nautilus_pyo3 import OrderBookDepth10
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import QuoteTick
from nautilus_trader.core.nautilus_pyo3 import TradeTick
from nautilus_trader.test_kit.rust.identifiers_pyo3 import TestIdProviderPyo3


class TestDataProviderPyo3:
    @staticmethod
    def order_book_delta(
        instrument_id: InstrumentId | None = None,
        price: float = 10000.0,
        size: float = 0.1,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> OrderBookDelta:
        return OrderBookDelta(
            instrument_id=instrument_id or TestIdProviderPyo3.ethusdt_binance_id(),
            action=BookAction.ADD,
            order=BookOrder(
                side=OrderSide.BUY,
                price=Price.from_str(str(price)),
                size=Quantity.from_str(str(size)),
                order_id=0,
            ),
            flags=0,
            sequence=0,
            ts_init=ts_init,
            ts_event=ts_event,
        )

    @staticmethod
    def order_book_depth10(
        instrument_id: InstrumentId | None = None,
        flags: int = 0,
        sequence: int = 0,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> OrderBookDepth10:
        bids: list[BookOrder] = []
        asks: list[BookOrder] = []

        # Create bids
        price = 99.00
        quantity = 100.0
        order_id = 1

        for _ in range(10):
            order = BookOrder(
                OrderSide.BUY,
                Price(price, 2),
                Quantity(quantity, 0),
                order_id,
            )

            bids.append(order)

            price -= 1.0
            quantity += 100.0
            order_id += 1

        # Create asks
        price = 100.00
        quantity = 100.0
        order_id = 11

        for _ in range(10):
            order = BookOrder(
                OrderSide.SELL,
                Price(price, 2),
                Quantity(quantity, 0),
                order_id,
            )

            asks.append(order)

            price += 1.0
            quantity += 100.0
            order_id += 1

        bid_counts = [1] * 10
        ask_counts = [1] * 10

        return OrderBookDepth10(
            instrument_id=instrument_id or TestIdProviderPyo3.aapl_xnas_id(),
            bids=bids,
            asks=asks,
            bid_counts=bid_counts,
            ask_counts=ask_counts,
            flags=flags,
            sequence=sequence,
            ts_event=ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    def quote_tick(
        instrument_id: InstrumentId | None = None,
        bid_price: float = 1987.0,
        ask_price: float = 1988.0,
        ask_size: float = 100_000.0,
        bid_size: float = 100_000.0,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> QuoteTick:
        return QuoteTick(
            instrument_id=instrument_id or TestIdProviderPyo3.ethusdt_binance_id(),
            bid_price=Price.from_str(str(bid_price)),
            ask_price=Price.from_str(str(ask_price)),
            bid_size=Quantity.from_str(str(bid_size)),
            ask_size=Quantity.from_str(str(ask_size)),
            ts_event=ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    def trade_tick(
        instrument_id: InstrumentId | None = None,
        price: float = 1987.0,
        size: float = 0.1,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> TradeTick:
        return TradeTick(
            instrument_id=instrument_id or TestIdProviderPyo3.ethusdt_binance_id(),
            price=Price.from_str(str(price)),
            size=Quantity.from_str(str(size)),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TestIdProviderPyo3.trade_id(),
            ts_init=ts_init,
            ts_event=ts_event,
        )

    @staticmethod
    def bar_spec_1min_bid() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)

    @staticmethod
    def bar_spec_1min_ask() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK)

    @staticmethod
    def bar_spec_1min_last() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)

    @staticmethod
    def bar_spec_1min_mid() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)

    @staticmethod
    def bartype_ethusdt_1min_bid() -> BarType:
        return BarType(
            TestIdProviderPyo3.ethusdt_binance_id(),
            TestDataProviderPyo3.bar_spec_1min_bid(),
        )

    @staticmethod
    def bar_5decimal() -> Bar:
        return Bar(
            bar_type=TestDataProviderPyo3.bartype_ethusdt_1min_bid(),
            open=Price.from_str("1.00002"),
            high=Price.from_str("1.00004"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00003"),
            volume=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
