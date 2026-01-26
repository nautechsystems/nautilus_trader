# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from decimal import Decimal

import msgspec
import pytest

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MIN_PRICE
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.enums import PolymarketEventType
from nautilus_trader.adapters.polymarket.common.enums import PolymarketLiquiditySide
from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderSide
from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderStatus
from nautilus_trader.adapters.polymarket.common.enums import PolymarketOrderType
from nautilus_trader.adapters.polymarket.common.enums import PolymarketTradeStatus
from nautilus_trader.adapters.polymarket.common.parsing import calculate_commission
from nautilus_trader.adapters.polymarket.common.parsing import determine_order_side
from nautilus_trader.adapters.polymarket.common.parsing import parse_polymarket_instrument
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookLevel
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuotes
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTickSizeChange
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketTrade
from nautilus_trader.adapters.polymarket.schemas.order import PolymarketMakerOrder
from nautilus_trader.adapters.polymarket.schemas.trade import PolymarketTradeReport
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketOpenOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserOrder
from nautilus_trader.adapters.polymarket.schemas.user import PolymarketUserTrade
from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.currencies import USDC_POS
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import AccountId
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
            instrument = parse_polymarket_instrument(market_info, token_id, outcome, 0)
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

    for _, price_change in enumerate(ws_message.price_changes):
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


def test_parse_user_trade_to_fill_report_ts_event() -> None:
    """
    Test that match_time (in seconds) is correctly converted to ts_event (in
    nanoseconds).

    Regression test for
    https://github.com/nautechsystems/nautilus_trader/issues/3273

    """
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.polymarket.resources.ws_messages",
        "user_trade2.json",  # trader_side=TAKER
    )
    assert data

    decoder = msgspec.json.Decoder(PolymarketUserTrade)
    msg = decoder.decode(data)
    instrument = TestInstrumentProvider.binary_option()
    account_id = AccountId("POLYMARKET-001")

    # Act
    fill_report = msg.parse_to_fill_report(
        account_id=account_id,
        instrument=instrument,
        client_order_id=None,
        ts_init=0,
        filled_user_order_id=msg.taker_order_id,
    )

    # Assert
    # match_time "1725958681" is in seconds, should convert to 1725958681000000000 nanoseconds
    assert msg.match_time == "1725958681"
    assert fill_report.ts_event == 1725958681000000000  # September 10, 2024


def test_parse_user_trade_taker_commission_with_fees() -> None:
    """
    Test that taker commission is correctly calculated from fee_rate_bps.

    This test uses a taker trade with 200 bps (2%) fees, as documented for Polymarket
    15-minute crypto prediction markets.

    Commission = size * price * (fee_rate_bps / 10000)          = 100 * 0.50 * (200 /
    10000)          = 50 * 0.02 = 1.0 USDC

    """
    # Arrange
    trade_data = {
        "event_type": "trade",
        "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        "bucket_index": 0,
        "fee_rate_bps": "200",  # 2% taker fee (Polymarket 15-min crypto markets)
        "id": "test-taker-trade-001",
        "last_update": "1725958681",
        "maker_address": "0x1234567890123456789012345678901234567890",
        "maker_orders": [
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "0",
                "maker_address": "0x1234567890123456789012345678901234567890",
                "matched_amount": "100",
                "order_id": "0xmaker_order_id",
                "outcome": "Yes",
                "owner": "maker-owner-id",
                "price": "0.50",
            },
        ],
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "match_time": "1725958681",
        "outcome": "Yes",
        "owner": "taker-owner-id",
        "price": "0.50",
        "side": "BUY",
        "size": "100",
        "status": "MINED",
        "taker_order_id": "0xtaker_order_id",
        "timestamp": "1725958681000",
        "trade_owner": "taker-owner-id",
        "trader_side": "TAKER",
        "type": "TRADE",
    }

    decoder = msgspec.json.Decoder(PolymarketUserTrade)
    msg = decoder.decode(msgspec.json.encode(trade_data))
    instrument = TestInstrumentProvider.binary_option()
    account_id = AccountId("POLYMARKET-001")

    # Act
    fill_report = msg.parse_to_fill_report(
        account_id=account_id,
        instrument=instrument,
        client_order_id=None,
        ts_init=0,
        filled_user_order_id=msg.taker_order_id,
    )

    # Assert
    # Commission = 100 * 0.50 * (200 / 10000) = 1.0 USDC.e
    assert fill_report.commission == Money(1.0, USDC_POS)


def test_parse_user_trade_maker_commission_with_fees() -> None:
    """
    Test that maker commission is correctly calculated from maker order's fee_rate_bps.

    For maker fills, the fee_rate_bps is taken from the individual maker_order, not from
    the top-level trade message.

    Commission = matched_amount * price * (fee_rate_bps / 10000)          = 50 * 0.60 *
    (100 / 10000)          = 30 * 0.01 = 0.30 USDC

    """
    # Arrange
    maker_owner = "maker-owner-id"
    maker_order_id = "0xmy_maker_order_id"
    trade_data = {
        "event_type": "trade",
        "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        "bucket_index": 0,
        "fee_rate_bps": "200",  # Taker's fee (not used for maker calculation)
        "id": "test-maker-trade-001",
        "last_update": "1725958681",
        "maker_address": "0x1234567890123456789012345678901234567890",
        "maker_orders": [
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "100",  # 1% maker fee
                "maker_address": "0x1234567890123456789012345678901234567890",
                "matched_amount": "50",
                "order_id": maker_order_id,
                "outcome": "Yes",
                "owner": maker_owner,
                "price": "0.60",
            },
        ],
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "match_time": "1725958681",
        "outcome": "Yes",
        "owner": maker_owner,
        "price": "0.60",
        "side": "SELL",
        "size": "50",
        "status": "MINED",
        "taker_order_id": "0xtaker_order_id",
        "timestamp": "1725958681000",
        "trade_owner": maker_owner,
        "trader_side": "MAKER",
        "type": "TRADE",
    }

    decoder = msgspec.json.Decoder(PolymarketUserTrade)
    msg = decoder.decode(msgspec.json.encode(trade_data))
    instrument = TestInstrumentProvider.binary_option()
    account_id = AccountId("POLYMARKET-001")

    # Act
    fill_report = msg.parse_to_fill_report(
        account_id=account_id,
        instrument=instrument,
        client_order_id=None,
        ts_init=0,
        filled_user_order_id=maker_order_id,
    )

    # Assert
    # Commission = 50 * 0.60 * (100 / 10000) = 0.30 USDC.e
    assert fill_report.commission == Money(0.30, USDC_POS)


def test_parse_user_trade_zero_commission_with_no_fees() -> None:
    """
    Test that commission is zero when fee_rate_bps is "0".

    This verifies the baseline case where no fees apply (most Polymarket markets).

    """
    # Arrange
    trade_data = {
        "event_type": "trade",
        "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        "bucket_index": 0,
        "fee_rate_bps": "0",
        "id": "test-no-fee-trade-001",
        "last_update": "1725958681",
        "maker_address": "0x1234567890123456789012345678901234567890",
        "maker_orders": [
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "0",
                "maker_address": "0x1234567890123456789012345678901234567890",
                "matched_amount": "100",
                "order_id": "0xmaker_order_id",
                "outcome": "Yes",
                "owner": "maker-owner-id",
                "price": "0.50",
            },
        ],
        "market": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "match_time": "1725958681",
        "outcome": "Yes",
        "owner": "taker-owner-id",
        "price": "0.50",
        "side": "BUY",
        "size": "100",
        "status": "MINED",
        "taker_order_id": "0xtaker_order_id",
        "timestamp": "1725958681000",
        "trade_owner": "taker-owner-id",
        "trader_side": "TAKER",
        "type": "TRADE",
    }

    decoder = msgspec.json.Decoder(PolymarketUserTrade)
    msg = decoder.decode(msgspec.json.encode(trade_data))
    instrument = TestInstrumentProvider.binary_option()
    account_id = AccountId("POLYMARKET-001")

    # Act
    fill_report = msg.parse_to_fill_report(
        account_id=account_id,
        instrument=instrument,
        client_order_id=None,
        ts_init=0,
        filled_user_order_id=msg.taker_order_id,
    )

    # Assert
    assert fill_report.commission == Money(0.0, USDC_POS)


@pytest.mark.parametrize(
    ("quantity", "price", "fee_rate_bps", "expected"),
    [
        # Zero fee rate
        (Decimal(100), Decimal("0.50"), Decimal(0), 0.0),
        # Standard fee calculation: 100 * 0.50 * 0.02 = 1.0
        (Decimal(100), Decimal("0.50"), Decimal(200), 1.0),
        # Sub-minimum rounds to zero: 1 * 0.01 * 0.0001 = 0.000001 -> 0.0
        (Decimal(1), Decimal("0.01"), Decimal(1), 0.0),
        # Exactly at minimum: 1 * 1.0 * 0.0001 = 0.0001
        (Decimal(1), Decimal("1.0"), Decimal(1), 0.0001),
        # Rounding to 4 decimals: 123.45 * 0.6789 * 0.015 = 1.25727... -> 1.2572
        (Decimal("123.45"), Decimal("0.6789"), Decimal(150), 1.2572),
    ],
)
def test_calculate_commission(
    quantity: Decimal,
    price: Decimal,
    fee_rate_bps: Decimal,
    expected: float,
) -> None:
    """
    Test commission calculation rounds to 4 decimal places (0.0001 USDC minimum).
    """
    result = calculate_commission(quantity, price, fee_rate_bps)
    assert result == expected


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


def test_trade_report_get_asset_id_taker_returns_trade_asset_id() -> None:
    """
    Test that get_asset_id returns the trade's asset_id when the user is the taker.
    """
    # Arrange
    taker_asset_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
    maker_asset_id = "48331043336612883890938759509493159234755048973500640148014422747788308965732"

    trade_report = PolymarketTradeReport(
        id="test-trade-id",
        taker_order_id="taker-order-123",
        market="0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        asset_id=taker_asset_id,
        side=PolymarketOrderSide.BUY,
        size="100",
        fee_rate_bps="0",
        price="0.5",
        status="MINED",
        match_time="1725868859",
        last_update="1725868885",
        outcome="Yes",
        bucket_index=0,
        owner="test-owner",
        maker_address="0x1234",
        transaction_hash="0xabcd",
        maker_orders=[
            PolymarketMakerOrder(
                asset_id=maker_asset_id,
                fee_rate_bps="0",
                maker_address="0x5678",
                matched_amount="100",
                order_id="maker-order-456",
                outcome="No",
                owner="maker-owner",
                price="0.5",
            ),
        ],
        trader_side=PolymarketLiquiditySide.TAKER,
    )

    # Act
    result = trade_report.get_asset_id("taker-order-123")

    # Assert
    assert result == taker_asset_id


def test_trade_report_get_asset_id_maker_returns_maker_order_asset_id() -> None:
    """
    Test that get_asset_id returns the maker order's asset_id when the user is a maker.

    This is critical for cross-asset matches where a YES maker order is matched against
    a NO taker order (or vice versa). The maker's asset_id may differ from the trade's
    asset_id (which represents the taker's asset).

    Regression test for
    https://github.com/nautechsystems/nautilus_trader/issues/3345

    """
    # Arrange
    taker_asset_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
    maker_asset_id = "48331043336612883890938759509493159234755048973500640148014422747788308965732"

    trade_report = PolymarketTradeReport(
        id="test-trade-id",
        taker_order_id="taker-order-123",
        market="0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        asset_id=taker_asset_id,  # Taker was trading YES
        side=PolymarketOrderSide.BUY,
        size="100",
        fee_rate_bps="0",
        price="0.5",
        status="MINED",
        match_time="1725868859",
        last_update="1725868885",
        outcome="Yes",
        bucket_index=0,
        owner="test-owner",
        maker_address="0x1234",
        transaction_hash="0xabcd",
        maker_orders=[
            PolymarketMakerOrder(
                asset_id=maker_asset_id,  # Maker was trading NO (different asset!)
                fee_rate_bps="0",
                maker_address="0x5678",
                matched_amount="100",
                order_id="maker-order-456",
                outcome="No",
                owner="maker-owner",
                price="0.5",
            ),
        ],
        trader_side=PolymarketLiquiditySide.MAKER,
    )

    # Act
    result = trade_report.get_asset_id("maker-order-456")

    # Assert
    assert result == maker_asset_id
    assert result != taker_asset_id


def test_parse_open_order_to_order_status_report_ts_accepted():
    # Arrange
    # created_at "1725842520" is in seconds (September 9, 2024)
    open_order = PolymarketOpenOrder(
        associate_trades=None,
        id="0x0f76f4dc6eaf3332f4100f2e8a0b4a927351dd64646b7bb12f37df775c657a78",
        status=PolymarketOrderStatus.LIVE,
        market="0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        original_size="5",
        outcome="Yes",
        maker_address="0xa3D82Ed56F4c68d2328Fb8c29e568Ba2cAF7d7c8",
        owner="3e2c94ca-8124-c4c1-c7ea-be1ea21b71fe",
        price="0.513",
        side=PolymarketOrderSide.BUY,
        size_matched="0",
        asset_id="21742633143463906290569050155826241533067272736897614950488156847949938836455",
        expiration="0",
        order_type=PolymarketOrderType.GTC,
        created_at=1725842520,
    )
    instrument = TestInstrumentProvider.binary_option()
    account_id = AccountId("POLYMARKET-001")

    # Act
    report = open_order.parse_to_order_status_report(
        account_id=account_id,
        instrument=instrument,
        client_order_id=None,
        ts_init=0,
    )

    # Assert - created_at in seconds should convert to nanoseconds
    assert report.ts_accepted == 1725842520000000000
    assert report.ts_last == 1725842520000000000


ASSET_ID_YES = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
ASSET_ID_NO = "48331043336612883890938759509493159234755048973500640148014422747788308965732"


@pytest.mark.parametrize(
    ("trader_side", "trade_side", "maker_asset_id", "expected_order_side"),
    [
        # TAKER: always uses trade side directly
        (PolymarketLiquiditySide.TAKER, PolymarketOrderSide.BUY, ASSET_ID_YES, OrderSide.BUY),
        (PolymarketLiquiditySide.TAKER, PolymarketOrderSide.SELL, ASSET_ID_YES, OrderSide.SELL),
        # MAKER same-asset: inverts the side
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.BUY, ASSET_ID_YES, OrderSide.SELL),
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.SELL, ASSET_ID_YES, OrderSide.BUY),
        # MAKER cross-asset: uses trade side (same as taker)
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.BUY, ASSET_ID_NO, OrderSide.BUY),
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.SELL, ASSET_ID_NO, OrderSide.SELL),
    ],
    ids=[
        "taker_buy",
        "taker_sell",
        "maker_same_asset_buy",
        "maker_same_asset_sell",
        "maker_cross_asset_buy",
        "maker_cross_asset_sell",
    ],
)
def test_determine_order_side(
    trader_side: PolymarketLiquiditySide,
    trade_side: PolymarketOrderSide,
    maker_asset_id: str,
    expected_order_side: OrderSide,
) -> None:
    """
    Test determine_order_side() correctly handles cross-asset matching.

    Regression test for
    https://github.com/nautechsystems/nautilus_trader/issues/3357

    """
    result = determine_order_side(
        trader_side=trader_side,
        trade_side=trade_side,
        taker_asset_id=ASSET_ID_YES,
        maker_asset_id=maker_asset_id,
    )
    assert result == expected_order_side


@pytest.mark.parametrize(
    ("trader_side", "trade_side", "maker_asset_id", "expected_order_side"),
    [
        # TAKER: always uses trade side directly
        (PolymarketLiquiditySide.TAKER, PolymarketOrderSide.BUY, ASSET_ID_YES, OrderSide.BUY),
        (PolymarketLiquiditySide.TAKER, PolymarketOrderSide.SELL, ASSET_ID_YES, OrderSide.SELL),
        # MAKER same-asset: inverts the side
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.BUY, ASSET_ID_YES, OrderSide.SELL),
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.SELL, ASSET_ID_YES, OrderSide.BUY),
        # MAKER cross-asset: uses trade side (same as taker)
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.BUY, ASSET_ID_NO, OrderSide.BUY),
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.SELL, ASSET_ID_NO, OrderSide.SELL),
    ],
    ids=[
        "taker_buy",
        "taker_sell",
        "maker_same_asset_buy",
        "maker_same_asset_sell",
        "maker_cross_asset_buy",
        "maker_cross_asset_sell",
    ],
)
def test_polymarket_user_trade_order_side(
    trader_side: PolymarketLiquiditySide,
    trade_side: PolymarketOrderSide,
    maker_asset_id: str,
    expected_order_side: OrderSide,
) -> None:
    """
    Test PolymarketUserTrade.order_side() correctly handles cross-asset matching.

    Regression test for
    https://github.com/nautechsystems/nautilus_trader/issues/3357

    """
    taker_order_id = "taker-order-123"
    maker_order_id = "maker-order-456"

    user_trade = PolymarketUserTrade(
        asset_id=ASSET_ID_YES,
        bucket_index=0,
        fee_rate_bps="0",
        id="test-trade-id",
        last_update="1725868885",
        maker_address="0x1234",
        maker_orders=[
            PolymarketMakerOrder(
                asset_id=maker_asset_id,
                fee_rate_bps="0",
                maker_address="0x5678",
                matched_amount="100",
                order_id=maker_order_id,
                outcome="Yes" if maker_asset_id == ASSET_ID_YES else "No",
                owner="maker-owner",
                price="0.5",
            ),
        ],
        market="0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        match_time="1725868859",
        outcome="Yes",
        owner="test-owner",
        price="0.5",
        side=trade_side,
        size="100",
        status=PolymarketTradeStatus.MINED,
        taker_order_id=taker_order_id,
        timestamp="1725868885871",
        trade_owner="test-owner",
        trader_side=trader_side,
        type=PolymarketEventType.TRADE,
    )

    # Act
    filled_order_id = (
        taker_order_id if trader_side == PolymarketLiquiditySide.TAKER else maker_order_id
    )
    result = user_trade.order_side(filled_order_id)

    # Assert
    assert result == expected_order_side


@pytest.mark.parametrize(
    ("trader_side", "trade_side", "maker_asset_id", "expected_order_side"),
    [
        # TAKER: always uses trade side directly
        (PolymarketLiquiditySide.TAKER, PolymarketOrderSide.BUY, ASSET_ID_YES, OrderSide.BUY),
        (PolymarketLiquiditySide.TAKER, PolymarketOrderSide.SELL, ASSET_ID_YES, OrderSide.SELL),
        # MAKER same-asset: inverts the side
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.BUY, ASSET_ID_YES, OrderSide.SELL),
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.SELL, ASSET_ID_YES, OrderSide.BUY),
        # MAKER cross-asset: uses trade side (same as taker)
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.BUY, ASSET_ID_NO, OrderSide.BUY),
        (PolymarketLiquiditySide.MAKER, PolymarketOrderSide.SELL, ASSET_ID_NO, OrderSide.SELL),
    ],
    ids=[
        "taker_buy",
        "taker_sell",
        "maker_same_asset_buy",
        "maker_same_asset_sell",
        "maker_cross_asset_buy",
        "maker_cross_asset_sell",
    ],
)
def test_polymarket_trade_report_order_side(
    trader_side: PolymarketLiquiditySide,
    trade_side: PolymarketOrderSide,
    maker_asset_id: str,
    expected_order_side: OrderSide,
) -> None:
    """
    Test PolymarketTradeReport.order_side() correctly handles cross-asset matching.

    Regression test for
    https://github.com/nautechsystems/nautilus_trader/issues/3357

    """
    taker_order_id = "taker-order-123"
    maker_order_id = "maker-order-456"

    trade_report = PolymarketTradeReport(
        id="test-trade-id",
        taker_order_id=taker_order_id,
        market="0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        asset_id=ASSET_ID_YES,
        side=trade_side,
        size="100",
        fee_rate_bps="0",
        price="0.5",
        status="MINED",
        match_time="1725868859",
        last_update="1725868885",
        outcome="Yes",
        bucket_index=0,
        owner="test-owner",
        maker_address="0x1234",
        transaction_hash="0xabcd",
        maker_orders=[
            PolymarketMakerOrder(
                asset_id=maker_asset_id,
                fee_rate_bps="0",
                maker_address="0x5678",
                matched_amount="100",
                order_id=maker_order_id,
                outcome="Yes" if maker_asset_id == ASSET_ID_YES else "No",
                owner="maker-owner",
                price="0.5",
            ),
        ],
        trader_side=trader_side,
    )

    # Act
    filled_order_id = (
        taker_order_id if trader_side == PolymarketLiquiditySide.TAKER else maker_order_id
    )
    result = trade_report.order_side(filled_order_id)

    # Assert
    assert result == expected_order_side
