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
import pandas as pd
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
from nautilus_trader.adapters.polymarket.common.parsing import basis_points_as_decimal
from nautilus_trader.adapters.polymarket.common.parsing import calculate_commission
from nautilus_trader.adapters.polymarket.common.parsing import determine_order_side
from nautilus_trader.adapters.polymarket.common.parsing import determine_trade_id
from nautilus_trader.adapters.polymarket.common.parsing import extract_fee_rates
from nautilus_trader.adapters.polymarket.common.parsing import parse_polymarket_instrument
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookLevel
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketBookSnapshot
from nautilus_trader.adapters.polymarket.schemas.book import PolymarketQuote
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
from nautilus_trader.model.currencies import pUSD
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
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
    # CLOB payloads in markets.json have no feeSchedule so fees default to zero
    for instrument in instruments:
        assert instrument.maker_fee == Decimal(0)
        assert instrument.taker_fee == Decimal(0)


@pytest.mark.parametrize(
    ("market_info", "expected"),
    [
        ({}, (Decimal(0), Decimal(0))),
        ({"feeSchedule": {"rate": 0.03}}, (Decimal(0), Decimal("0.03"))),
        (
            {"_gamma_original": {"feeSchedule": {"rate": 0.072}}},
            (Decimal(0), Decimal("0.072")),
        ),
        ({"feeSchedule": {"rate": None}}, (Decimal(0), Decimal(0))),
        ({"_gamma_original": {}}, (Decimal(0), Decimal(0))),
        (
            # Top-level feeSchedule takes precedence over _gamma_original
            {
                "feeSchedule": {"rate": 0.04},
                "_gamma_original": {"feeSchedule": {"rate": 0.072}},
            },
            (Decimal(0), Decimal("0.04")),
        ),
    ],
)
def test_extract_fee_rates(
    market_info: dict,
    expected: tuple[Decimal, Decimal],
) -> None:
    """
    Polymarket charges fees from feeSchedule.rate; maker is always zero.

    References
    ----------
    https://docs.polymarket.com/trading/fees

    """
    assert extract_fee_rates(market_info) == expected


def test_parse_polymarket_instrument_populates_taker_fee_from_fee_schedule() -> None:
    """
    parse_polymarket_instrument should populate taker_fee from an attached feeSchedule,
    leaving maker_fee at zero.
    """
    # Arrange: CLOB-shaped payload with a Gamma feeSchedule stitched on
    token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
    market_info: dict[str, object] = {
        "condition_id": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "question": "Test market?",
        "minimum_tick_size": 0.001,
        "minimum_order_size": 5,
        "end_date_iso": "2025-12-31T00:00:00Z",
        "maker_base_fee": 1000,  # Max cap, must be ignored
        "taker_base_fee": 1000,  # Max cap, must be ignored
        "feeSchedule": {"rate": 0.03, "takerOnly": True, "exponent": 1, "rebateRate": 0.25},
        "tokens": [{"token_id": token_id, "outcome": "Yes"}],
    }

    # Act
    instrument = parse_polymarket_instrument(
        market_info=market_info,
        token_id=token_id,
        outcome="Yes",
        ts_init=0,
    )

    # Assert
    assert instrument.maker_fee == Decimal(0)
    assert instrument.taker_fee == Decimal("0.03")


def test_parse_polymarket_instrument_defaults_fees_without_fee_schedule() -> None:
    """
    parse_polymarket_instrument leaves both fees at zero when feeSchedule is absent.
    """
    # Arrange
    token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
    market_info: dict[str, object] = {
        "condition_id": "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917",
        "question": "Test market?",
        "minimum_tick_size": 0.001,
        "minimum_order_size": 5,
        "end_date_iso": "2025-12-31T00:00:00Z",
        "maker_base_fee": 1000,
        "taker_base_fee": 1000,
        "tokens": [{"token_id": token_id, "outcome": "Yes"}],
    }

    # Act
    instrument = parse_polymarket_instrument(
        market_info=market_info,
        token_id=token_id,
        outcome="Yes",
        ts_init=0,
    )

    # Assert
    assert instrument.maker_fee == Decimal(0)
    assert instrument.taker_fee == Decimal(0)


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


def _make_last_quote(
    instrument: BinaryOption,
    bid_price: float,
    ask_price: float,
    bid_size: float = 500.0,
    ask_size: float = 600.0,
) -> QuoteTick:
    return QuoteTick(
        instrument_id=instrument.id,
        bid_price=instrument.make_price(bid_price),
        ask_price=instrument.make_price(ask_price),
        bid_size=instrument.make_qty(bid_size),
        ask_size=instrument.make_qty(ask_size),
        ts_event=0,
        ts_init=0,
    )


def test_parse_to_quote_ticks_uses_best_bid_ask_not_changed_level() -> None:
    """
    Regression test for https://github.com/nautechsystems/nautilus_trader/issues/3905.

    A size=0 removal at a deep level must not be used as the new top of book.
    The post-change top is carried in `best_bid`/`best_ask`.

    """
    # Arrange
    instrument = TestInstrumentProvider.binary_option()
    last_quote = _make_last_quote(instrument, bid_price=0.009, ask_price=0.155)

    quotes = PolymarketQuotes(
        market="0x13e081035bf10a67c7faf2adde3912d26da6986a5aa7c097d0a134ab4a075717",
        price_changes=[
            PolymarketQuote(
                asset_id="39790575150005864502035112061767534106841339306960941347649609867025460648353",
                price="0.156",
                side=PolymarketOrderSide.SELL,
                size="0",
                hash="2aa07bfad07528cd22fffec6dbf7005e1f979720",
                best_bid="0.009",
                best_ask="0.155",
            ),
        ],
        timestamp="1776713442539",
    )

    # Act
    result = quotes.parse_to_quote_ticks(
        instrument=instrument,
        last_quote=last_quote,
        ts_init=1,
    )

    # Assert
    assert len(result) == 1
    quote = result[0]
    assert quote.bid_price == instrument.make_price(0.009)
    assert quote.ask_price == instrument.make_price(0.155)
    # Changed level (0.156) is not the new top; sizes must carry from last_quote
    assert quote.bid_size == last_quote.bid_size
    assert quote.ask_size == last_quote.ask_size


@pytest.mark.parametrize(
    ("side", "changed_price", "at_top"),
    [
        (PolymarketOrderSide.BUY, "0.52", True),  # BUY at new top
        (PolymarketOrderSide.BUY, "0.48", False),  # BUY at deeper level
        (PolymarketOrderSide.SELL, "0.60", True),  # SELL at new top
        (PolymarketOrderSide.SELL, "0.65", False),  # SELL at deeper level
    ],
    ids=["buy-top", "buy-deep", "sell-top", "sell-deep"],
)
def test_parse_to_quote_ticks_size_propagation(
    side: PolymarketOrderSide,
    changed_price: str,
    at_top: bool,
) -> None:
    """
    When the changed level is the new top, its size is authoritative for that side;
    otherwise the top size carries from `last_quote`.

    The untouched side always
    carries from `last_quote`.

    """
    # Arrange
    instrument = TestInstrumentProvider.binary_option()
    last_quote = _make_last_quote(instrument, bid_price=0.50, ask_price=0.60)

    quotes = PolymarketQuotes(
        market="0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        price_changes=[
            PolymarketQuote(
                asset_id="23360939988679364027624185518382759743328544433592111535569478055890815567848",
                price=changed_price,
                side=side,
                size="1234",
                hash="aa",
                best_bid="0.52",
                best_ask="0.60",
            ),
        ],
        timestamp="1729084877448",
    )

    # Act
    result = quotes.parse_to_quote_ticks(
        instrument=instrument,
        last_quote=last_quote,
        ts_init=1,
    )

    # Assert
    assert len(result) == 1
    quote = result[0]
    assert quote.bid_price == instrument.make_price(0.52)
    assert quote.ask_price == instrument.make_price(0.60)

    if side == PolymarketOrderSide.BUY:
        expected_bid_size = instrument.make_qty(1234) if at_top else last_quote.bid_size
        assert quote.bid_size == expected_bid_size
        assert quote.ask_size == last_quote.ask_size
    else:
        expected_ask_size = instrument.make_qty(1234) if at_top else last_quote.ask_size
        assert quote.ask_size == expected_ask_size
        assert quote.bid_size == last_quote.bid_size


def test_parse_to_quote_ticks_skips_when_best_bid_ask_is_none() -> None:
    """
    `best_bid`/`best_ask` being `None` (Optional fields missing from the payload)
    triggers the explicit None guard.
    """
    # Arrange
    instrument = TestInstrumentProvider.binary_option()
    last_quote = _make_last_quote(instrument, bid_price=0.50, ask_price=0.60)

    quotes = PolymarketQuotes(
        market="0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        price_changes=[
            PolymarketQuote(
                asset_id="1",
                price="0.50",
                side=PolymarketOrderSide.BUY,
                size="10",
                hash="a",
                best_bid=None,
                best_ask=None,
            ),
        ],
        timestamp="1729084877448",
    )

    # Act
    result = quotes.parse_to_quote_ticks(
        instrument=instrument,
        last_quote=last_quote,
        ts_init=1,
    )

    # Assert
    assert result == []


def test_parse_to_quote_ticks_skips_invalid_tops() -> None:
    """
    Entries with missing/zero best_bid or best_ask, or a locked/crossed book, are
    skipped rather than producing degenerate quotes.
    """
    # Arrange
    instrument = TestInstrumentProvider.binary_option()
    last_quote = _make_last_quote(instrument, bid_price=0.50, ask_price=0.60)

    quotes = PolymarketQuotes(
        market="0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        price_changes=[
            # Zero best_bid (empty bid side)
            PolymarketQuote(
                asset_id="1",
                price="0.50",
                side=PolymarketOrderSide.SELL,
                size="10",
                hash="a",
                best_bid="0",
                best_ask="0.60",
            ),
            # Crossed (bid >= ask)
            PolymarketQuote(
                asset_id="1",
                price="0.70",
                side=PolymarketOrderSide.BUY,
                size="10",
                hash="b",
                best_bid="0.70",
                best_ask="0.60",
            ),
        ],
        timestamp="1729084877448",
    )

    # Act
    result = quotes.parse_to_quote_ticks(
        instrument=instrument,
        last_quote=last_quote,
        ts_init=1,
    )

    # Assert
    assert result == []


@pytest.mark.parametrize(
    ("bids", "asks", "expected_len"),
    [
        (
            [
                PolymarketBookLevel(price="0.40", size="150"),
                PolymarketBookLevel(price="0.50", size="250"),
            ],
            [
                PolymarketBookLevel(price="0.52", size="100"),
                PolymarketBookLevel(price="0.60", size="200"),
            ],
            5,
        ),
        (
            [
                PolymarketBookLevel(price="0.40", size="150"),
                PolymarketBookLevel(price="0.50", size="250"),
            ],
            [],
            3,
        ),
        (
            [],
            [
                PolymarketBookLevel(price="0.52", size="100"),
                PolymarketBookLevel(price="0.60", size="200"),
            ],
            3,
        ),
    ],
    ids=["two-sided", "bids-only", "asks-only"],
)
def test_parse_to_snapshot_flags_snapshot_bit_on_every_delta(
    bids: list[PolymarketBookLevel],
    asks: list[PolymarketBookLevel],
    expected_len: int,
) -> None:
    """
    Every snapshot delta (CLEAR + ADDs) must carry F_SNAPSHOT so downstream consumers
    (data engine, wranglers) can distinguish the opening rebuild from an incremental
    book reset.

    F_LAST must be set on exactly one delta (the last).

    """
    # Arrange
    snapshot = PolymarketBookSnapshot(
        market="0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        asset_id="23360939988679364027624185518382759743328544433592111535569478055890815567848",
        bids=bids,
        asks=asks,
        timestamp="1728799418260",
    )
    instrument = TestInstrumentProvider.binary_option()

    # Act
    deltas = snapshot.parse_to_snapshot(instrument=instrument, ts_init=1)

    # Assert
    assert deltas is not None
    assert len(deltas.deltas) == expected_len

    for delta in deltas.deltas:
        assert delta.flags & RecordFlag.F_SNAPSHOT, f"F_SNAPSHOT missing from {delta}"

    f_last_count = sum(1 for d in deltas.deltas if d.flags & RecordFlag.F_LAST)
    assert f_last_count == 1
    assert deltas.deltas[-1].flags & RecordFlag.F_LAST


def test_parse_to_deltas_flags_last_on_final_only() -> None:
    """
    F_LAST must be set on exactly the final delta in the batch, not on every entry.
    """
    # Arrange
    quotes = PolymarketQuotes(
        market="0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        price_changes=[
            PolymarketQuote(
                asset_id="1",
                price="0.50",
                side=PolymarketOrderSide.BUY,
                size="10",
                hash="a",
                best_bid="0.50",
                best_ask="0.60",
            ),
            PolymarketQuote(
                asset_id="1",
                price="0.48",
                side=PolymarketOrderSide.BUY,
                size="20",
                hash="b",
                best_bid="0.50",
                best_ask="0.60",
            ),
            PolymarketQuote(
                asset_id="1",
                price="0.60",
                side=PolymarketOrderSide.SELL,
                size="0",
                hash="c",
                best_bid="0.50",
                best_ask="0.62",
            ),
        ],
        timestamp="1729084877448",
    )
    instrument = TestInstrumentProvider.binary_option()

    # Act
    deltas = quotes.parse_to_deltas(instrument=instrument, ts_init=1)

    # Assert
    assert len(deltas.deltas) == 3
    assert deltas.deltas[0].flags == 0
    assert deltas.deltas[1].flags == 0
    assert deltas.deltas[2].flags & RecordFlag.F_LAST


def test_polymarket_quote_decodes_without_best_bid_ask() -> None:
    """
    `best_bid`/`best_ask` are optional; a payload that omits them must decode with the
    fields defaulting to None rather than failing at the msgspec layer.
    """
    # Arrange
    payload = {
        "market": "0x1a4f04c2e6c000d9fc524eb12e7333217411a226c34745af140f195c0227cd5f",
        "price_changes": [
            {
                "asset_id": "1",
                "price": "0.50",
                "side": "BUY",
                "size": "10",
                "hash": "a",
            },
        ],
        "event_type": "price_change",
        "timestamp": "1729084877448",
    }

    # Act
    quotes = msgspec.json.decode(msgspec.json.encode(payload), type=PolymarketQuotes)

    # Assert
    assert len(quotes.price_changes) == 1
    assert quotes.price_changes[0].best_bid is None
    assert quotes.price_changes[0].best_ask is None


def test_determine_trade_id_is_deterministic() -> None:
    id1 = determine_trade_id("asset-1", PolymarketOrderSide.BUY, "0.5", "10", "1700000")
    id2 = determine_trade_id("asset-1", PolymarketOrderSide.BUY, "0.5", "10", "1700000")
    assert id1 == id2


def test_determine_trade_id_differentiates_sides() -> None:
    buy = determine_trade_id("asset-1", PolymarketOrderSide.BUY, "0.5", "10", "1700000")
    sell = determine_trade_id("asset-1", PolymarketOrderSide.SELL, "0.5", "10", "1700000")
    assert buy != sell


def test_determine_trade_id_field_delimiter_prevents_collision() -> None:
    # "0.12" + "34" would collide with "0.1" + "234" if fields were concatenated
    a = determine_trade_id("asset-1", PolymarketOrderSide.BUY, "0.12", "34", "1700000")
    b = determine_trade_id("asset-1", PolymarketOrderSide.BUY, "0.1", "234", "1700000")
    assert a != b


def test_determine_trade_id_format() -> None:
    trade_id = determine_trade_id("asset-1", PolymarketOrderSide.BUY, "0.5", "10", "1700000")
    value = trade_id.value
    assert len(value) == 16
    assert all(c in "0123456789abcdef" for c in value)


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


def test_maker_order_without_side_omits_field_in_encoded_json() -> None:
    # PolymarketMakerOrder.side is optional because legacy / WS-channel
    # payloads omit it; the V2 REST trade response carries it. The struct
    # must NOT serialize the absent field as `"side": null` because that
    # would corrupt fill `info=msg.to_dict()` payloads and the existing
    # dict-shape regression tests. `omit_defaults=True` enforces this.
    order = PolymarketMakerOrder(
        asset_id="x",
        fee_rate_bps="0",
        maker_address="y",
        matched_amount="1",
        order_id="z",
        outcome="Yes",
        owner="o",
        price="0.5",
    )

    encoded = msgspec.json.decode(msgspec.json.encode(order))

    assert "side" not in encoded
    assert encoded["asset_id"] == "x"


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
            "currency": "pUSD",
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


def _binary_option_with_taker_fee(taker_fee: Decimal) -> BinaryOption:
    base = TestInstrumentProvider.binary_option()
    return BinaryOption(
        instrument_id=base.id,
        raw_symbol=base.raw_symbol,
        outcome=base.outcome,
        description=base.description,
        asset_class=base.asset_class,
        currency=base.quote_currency,
        price_precision=base.price_precision,
        price_increment=base.price_increment,
        size_precision=base.size_precision,
        size_increment=base.size_increment,
        activation_ns=base.activation_ns,
        expiration_ns=base.expiration_ns,
        max_quantity=base.max_quantity,
        min_quantity=base.min_quantity,
        maker_fee=Decimal(0),
        taker_fee=taker_fee,
        ts_event=base.ts_event,
        ts_init=base.ts_init,
    )


def test_parse_user_trade_taker_commission_with_fees() -> None:
    """
    Test that taker commission uses the instrument's effective feeRate and follows the
    Polymarket formula fee = C * feeRate * p * (1 - p).

    Uses the sports-market rate (0.03) so the result matches docs example.

    Commission = 100 * 0.03 * 0.5 * 0.5 = 0.75 USDC

    References
    ----------
    https://docs.polymarket.com/trading/fees

    """
    # Arrange
    trade_data = {
        "event_type": "trade",
        "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        "bucket_index": 0,
        "fee_rate_bps": "1000",  # Max fee cap from order signing; not used for commission
        "id": "test-taker-trade-001",
        "last_update": "1725958681",
        "maker_address": "0x1234567890123456789012345678901234567890",
        "maker_orders": [
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "1000",
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
    instrument = _binary_option_with_taker_fee(Decimal("0.03"))
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
    assert fill_report.commission == Money(0.75, pUSD)


def test_parse_user_trade_maker_commission_is_zero() -> None:
    """
    Test that maker fills never pay commission, regardless of feeRate.

    Polymarket docs: "Makers are never charged fees. Only takers pay fees."

    References
    ----------
    https://docs.polymarket.com/trading/fees

    """
    # Arrange
    maker_owner = "maker-owner-id"
    maker_order_id = "0xmy_maker_order_id"
    trade_data = {
        "event_type": "trade",
        "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
        "bucket_index": 0,
        "fee_rate_bps": "1000",
        "id": "test-maker-trade-001",
        "last_update": "1725958681",
        "maker_address": "0x1234567890123456789012345678901234567890",
        "maker_orders": [
            {
                "asset_id": "21742633143463906290569050155826241533067272736897614950488156847949938836455",
                "fee_rate_bps": "1000",
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
    instrument = _binary_option_with_taker_fee(Decimal("0.03"))
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
    assert fill_report.commission == Money(0, pUSD)


def test_parse_user_trade_zero_commission_with_no_fees() -> None:
    """
    Test that commission is zero when the instrument has no taker fee.

    This verifies the baseline case where no fees apply (CLOB-only instruments or
    markets with a zero feeSchedule.rate).

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
    assert fill_report.commission == Money(0.0, pUSD)


@pytest.mark.parametrize(
    ("basis_points", "expected"),
    [
        (Decimal(0), Decimal(0)),
        (Decimal(1), Decimal("0.0001")),
        (Decimal(100), Decimal("0.01")),
        (Decimal(200), Decimal("0.02")),
        (Decimal(10000), Decimal(1)),
    ],
)
def test_basis_points_as_decimal(basis_points: Decimal, expected: Decimal) -> None:
    result = basis_points_as_decimal(basis_points)
    assert result == expected


@pytest.mark.parametrize(
    ("quantity", "price", "fee_rate", "liquidity_side", "expected"),
    [
        # Maker fills never pay commission regardless of rate
        (Decimal(100), Decimal("0.50"), Decimal("0.03"), LiquiditySide.MAKER, 0.0),
        # Taker with zero rate
        (Decimal(100), Decimal("0.50"), Decimal(0), LiquiditySide.TAKER, 0.0),
        # Crypto rate at p=0.5 peaks fee: 100 * 0.072 * 0.5 * 0.5 = 1.8
        (Decimal(100), Decimal("0.50"), Decimal("0.072"), LiquiditySide.TAKER, 1.8),
        # Crypto rate symmetric around p=0.5: same at 0.3 and 0.7
        (Decimal(100), Decimal("0.30"), Decimal("0.072"), LiquiditySide.TAKER, 1.512),
        (Decimal(100), Decimal("0.70"), Decimal("0.072"), LiquiditySide.TAKER, 1.512),
        # Sports rate at p=0.5: 100 * 0.03 * 0.5 * 0.5 = 0.75
        (Decimal(100), Decimal("0.50"), Decimal("0.03"), LiquiditySide.TAKER, 0.75),
        # Sub-minimum rounds to zero: 1 * 0.01 * 0.0001 * 0.99 = 9.9e-7 -> 0.0
        (Decimal(1), Decimal("0.01"), Decimal("0.0001"), LiquiditySide.TAKER, 0.0),
        # Exactly at 5-decimal minimum after rounding
        (Decimal(1), Decimal("0.50"), Decimal("0.00004"), LiquiditySide.TAKER, 1e-05),
    ],
)
def test_calculate_commission(
    quantity: Decimal,
    price: Decimal,
    fee_rate: Decimal,
    liquidity_side: LiquiditySide,
    expected: float,
) -> None:
    """
    Polymarket fee formula: fee = C * feeRate * p * (1 - p).

    References
    ----------
    https://docs.polymarket.com/trading/fees

    """
    result = calculate_commission(
        quantity=quantity,
        price=price,
        fee_rate=fee_rate,
        liquidity_side=liquidity_side,
    )
    assert result == pytest.approx(expected, abs=1e-9)


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
            "currency": "pUSD",
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


def test_parse_open_order_to_order_status_report_zero_expiration_is_none():
    # `expiration="0"` is the V2 sentinel for non-GTD orders; it must
    # surface as `expire_time=None`, not 1970-01-01.
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

    report = open_order.parse_to_order_status_report(
        account_id=AccountId("POLYMARKET-001"),
        instrument=instrument,
        client_order_id=None,
        ts_init=0,
    )

    assert report.expire_time is None


def test_parse_open_order_to_order_status_report_nonzero_expiration_is_seconds():
    # V2 emits expiration as Unix seconds. 1735689600 == 2025-01-01 00:00:00 UTC.
    # Pre-fix this was parsed as ms and produced 1970-01-21.
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
        expiration="1735689600",
        order_type=PolymarketOrderType.GTD,
        created_at=1725842520,
    )
    instrument = TestInstrumentProvider.binary_option()

    report = open_order.parse_to_order_status_report(
        account_id=AccountId("POLYMARKET-001"),
        instrument=instrument,
        client_order_id=None,
        ts_init=0,
    )

    expected = pd.Timestamp(1735689600, unit="s", tz="UTC")
    assert report.expire_time == expected


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
