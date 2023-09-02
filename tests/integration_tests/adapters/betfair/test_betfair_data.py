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

import asyncio
from collections import Counter
from unittest.mock import patch

import msgspec
import pytest
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.data import BetfairParser
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDeltas
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import create_betfair_order_book
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.adapters.betfair.providers import parse_market_catalog
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.enums import LogLevel
from nautilus_trader.common.logging import Logger
from nautilus_trader.model.data.base import GenericData
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentClose
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.orderbook import OrderBook
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairDataProvider
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


INSTRUMENTS = []


@pytest.fixture(scope="session", autouse=True)
@patch("nautilus_trader.adapters.betfair.providers.load_markets_metadata")
def instrument_list(mock_load_markets_metadata):
    """
    Prefill `INSTRUMENTS` cache for tests.
    """
    global INSTRUMENTS

    # Setup
    loop = asyncio.get_event_loop()
    logger = Logger(clock=LiveClock(), level_stdout=LogLevel.ERROR)
    client = BetfairTestStubs.betfair_client(loop=loop, logger=logger)
    instrument_provider = BetfairInstrumentProvider(client=client, logger=logger, filters={})

    # Load instruments
    market_ids = BetfairDataProvider.market_ids()
    catalog = parse_market_catalog(BetfairResponses.betting_list_market_catalogue()["result"])
    mock_load_markets_metadata.return_value = [c for c in catalog if c.market_id in market_ids]
    t = loop.create_task(
        instrument_provider.load_all_async(market_filter={"market_id": market_ids}),
    )
    loop.run_until_complete(t)

    # Fill INSTRUMENTS global cache
    INSTRUMENTS.extend(instrument_provider.list_all())
    assert INSTRUMENTS


@pytest.mark.asyncio()
async def test_connect(mocker, data_client, instrument):
    # Arrange
    mocker.patch("nautilus_trader.adapters.betfair.data.BetfairDataClient._post_connect_heartbeat")
    mocker.patch("nautilus_trader.adapters.betfair.data.BetfairMarketStreamClient.connect")
    mocker.patch("nautilus_trader.adapters.betfair.client.BetfairHttpClient.connect")

    # Act
    data_client.connect()
    for _ in range(5):
        await asyncio.sleep(0)  # _connect uses multiple awaits, multiple sleeps required.

    # Assert
    assert data_client.is_connected


@pytest.mark.asyncio()
async def test_subscriptions(data_client, instrument):
    # Arrange, Act
    data_client.subscribe_trade_ticks(instrument.id)
    await asyncio.sleep(0)
    data_client.subscribe_instrument_status_updates(instrument.id)
    await asyncio.sleep(0)
    data_client.subscribe_instrument_close(instrument.id)
    await asyncio.sleep(0)

    # Assert
    assert data_client.subscribed_trade_ticks() == [instrument.id]


def test_market_heartbeat(data_client):
    data_client.on_market_update(BetfairStreaming.mcm_HEARTBEAT())


@patch.object(BetfairDataClient, "degrade")
def test_stream_latency(mock_degrade, data_client):
    # Arrange
    data_client.start()
    assert mock_degrade.call_count == 0

    # Act
    data_client.on_market_update(BetfairStreaming.mcm_latency())

    # Assert
    assert mock_degrade.call_count == 1


@pytest.mark.asyncio()
async def test_market_sub_image_market_def(data_client, mock_data_engine_process):
    # Arrange
    update = BetfairStreaming.mcm_SUB_IMAGE()

    # Act
    data_client.on_market_update(update)

    # Assert - expected messages
    mock_calls = mock_data_engine_process.call_args_list
    result = [type(call.args[0]).__name__ for call in mock_data_engine_process.call_args_list]
    expected = ["InstrumentStatusUpdate"] * 7 + ["OrderBookDeltas"] * 7
    assert result == expected

    # Assert - Check orderbook prices
    orderbook_calls = [
        call.args[0] for call in mock_calls if isinstance(call.args[0], OrderBookDeltas)
    ]
    set_result = {
        delta.order.price.as_double() for deltas in orderbook_calls for delta in deltas.deltas
    }
    set_expected = {
        0.0,
        1.8,
        2.72,
        2.54,
        4.6,
        5.8,
        2.18,
        130.0,
        42.0,
        46.0,
        980.0,
    }
    assert set_result == set_expected


def test_market_update(data_client, mock_data_engine_process):
    # Arrange, Act
    raw = BetfairStreaming.mcm_UPDATE()

    # Act
    data_client.on_market_update(raw)

    # Assert
    book_deltas = mock_data_engine_process.call_args_list[0].args[0]
    assert isinstance(book_deltas, OrderBookDeltas)
    assert {d.action for d in book_deltas.deltas} == {BookAction.UPDATE, BookAction.DELETE}
    assert book_deltas.deltas[0].order.price == betfair_float_to_price(4.7)


def test_market_update_md(data_client, mock_data_engine_process):
    data_client.on_market_update(BetfairStreaming.mcm_UPDATE_md())
    result = [type(call.args[0]).__name__ for call in mock_data_engine_process.call_args_list]
    expected = ["InstrumentStatusUpdate"] * 2
    assert result == expected


def test_market_update_live_image(data_client, mock_data_engine_process):
    data_client.on_market_update(BetfairStreaming.mcm_live_IMAGE())
    result = [type(call.args[0]).__name__ for call in mock_data_engine_process.call_args_list]
    expected = ["OrderBookDeltas"] + ["TradeTick"] * 13 + ["OrderBookDeltas"] + ["TradeTick"] * 17
    assert result == expected


def test_market_update_live_update(data_client, mock_data_engine_process):
    data_client.on_market_update(BetfairStreaming.mcm_live_UPDATE())
    result = [type(call.args[0]).__name__ for call in mock_data_engine_process.call_args_list]
    expected = ["TradeTick", "OrderBookDeltas"]
    assert result == expected


def test_market_bsp(data_client, mock_data_engine_process):
    # Arrange
    update = BetfairStreaming.mcm_BSP()
    provider = data_client.instrument_provider
    for mc in stream_decode(update[0]).mc:
        market_def = msgspec.structs.replace(mc.market_definition, market_id=mc.id)
        instruments = make_instruments(market=market_def, currency="GBP")
        provider.add_bulk(instruments)

    # Act
    for u in update:
        data_client.on_market_update(u)

    # Assert - Count of messages
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    result = Counter([type(args).__name__ for args in mock_call_args])
    expected = {
        "TradeTick": 95,
        "OrderBookDeltas": 11,
        "InstrumentStatusUpdate": 9,
        "BetfairTicker": 8,
        "GenericData": 8,
        "InstrumentClose": 1,
    }
    assert result == expected

    # Assert - Count of generic data messages
    sp_deltas = [
        d
        for deltas in mock_call_args
        if isinstance(deltas, GenericData)
        for d in deltas.data.deltas
    ]
    assert len(sp_deltas) == 30


def test_orderbook_repr(data_client, mock_data_engine_process):
    # Arrange
    raw = BetfairStreaming.mcm_live_IMAGE()

    # Act
    data_client.on_market_update(raw)

    # Assert
    ob_snap = mock_data_engine_process.call_args_list[14][0][0]
    ob = create_betfair_order_book(InstrumentId(Symbol("1"), BETFAIR_VENUE))
    ob.apply(ob_snap)
    assert ob.best_ask_price() == betfair_float_to_price(1.71)
    assert ob.best_bid_price() == betfair_float_to_price(1.70)


def test_orderbook_updates(data_client):
    # Arrange
    order_books: dict[InstrumentId, OrderBook] = {}
    parser = BetfairParser()

    # Act
    for raw_update in BetfairStreaming.market_updates():
        line = stream_decode(raw_update)
        for update in parser.parse(mcm=line):
            if len(order_books) > 1 and update.instrument_id != list(order_books)[1]:
                continue
            if isinstance(update, OrderBookDeltas) and update.is_snapshot:
                order_books[update.instrument_id] = create_betfair_order_book(
                    instrument_id=update.instrument_id,
                )
                order_books[update.instrument_id].apply(update)
            elif isinstance(update, OrderBookDeltas):
                order_books[update.instrument_id].apply(update)
            elif isinstance(update, TradeTick):
                pass
            else:
                raise KeyError

    # Assert
    book = order_books[next(iter(order_books))]
    expected = """╭───────────────┬───────┬──────────────╮
│ bids          │ price │ asks         │
├───────────────┼───────┼──────────────┤
│               │ 1.21  │ [76.380000]  │
│               │ 1.20  │ [156.740000] │
│               │ 1.19  │ [147.790000] │
│ [151.960000]  │ 1.18  │              │
│ [1275.830000] │ 1.17  │              │
│ [932.640000]  │ 1.16  │              │
╰───────────────┴───────┴──────────────╯"""

    result = book.pprint()
    assert result == expected


def test_instrument_opening_events(data_client):
    updates = BetfairDataProvider.market_updates()
    parser = BetfairParser()
    messages = parser.parse(updates[0])
    assert len(messages) == 2
    assert isinstance(messages[0], InstrumentStatusUpdate)
    assert messages[0].status == MarketStatus.PRE_OPEN
    assert isinstance(messages[1], InstrumentStatusUpdate)
    assert messages[0].status == MarketStatus.PRE_OPEN


def test_instrument_in_play_events(data_client):
    parser = BetfairParser()
    events = [
        msg
        for update in BetfairDataProvider.market_updates()
        for msg in parser.parse(update)
        if isinstance(msg, InstrumentStatusUpdate)
    ]
    assert len(events) == 14
    result = [ev.status for ev in events]
    expected = [
        MarketStatus.PRE_OPEN.value,
        MarketStatus.PRE_OPEN.value,
        MarketStatus.PRE_OPEN.value,
        MarketStatus.PRE_OPEN.value,
        MarketStatus.PRE_OPEN.value,
        MarketStatus.PRE_OPEN.value,
        MarketStatus.PAUSE.value,
        MarketStatus.PAUSE.value,
        MarketStatus.OPEN.value,
        MarketStatus.OPEN.value,
        MarketStatus.PAUSE.value,
        MarketStatus.PAUSE.value,
        MarketStatus.CLOSED.value,
        MarketStatus.CLOSED.value,
    ]
    assert result == expected


def test_instrument_closing_events(data_client):
    updates = BetfairDataProvider.market_updates()
    parser = BetfairParser()
    messages = parser.parse(updates[-1])
    assert len(messages) == 4
    assert isinstance(messages[0], InstrumentStatusUpdate)
    assert messages[0].status == MarketStatus.CLOSED
    assert isinstance(messages[2], InstrumentClose)
    assert messages[2].close_price == 1.0000
    assert isinstance(messages[2], InstrumentClose)
    assert messages[2].close_type == InstrumentCloseType.CONTRACT_EXPIRED
    assert isinstance(messages[1], InstrumentStatusUpdate)
    assert messages[1].status == MarketStatus.CLOSED
    assert isinstance(messages[3], InstrumentClose)
    assert messages[3].close_price == 0.0
    assert isinstance(messages[3], InstrumentClose)
    assert messages[3].close_type == InstrumentCloseType.CONTRACT_EXPIRED


def test_betfair_ticker(data_client, mock_data_engine_process) -> None:
    # Arrange
    raw = BetfairStreaming.mcm_UPDATE_tv()

    # Act
    data_client.on_market_update(raw)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    ticker: BetfairTicker = mock_call_args[1]
    assert ticker.last_traded_price == 3.15
    assert ticker.traded_volume == 364.45


def test_betfair_ticker_sp(data_client, mock_data_engine_process):
    # Arrange
    lines = BetfairDataProvider.read_lines("1.206064380.bz2")

    # Act
    for line in lines:
        data_client.on_market_update(line)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    starting_prices_near = [
        t for t in mock_call_args if isinstance(t, BetfairTicker) if t.starting_price_near
    ]
    starting_prices_far = [
        t for t in mock_call_args if isinstance(t, BetfairTicker) if t.starting_price_far
    ]
    assert len(starting_prices_near) == 1739
    assert len(starting_prices_far) == 1182


def test_betfair_starting_price(data_client, mock_data_engine_process):
    # Arrange
    lines = BetfairDataProvider.read_lines("1.206064380.bz2")

    # Act
    for line in lines[-100:]:
        data_client.on_market_update(line)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]

    starting_prices = [
        t
        for t in mock_call_args
        if isinstance(t, GenericData) and isinstance(t.data, BetfairStartingPrice)
    ]
    assert len(starting_prices) == 36


def test_betfair_orderbook(data_client) -> None:
    # Arrange
    books: dict[InstrumentId, OrderBook] = {}
    parser = BetfairParser()

    # Act, Assert
    for update in BetfairDataProvider.market_updates():
        for message in parser.parse(update):
            if message.instrument_id not in books:
                books[message.instrument_id] = create_betfair_order_book(
                    instrument_id=message.instrument_id,
                )
            book = books[message.instrument_id]
            if isinstance(message, OrderBookDeltas):
                book.apply_deltas(message)
            elif isinstance(message, OrderBookDelta):
                book.apply_delta(message)
            elif isinstance(
                message,
                (Ticker, TradeTick, InstrumentStatusUpdate, InstrumentClose),
            ):
                pass
            else:
                raise NotImplementedError(str(type(message)))
            book.check_integrity()  # Asserts correctness


def test_bsp_deltas_apply(data_client, instrument):
    # Arrange
    book = TestDataStubs.make_book(
        instrument=instrument,
        book_type=BookType.L2_MBP,
        asks=[(0.0010000, 55.81)],
    )
    deltas = BSPOrderBookDeltas.from_dict(
        {
            "type": "BSPOrderBookDeltas",
            "instrument_id": instrument.id.value,
            "deltas": msgspec.json.encode(
                [
                    {
                        "type": "OrderBookDelta",
                        "instrument_id": instrument.id.value,
                        "book_type": "L2_MBP",
                        "action": "UPDATE",
                        "order": {
                            "price": "0.990099",
                            "size": "2.0",
                            "side": "BUY",
                            "order_id": 1,
                        },
                        "flags": 0,
                        "sequence": 0,
                        "ts_event": 1667288437852999936,
                        "ts_init": 1667288437852999936,
                    },
                ],
            ),
            "update_id": 0,
            "ts_event": 1667288437852999936,
            "ts_init": 1667288437852999936,
        },
    )

    # Act
    book.apply(deltas)

    # Assert
    assert book.best_ask_price() == betfair_float_to_price(0.001)
    assert book.best_bid_price() == betfair_float_to_price(0.990099)
