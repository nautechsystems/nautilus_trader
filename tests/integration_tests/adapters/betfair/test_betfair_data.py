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

import asyncio
from collections import Counter
from unittest.mock import patch

import msgspec
import pytest
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.constants import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import create_betfair_order_book
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProviderConfig
from nautilus_trader.adapters.betfair.providers import make_instruments
from nautilus_trader.adapters.betfair.providers import parse_market_catalog
from nautilus_trader.core.rust.model import OrderSide
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import SubscribeInstrumentClose
from nautilus_trader.data.messages import SubscribeInstrumentStatus
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatusAction
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
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
    # Get the running loop from pytest-asyncio (session-scoped)
    loop = asyncio.get_running_loop()

    client = BetfairTestStubs.betfair_client(loop=loop)
    market_ids = BetfairDataProvider.market_ids()
    config = BetfairInstrumentProviderConfig(market_ids=market_ids, account_currency="GBP")
    instrument_provider = BetfairInstrumentProvider(client=client, config=config)

    # Load instruments
    catalog = parse_market_catalog(BetfairResponses.betting_list_market_catalogue()["result"])
    mock_load_markets_metadata.return_value = [c for c in catalog if c.market_id in market_ids]
    t = loop.create_task(
        instrument_provider.load_all_async(),
    )
    loop.run_until_complete(t)

    # Fill INSTRUMENTS global cache
    INSTRUMENTS.extend(instrument_provider.list_all())
    assert INSTRUMENTS  # TODO: Fix Betfair symbology
    yield
    # pytest-asyncio manages loop lifecycle, no need to close


@pytest.mark.asyncio()
async def test_connect(mocker, data_client, instrument):
    # Arrange
    mocker.patch("nautilus_trader.adapters.betfair.data.BetfairMarketStreamClient.connect")
    mocker.patch("nautilus_trader.adapters.betfair.client.BetfairHttpClient.connect")
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
    data_client.subscribe_trade_ticks(
        SubscribeTradeTicks(
            instrument_id=instrument.id,
            client_id=None,
            venue=instrument.id.venue,
            command_id=UUID4(),
            ts_init=0,
        ),
    )
    await asyncio.sleep(0)
    data_client.subscribe_instrument_status(
        SubscribeInstrumentStatus(
            instrument_id=instrument.id,
            client_id=None,
            venue=instrument.id.venue,
            command_id=UUID4(),
            ts_init=0,
        ),
    )
    await asyncio.sleep(0)
    data_client.subscribe_instrument_close(
        SubscribeInstrumentClose(
            instrument_id=instrument.id,
            client_id=None,
            venue=instrument.id.venue,
            command_id=UUID4(),
            ts_init=0,
        ),
    )
    await asyncio.sleep(0)

    # Assert
    assert data_client.subscribed_trade_ticks() == [instrument.id]
    assert data_client.subscribed_instrument_status() == [instrument.id]
    assert data_client.subscribed_instrument_close() == [instrument.id]


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
    expected = (
        ["BettingInstrument"] * 7
        + ["InstrumentStatus"] * 7
        + ["OrderBookDeltas"] * 7
        + ["CustomData"]
    )
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
    expected = ["BettingInstrument"] * 2 + ["InstrumentStatus"] * 2 + ["CustomData"]
    assert result == expected


def test_market_update_live_image(data_client, mock_data_engine_process):
    data_client.on_market_update(BetfairStreaming.mcm_live_IMAGE())
    result = [type(call.args[0]).__name__ for call in mock_data_engine_process.call_args_list]
    expected = (
        ["OrderBookDeltas"]
        + ["TradeTick"] * 13
        + ["OrderBookDeltas"]
        + ["TradeTick"] * 17
        + ["CustomData"]
    )
    assert result == expected


def test_market_update_live_update(data_client, mock_data_engine_process):
    data_client.on_market_update(BetfairStreaming.mcm_live_UPDATE())
    result = [type(call.args[0]).__name__ for call in mock_data_engine_process.call_args_list]
    expected = ["TradeTick", "OrderBookDeltas", "CustomData"]
    assert result == expected


def test_market_bsp(data_client, mock_data_engine_process):
    # Arrange
    update = BetfairStreaming.mcm_BSP()
    provider = data_client.instrument_provider
    for mc in stream_decode(update[0]).mc:
        market_def = msgspec.structs.replace(mc.market_definition, market_id=mc.id)
        instruments = make_instruments(
            market=market_def,
            currency="GBP",
            ts_event=0,
            ts_init=0,
        )
        provider.add_bulk(instruments)

    # Act
    for u in update:
        data_client.on_market_update(u)

    # Assert - Count of messages
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    result = Counter([type(args).__name__ for args in mock_call_args])
    expected = {
        "BettingInstrument": 9,
        "TradeTick": 95,
        "OrderBookDeltas": 11,
        "InstrumentStatus": 9,
        "CustomData": 40,
        "InstrumentClose": 1,
    }
    assert dict(result) == expected

    # Assert - Count of custom data messages
    sp_deltas = [
        data
        for data in mock_call_args
        if isinstance(data, CustomData) and isinstance(data.data, BSPOrderBookDelta)
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


def test_orderbook_updates(data_client, parser):
    # Arrange
    order_books: dict[InstrumentId, OrderBook] = {}

    # Act
    for raw_update in BetfairStreaming.market_updates():
        line = stream_decode(raw_update)
        for update in parser.parse(mcm=line):
            if isinstance(update, CustomData):
                continue
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
    expected = (
        "bid_levels: 18\n"
        "ask_levels: 41\n"
        "sequence: 0\n"
        "update_count: 60\n"
        "ts_last: 1617253902640999936\n"
        "╭───────────┬───────┬──────────╮\n"
        "│ bids      │ price │ asks     │\n"
        "├───────────┼───────┼──────────┤\n"
        "│           │ 1.21  │ [76.38]  │\n"
        "│           │ 1.20  │ [156.74] │\n"
        "│           │ 1.19  │ [147.79] │\n"
        "│ [151.96]  │ 1.18  │          │\n"
        "│ [1275.83] │ 1.17  │          │\n"
        "│ [932.64]  │ 1.16  │          │\n"
        "╰───────────┴───────┴──────────╯"
    )

    result = book.pprint()
    assert result == expected


def test_instrument_opening_events(data_client, parser):
    updates = BetfairDataProvider.market_updates()
    messages = parser.parse(updates[0])
    assert len(messages) == 5
    assert isinstance(messages[0], BettingInstrument)
    assert isinstance(messages[2], InstrumentStatus)
    assert messages[2].action == MarketStatusAction.PRE_OPEN
    assert isinstance(messages[3], InstrumentStatus)
    assert messages[3].action == MarketStatusAction.PRE_OPEN
    assert isinstance(messages[4], CustomData)


def test_instrument_in_play_events(data_client, parser):
    events = [
        msg
        for update in BetfairDataProvider.market_updates()
        for msg in parser.parse(update)
        if isinstance(msg, InstrumentStatus)
    ]
    assert len(events) == 14
    result = [ev.action for ev in events]
    expected = [
        MarketStatusAction.PRE_OPEN.value,
        MarketStatusAction.PRE_OPEN.value,
        MarketStatusAction.PRE_OPEN.value,
        MarketStatusAction.PRE_OPEN.value,
        MarketStatusAction.PRE_OPEN.value,
        MarketStatusAction.PRE_OPEN.value,
        MarketStatusAction.PAUSE.value,
        MarketStatusAction.PAUSE.value,
        MarketStatusAction.TRADING.value,
        MarketStatusAction.TRADING.value,
        MarketStatusAction.PAUSE.value,
        MarketStatusAction.PAUSE.value,
        MarketStatusAction.CLOSE.value,
        MarketStatusAction.CLOSE.value,
    ]
    assert result == expected


def test_instrument_update(data_client, cache, parser):
    # Arrange
    [instrument] = cache.instruments()
    assert instrument.info == {}

    # Act
    updates = BetfairDataProvider.market_updates()
    for upd in updates[:1]:
        data_client._on_market_update(mcm=upd)
    new_instrument = cache.instruments()

    # Assert
    result = new_instrument[2].info
    assert len(result) == 29


def test_instrument_closing_events(data_client, parser):
    updates = BetfairDataProvider.market_updates()
    messages = parser.parse(updates[-1])
    assert len(messages) == 7
    ins1, ins2, status1, status2, close1, close2, completed = messages

    # Instrument1
    assert isinstance(ins1, BettingInstrument)
    assert isinstance(status1, InstrumentStatus)
    assert status1.action == MarketStatusAction.CLOSE
    assert isinstance(close1, InstrumentClose)
    assert close1.close_price == 1.0000
    assert close1.close_type == InstrumentCloseType.CONTRACT_EXPIRED

    # Instrument2
    assert isinstance(ins2, BettingInstrument)
    assert isinstance(close2, InstrumentClose)
    assert isinstance(status2, InstrumentStatus)
    assert status2.action == MarketStatusAction.CLOSE
    assert close2.close_price == 0.0
    assert close2.close_type == InstrumentCloseType.CONTRACT_EXPIRED

    assert isinstance(completed, CustomData)


def test_betfair_ticker(data_client, mock_data_engine_process) -> None:
    # Arrange
    raw = BetfairStreaming.mcm_UPDATE_tv()

    # Act
    data_client.on_market_update(raw)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    ticker: BetfairTicker = mock_call_args[1].data
    assert ticker.last_traded_price == 3.15
    assert ticker.traded_volume == 364.45
    assert (
        str(ticker)
        == "BetfairTicker(instrument_id=1-176621195-42153-None.BETFAIR, ltp=3.15, tv=364.45, spn=None, spf=None, ts_init=1471370160471000064)"
    )


def test_betfair_ticker_sp(data_client, mock_data_engine_process):
    # Arrange
    lines = BetfairDataProvider.read_lines("1-206064380.bz2")

    # Act
    for line in lines:
        line = line.replace(b'"con":true', b'"con":false')
        data_client.on_market_update(line)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    custom_data = [data.data for data in mock_call_args if isinstance(data, CustomData)]
    starting_prices_near = [
        data for data in custom_data if isinstance(data, BetfairTicker) if data.starting_price_near
    ]
    starting_prices_far = [
        data for data in custom_data if isinstance(data, BetfairTicker) and data.starting_price_far
    ]
    assert len(starting_prices_near) == 1739
    assert len(starting_prices_far) == 1182


def test_betfair_starting_price(data_client, mock_data_engine_process):
    # Arrange
    lines = BetfairDataProvider.read_lines("1-206064380.bz2")

    # Act
    for line in lines[-100:]:
        line = line.replace(b'"con":true', b'"con":false')
        data_client.on_market_update(line)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    custom_data = [data.data for data in mock_call_args if isinstance(data, CustomData)]
    starting_prices = [data for data in custom_data if isinstance(data, BetfairStartingPrice)]
    assert len(starting_prices) == 36


def test_betfair_orderbook(data_client, parser) -> None:
    # Arrange
    books: dict[InstrumentId, OrderBook] = {}

    # Act, Assert
    for update in BetfairDataProvider.market_updates():
        for message in parser.parse(update):
            if isinstance(message, BettingInstrument | CustomData):
                continue
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
                BetfairTicker | TradeTick | InstrumentStatus | InstrumentClose,
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
        asks=[(1000, 55.81)],
    )

    bsp_delta = BSPOrderBookDelta(
        instrument_id=instrument.id,
        action=BookAction.UPDATE,
        order=BookOrder(
            price=Price.from_str("1.01"),
            size=Quantity.from_str("2.0"),
            side=OrderSide.BUY,
            order_id=1,
        ),
        flags=0,
        sequence=0,
        ts_event=1667288437852999936,
        ts_init=1667288437852999936,
    )

    # Act
    book.apply(bsp_delta)

    # Assert
    assert book.best_ask_price() == betfair_float_to_price(1000)
    assert book.best_bid_price() == betfair_float_to_price(1.01)


@pytest.mark.asyncio
async def test_subscribe_instruments(data_client, instrument):
    await data_client._subscribe_instrument(instrument.id)
