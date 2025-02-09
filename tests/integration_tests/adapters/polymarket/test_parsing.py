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

import pkgutil

import msgspec
import pytest

from nautilus_trader.adapters.polymarket.common.parsing import parse_instrument
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuotes
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTickSizeChange
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTrade
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserTrade
from nautilus_trader.model.data import OrderBookDeltas
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


def test_parse_order_book_deltas() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "price_change_v2.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketQuotes)
    ws_message = decoder.decode(data)
    instrument = TestInstrumentProvider.binary_option()

    # Act
    deltas = ws_message.parse_to_deltas(instrument=instrument, ts_init=2)

    # Assert
    assert isinstance(deltas, OrderBookDeltas)
    assert deltas.deltas[0].action == BookAction.UPDATE
    assert deltas.deltas[0].order.side == OrderSide.BUY
    assert deltas.deltas[0].order.price == instrument.make_price(0.600)
    assert deltas.deltas[0].order.size == instrument.make_qty(3_300.00)
    assert deltas.deltas[0].flags == RecordFlag.F_LAST
    assert deltas.deltas[0].ts_event == 1729084877448000000
    assert deltas.deltas[0].ts_init == 2


def test_parse_quote_ticks() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "price_change_v2.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketQuotes)
    ws_message = decoder.decode(data)
    instrument = TestInstrumentProvider.binary_option()

    last_quote = TestDataStubs.quote_tick(instrument=instrument, bid_price=0.513)

    # Act
    quotes = ws_message.parse_to_quote_ticks(
        instrument=instrument,
        last_quote=last_quote,
        ts_init=2,
    )

    # Assert
    assert isinstance(quotes, list)
    assert quotes[0].bid_price == instrument.make_price(0.600)
    assert quotes[0].ask_price == instrument.make_price(1.000)
    assert quotes[0].bid_size == instrument.make_qty(3_300.0)
    assert quotes[0].ask_size == instrument.make_qty(100_000.00)
    assert quotes[0].ts_event == 1729084877448000000
    assert quotes[0].ts_init == 2


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


def test_parse_user_trade_to_dict() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "user_trade1.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketUserTrade)
    msg = decoder.decode(data)

    # Act
    values = msg.to_dict()

    # Assert
    assert values == {
        "event_type": "trade",
        "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        "bucket_index": "0",
        "fee_rate_bps": "0",
        "id": "83b5c849-620e-4c23-b63b-2e779c04a6e7",
        "last_update": "1725868885",
        "maker_address": "0x64B7a036c378f9CF8163A5480437CD12Ae14b6A1",
        "maker_orders": [
            {
                "asset_id": "48331043336612883890938759509493159234755048973500640148014422747788308965732",
                "fee_rate_bps": "0",
                "maker_address": "0xFfd192468b7a05b38c37C82d09fA941289FaEB23",
                "matched_amount": "10",
                "order_id": "0x3b67d584e1e7ad29b06bda373449638898aa87f0c9fd52a34bdbfb1325a6c184",
                "outcome": "No",
                "owner": "78132bc3-22af-6aa2-79ae-11929f821cae",
                "price": "0.482",
            },
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "0",
                "maker_address": "0xA15DfDEE79Fe241A99D4f42a8D41510b670fFb40",
                "matched_amount": "247.68",
                "order_id": "0x67620d882faa37cd1a6668de1271c4b1b6f58fb4ebabc2c095692dfd9c15735b",
                "outcome": "Yes",
                "owner": "86f776cc-e18b-c94e-80b4-a7364e0ecec5",
                "price": "0.518",
            },
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "0",
                "maker_address": "0x4a5Ef3c64056362ce10fCd2C1B9BBd9BEC4CB3EF",
                "matched_amount": "227.92",
                "order_id": "0x8d2f8f0d2bd92bc734c3f324d6e88b2fa0e96a91efb124aa6d73bfb4639e7287",
                "outcome": "Yes",
                "owner": "58c3ba99-0006-1c64-a59b-290c59abd1ce",
                "price": "0.518",
            },
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "0",
                "maker_address": "0xa3D82Ed56F4c68d2328Fb8c29e568Ba2cAF7d7c8",
                "matched_amount": "5",
                "order_id": "0xab679e56242324e15e59cfd488cd0f12e4fd71b153b9bfb57518898b9983145e",
                "outcome": "Yes",
                "owner": "3e2c94ca-8124-c4c1-c7ea-be1ea21b71fe",
                "price": "0.518",
            },
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "0",
                "maker_address": "0xac2ce2bA3Bde4959921D35d6480eF77Fb7CE53d9",
                "matched_amount": "394.46",
                "order_id": "0xb222c67c2d1e6c01eace5ca2b830cf3a0e6f5ef079270781e5ebd42a86722578",
                "outcome": "Yes",
                "owner": "2411624a-9df5-6457-cba9-abf680875588",
                "price": "0.518",
            },
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "0",
                "maker_address": "0x5c744e01Ac62d025E56F1398605dE581dE607C65",
                "matched_amount": "211.81",
                "order_id": "0xed3e5b80ca742bbd5048cdd42cf6fe8782a0e202658e070b4c8ebc4911059652",
                "outcome": "Yes",
                "owner": "99d32b22-5e10-8caa-a981-d21ad20989e2",
                "price": "0.518",
            },
        ],
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "match_time": "1725868859",
        "outcome": "Yes",
        "owner": "092dab0c-74fa-5ba7-4b67-572daeace198",
        "price": "0.518",
        "side": "BUY",
        "size": "1096.87",
        "status": "MINED",
        "taker_order_id": "0x3ad09f225ebe141dfbdb3824f31cb457e8e0301ca4e0a06311e543f5328b9dea",
        "timestamp": "1725868885871",
        "trade_owner": "092dab0c-74fa-5ba7-4b67-572daeace198",
        "trader_side": "MAKER",
        "type": "TRADE",
    }


def test_parse_tick_size_change() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "tick_size_change.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketTickSizeChange)

    # Act
    msg = decoder.decode(data)

    # Assert
    assert isinstance(msg, PolymarketTickSizeChange)
    assert msg.new_tick_size == "0.001"
