# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import pkgutil

import msgspec
import pytest

from nautilus_trader.adapters.polymarket.common.parsing import parse_instrument
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuote
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTrade
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserTrade
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


def test_parse_instruments() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "markets.json",
    )
    assert data
    response = msgspec.json.decode(data)

    # Act
    instruments: list[BinaryOption] = []
    for market_info in response["data"]:
        for token_info in market_info["tokens"]:
            token_id = token_info["token_id"]
            if not token_id:
                continue
            outcome = token_info["outcome"]
            instrument = parse_instrument(market_info, token_id, outcome, 0)
            instruments.append(instrument)

    # Assert
    assert len(instruments) == 198


def test_parse_order_book_snapshots() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.http_responses",
        "book.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketBookSnapshot)
    ws_message = decoder.decode(data)
    instrument = TestInstrumentProvider.binary_option()

    # Act
    snapshot = ws_message.parse_to_snapshot(instrument=instrument, ts_init=1728799418260000001)

    # Assert
    assert isinstance(snapshot, OrderBookDeltas)
    assert len(snapshot.deltas) == 13
    assert snapshot.is_snapshot
    assert snapshot.ts_event == 1728799418260000000
    assert snapshot.ts_init == 1728799418260000001


def test_parse_order_book_delta() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "price_change.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketQuote)
    ws_message = decoder.decode(data)
    instrument = TestInstrumentProvider.binary_option()

    # Act
    delta = ws_message.parse_to_delta(instrument=instrument, ts_init=2)

    # Assert
    assert isinstance(delta, OrderBookDelta)
    assert delta.action == BookAction.UPDATE
    assert delta.order.side == OrderSide.SELL
    assert delta.order.price == instrument.make_price(0.514)
    assert delta.order.size == instrument.make_qty(21_574.08)
    assert delta.flags == RecordFlag.F_LAST
    assert delta.ts_event == 1723967931411000064
    assert delta.ts_init == 2


def test_parse_quote_tick() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "price_change.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketQuote)
    ws_message = decoder.decode(data)
    instrument = TestInstrumentProvider.binary_option()

    last_quote = TestDataStubs.quote_tick(instrument=instrument, bid_price=0.513)

    # Act
    quote = ws_message.parse_to_quote_tick(
        instrument=instrument,
        last_quote=last_quote,
        ts_init=2,
    )

    # Assert
    assert isinstance(quote, QuoteTick)
    assert quote.bid_price == instrument.make_price(0.513)
    assert quote.ask_price == instrument.make_price(0.514)
    assert quote.bid_size == instrument.make_qty(100_000.0)
    assert quote.ask_size == instrument.make_qty(21_574.08)
    assert quote.ts_event == 1723967931411000064
    assert quote.ts_init == 2


def test_parse_trade_tick() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "last_trade_price.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketTrade)
    ws_message = decoder.decode(data)
    instrument = TestInstrumentProvider.binary_option()

    # Act
    trade = ws_message.parse_to_trade_tick(instrument=instrument, ts_init=2)

    # Assert
    assert isinstance(trade, TradeTick)
    assert trade.price == instrument.make_price(0.491)
    assert trade.size == instrument.make_qty(85.36)
    assert trade.aggressor_side == AggressorSide.SELLER
    assert trade.ts_event == 1724564136087000064
    assert trade.ts_init == 2


def test_parse_order_placement() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "order_placement.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketUserOrder)

    # Act
    msg = decoder.decode(data)

    # Assert
    assert isinstance(msg, PolymarketUserOrder)


def test_parse_order_cancel() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "order_cancel.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketUserOrder)

    # Act
    msg = decoder.decode(data)

    # Assert
    assert isinstance(msg, PolymarketUserOrder)


@pytest.mark.parametrize(
    "data_file",
    [
        "user_trade1.json",
        "user_trade2.json",
    ],
)
def test_parse_user_trade(data_file: str) -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        data_file,
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketUserTrade)

    # Act
    msg = decoder.decode(data)

    # Assert
    assert isinstance(msg, PolymarketUserTrade)
