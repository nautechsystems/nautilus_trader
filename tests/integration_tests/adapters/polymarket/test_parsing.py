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

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MIN_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.parsing import parse_instrument
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookLevel
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuotes
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTickSizeChange
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTrade
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserTrade
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.providers import TestInstrumentProvider


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

    # Assert - Test new schema structure
    assert ws_message.market == "0x5f65177b394277fd294cd75650044e32ba009a95022d88a0c1d565897d72f8f1"
    assert len(ws_message.price_changes) == 3
    assert ws_message.timestamp == "1729084877448"

    # Test first price change
    first_change = ws_message.price_changes[0]
    assert (
        first_change.asset_id
        == "52114319501245915516055106046884209969926127482827954674443846427813813222426"
    )
    assert first_change.price == "0.6"
    assert first_change.side.value == "BUY"
    assert first_change.size == "3300"
    assert first_change.hash == "bf32b3746fff40c76c98021b7f3f07261169dd26"
    assert first_change.best_bid == "0.6"
    assert first_change.best_ask == "0.7"


def test_parse_quote_ticks() -> None:
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "price_change_v2.json",
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketQuotes)
    ws_message = decoder.decode(data)

    # Assert - Test that we can access the new schema fields
    assert len(ws_message.price_changes) == 3

    for i, price_change in enumerate(ws_message.price_changes):
        assert (
            price_change.asset_id
            == "52114319501245915516055106046884209969926127482827954674443846427813813222426"
        )
        assert price_change.side.value == "BUY"
        assert price_change.best_bid == "0.6"
        assert price_change.best_ask == "0.7"
        assert price_change.hash is not None


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
        "bucket_index": 0,
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


def test_parse_book_snapshot_to_quote_empty_bids() -> None:
    """
    Test parsing book snapshot to quote when bids are empty (one-sided market).
    """
    # Arrange
    book_snapshot = PolymarketBookSnapshot(
        market="0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        asset_id="23360939988679364027624185518382759743328544433592111535569478055890815567848",
        bids=[],  # Empty bids
        asks=[
            PolymarketBookLevel(price="0.50", size="100.0"),
            PolymarketBookLevel(price="0.60", size="200.0"),
        ],
        timestamp="1728799418260",
    )
    instrument = TestInstrumentProvider.binary_option()

    # Act - Test with default drop_quotes_missing_side=True
    quote = book_snapshot.parse_to_quote(instrument=instrument, ts_init=1728799418260000001)
    assert quote is None  # Should return None when bids are missing

    # Act - Test with drop_quotes_missing_side=False
    quote = book_snapshot.parse_to_quote(
        instrument=instrument,
        ts_init=1728799418260000001,
        drop_quotes_missing_side=False,
    )

    # Assert
    assert quote is not None
    assert quote.bid_price == instrument.make_price(0.001)  # POLYMARKET_MIN_PRICE
    assert quote.bid_size == instrument.make_qty(0.0)
    assert quote.ask_price == instrument.make_price(0.60)
    assert quote.ask_size == instrument.make_qty(200.0)


def test_parse_book_snapshot_to_quote_empty_asks() -> None:
    """
    Test parsing book snapshot to quote when asks are empty (one-sided market).
    """
    # Arrange
    book_snapshot = PolymarketBookSnapshot(
        market="0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        asset_id="23360939988679364027624185518382759743328544433592111535569478055890815567848",
        bids=[
            PolymarketBookLevel(price="0.40", size="150.0"),
            PolymarketBookLevel(price="0.50", size="250.0"),
        ],
        asks=[],  # Empty asks
        timestamp="1728799418260",
    )
    instrument = TestInstrumentProvider.binary_option()

    # Act - Test with default drop_quotes_missing_side=True
    quote = book_snapshot.parse_to_quote(instrument=instrument, ts_init=1728799418260000001)
    assert quote is None  # Should return None when asks are missing

    # Act - Test with drop_quotes_missing_side=False
    quote = book_snapshot.parse_to_quote(
        instrument=instrument,
        ts_init=1728799418260000001,
        drop_quotes_missing_side=False,
    )

    # Assert
    assert quote is not None
    assert quote.bid_price == instrument.make_price(0.50)
    assert quote.bid_size == instrument.make_qty(250.0)
    assert quote.ask_price == instrument.make_price(POLYMARKET_MAX_PRICE)
    assert quote.ask_size == instrument.make_qty(0.0)


def test_parse_book_snapshot_to_quote_both_empty() -> None:
    """
    Test parsing book snapshot to quote when both bids and asks are empty.
    """
    # Arrange
    book_snapshot = PolymarketBookSnapshot(
        market="0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        asset_id="23360939988679364027624185518382759743328544433592111535569478055890815567848",
        bids=[],  # Empty bids
        asks=[],  # Empty asks
        timestamp="1728799418260",
    )
    instrument = TestInstrumentProvider.binary_option()

    # Act - Test with default drop_quotes_missing_side=True
    quote = book_snapshot.parse_to_quote(instrument=instrument, ts_init=1728799418260000001)
    assert quote is None  # Should return None when both sides are missing

    # Act - Test with drop_quotes_missing_side=False
    quote = book_snapshot.parse_to_quote(
        instrument=instrument,
        ts_init=1728799418260000001,
        drop_quotes_missing_side=False,
    )

    # Assert
    assert quote is not None
    assert quote.bid_price == instrument.make_price(POLYMARKET_MIN_PRICE)
    assert quote.bid_size == instrument.make_qty(0.0)
    assert quote.ask_price == instrument.make_price(POLYMARKET_MAX_PRICE)
    assert quote.ask_size == instrument.make_qty(0.0)


def test_parse_empty_book_snapshot_returns_none():
    """
    Test that parsing a book snapshot with both empty bids and asks returns None.

    This can occur near market resolution and should not crash.

    """
    # Arrange
    raw_data = {
        "asks": [],
        "asset_id": "46428986054832220603415781377952331535489217742718963672459046269597594860904",
        "bids": [],
        "event_type": "book",
        "hash": "71f5c52df95bea6fa56312686790af61d4a42fcc",
        "market": "0x22025ebf02ae8bf9aae999649b145ebe9b5db6e23a36acc7abe9ef5ca184ab57",
        "received_at": 1756944696893,
        "timestamp": "1756944561003",
    }

    instrument = BinaryOption.from_dict(
        {
            "activation_ns": 0,
            "asset_class": "ALTERNATIVE",
            "currency": "USDC.e",
            "description": "Bitcoin Up or Down - September 3, 7PM ET",
            "expiration_ns": 1756944000000000000,
            "id": "0x22025ebf02ae8bf9aae999649b145ebe9b5db6e23a36acc7abe9ef5ca184ab57-46428986054832220603415781377952331535489217742718963672459046269597594860904.POLYMARKET",
            "maker_fee": "0",
            "margin_init": "0",
            "margin_maint": "0",
            "max_quantity": None,
            "min_quantity": "1",
            "outcome": "Up",
            "price_increment": "0.01",
            "price_precision": 2,
            "raw_symbol": "46428986054832220603415781377952331535489217742718963672459046269597594860904",
            "size_increment": "0.000001",
            "size_precision": 6,
            "taker_fee": "0",
            "tick_scheme_name": None,
            "ts_event": 0,
            "ts_init": 0,
            "type": "BinaryOption",
        },
    )

    # Act
    snapshot = msgspec.json.decode(msgspec.json.encode(raw_data), type=PolymarketBookSnapshot)
    result = snapshot.parse_to_snapshot(instrument=instrument, ts_init=0)

    # Assert
    assert result is None


def test_parse_empty_book_snapshot_in_backtest_engine():
    """
    Integration test: empty book snapshots should not crash the backtest engine.
    This is a regression test for a double-free crash that occurred when processing
    OrderBookDeltas with only a CLEAR delta and no F_LAST flag.
    """
    # Arrange
    raw_data = [
        {
            "asks": [],
            "asset_id": "46428986054832220603415781377952331535489217742718963672459046269597594860904",
            "bids": [],
            "event_type": "book",
            "hash": "71f5c52df95bea6fa56312686790af61d4a42fcc",
            "market": "0x22025ebf02ae8bf9aae999649b145ebe9b5db6e23a36acc7abe9ef5ca184ab57",
            "received_at": 1756944696893,
            "timestamp": "1756944561003",
        },
    ]

    instrument = BinaryOption.from_dict(
        {
            "activation_ns": 0,
            "asset_class": "ALTERNATIVE",
            "currency": "USDC.e",
            "description": "Bitcoin Up or Down - September 3, 7PM ET",
            "expiration_ns": 1756944000000000000,
            "id": "0x22025ebf02ae8bf9aae999649b145ebe9b5db6e23a36acc7abe9ef5ca184ab57-46428986054832220603415781377952331535489217742718963672459046269597594860904.POLYMARKET",
            "maker_fee": "0",
            "margin_init": "0",
            "margin_maint": "0",
            "max_quantity": None,
            "min_quantity": "1",
            "outcome": "Up",
            "price_increment": "0.01",
            "price_precision": 2,
            "raw_symbol": "46428986054832220603415781377952331535489217742718963672459046269597594860904",
            "size_increment": "0.000001",
            "size_precision": 6,
            "taker_fee": "0",
            "tick_scheme_name": None,
            "ts_event": 0,
            "ts_init": 0,
            "type": "BinaryOption",
        },
    )

    config = BacktestEngineConfig(
        trader_id="BACKTESTER-001",
        logging=LoggingConfig(log_level="ERROR"),  # Suppress output
    )
    engine = BacktestEngine(config=config)
    engine.add_venue(
        venue=POLYMARKET_VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.CASH,
        base_currency=USDC,
        starting_balances=[Money(100, USDC)],
        book_type=BookType.L2_MBP,
    )
    engine.add_instrument(instrument)

    # Act - parse and add data (should skip empty snapshots)
    deltas = []
    for msg in raw_data:
        snapshot = msgspec.json.decode(msgspec.json.encode(msg), type=PolymarketBookSnapshot)
        ob_snapshot = snapshot.parse_to_snapshot(instrument=instrument, ts_init=0)
        if ob_snapshot is not None:
            deltas.append(ob_snapshot)

    if deltas:
        engine.add_data(deltas)

    # Assert - should complete without crashing
    engine.run()
