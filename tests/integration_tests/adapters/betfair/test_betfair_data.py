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

import asyncio
from collections import Counter
from pathlib import Path
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import msgspec
import pytest
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.config import BetfairDataClientConfig
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.data_types import BetfairOrderVoided
from nautilus_trader.adapters.betfair.data_types import BetfairRaceProgress
from nautilus_trader.adapters.betfair.data_types import BetfairRaceRunnerData
from nautilus_trader.adapters.betfair.data_types import BetfairStartingPrice
from nautilus_trader.adapters.betfair.data_types import BetfairTicker
from nautilus_trader.adapters.betfair.data_types import BSPOrderBookDelta
from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
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
from nautilus_trader.model.data import DataType
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
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.functions import eventually
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
    # Create a new event loop for sync fixture
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)

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
    return
    # pytest-asyncio manages loop lifecycle, no need to close


@pytest.mark.asyncio
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


@pytest.mark.asyncio
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


def test_race_stream_client_created_when_configured(
    mocker,
    betfair_client,
    instrument_provider,
    instrument,
    venue,
    event_loop,
    msgbus,
    cache,
    clock,
):
    """
    Verify that subscribe_race_data=True creates a race stream client on a separate
    connection to sports-data-stream-api.betfair.com.
    """
    # Arrange
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_client",
        return_value=betfair_client,
    )
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_instrument_provider",
        return_value=instrument_provider,
    )
    instrument_provider.add(instrument)

    # Act
    client = BetfairLiveDataClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=BetfairDataClientConfig(
            account_currency="GBP",
            username="username",
            password="password",
            app_key="app_key",
            subscribe_race_data=True,
        ),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Assert
    assert client._race_stream is not None
    assert client._race_stream.host == "sports-data-stream-api.betfair.com"


def test_race_stream_client_not_created_by_default(
    mocker,
    betfair_client,
    instrument_provider,
    instrument,
    venue,
    event_loop,
    msgbus,
    cache,
    clock,
):
    """
    Verify that the race stream client is not created by default.
    """
    # Arrange
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_client",
        return_value=betfair_client,
    )
    mocker.patch(
        "nautilus_trader.adapters.betfair.factories.get_cached_betfair_instrument_provider",
        return_value=instrument_provider,
    )
    instrument_provider.add(instrument)

    # Act
    client = BetfairLiveDataClientFactory.create(
        loop=event_loop,
        name=venue.value,
        config=BetfairDataClientConfig(
            account_currency="GBP",
            username="username",
            password="password",
            app_key="app_key",
        ),
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Assert
    assert client._race_stream is None


@pytest.mark.asyncio
async def test_race_stream_fatal_error_disables_race_stream(data_client):
    """
    A permanent auth error on the race stream should disconnect and disable it rather
    than entering an infinite reconnect loop.
    """
    # Arrange
    mock_stream = MagicMock()
    mock_stream.disconnect = AsyncMock()
    data_client._race_stream = mock_stream
    raw = msgspec.json.encode(
        {
            "op": "status",
            "id": 1,
            "connectionClosed": True,
            "errorCode": "NO_APP_KEY",
            "errorMessage": "AppKey is not configured for service",
            "statusCode": "FAILURE",
        },
    )

    # Act
    data_client.on_race_stream_update(raw)
    await eventually(lambda: data_client._race_stream is None)

    # Assert - stream disconnected and reference cleared
    mock_stream.disconnect.assert_called_once()
    assert not data_client._is_reconnecting


@pytest.mark.asyncio
async def test_race_stream_transient_error_renews_session_via_keep_alive(data_client):
    """
    A transient race stream closure should renew the session token via keep_alive (not
    reconnect which resets shared HTTP client state) and reconnect the race stream.
    """
    # Arrange
    mock_stream = MagicMock()
    mock_stream.reconnect = AsyncMock()
    data_client._race_stream = mock_stream
    data_client._client.keep_alive = AsyncMock()
    data_client._client.reconnect = AsyncMock()
    raw = msgspec.json.encode(
        {
            "op": "status",
            "id": 1,
            "connectionClosed": True,
            "errorCode": "NO_SESSION",
            "errorMessage": "Session expired",
            "statusCode": "FAILURE",
        },
    )

    # Act
    data_client.on_race_stream_update(raw)
    await eventually(lambda: not data_client._is_reconnecting_race_stream)

    # Assert - keep_alive used (not reconnect), race stream reconnected
    data_client._client.keep_alive.assert_called_once()
    data_client._client.reconnect.assert_not_called()
    mock_stream.reconnect.assert_called_once()


@pytest.mark.asyncio
async def test_race_stream_reconnect_falls_back_to_full_session_refresh(data_client):
    """
    When keep_alive fails during race reconnect, fall back to a full session refresh so
    the race stream can recover.
    """
    # Arrange
    mock_stream = MagicMock()
    mock_stream.reconnect = AsyncMock()
    data_client._race_stream = mock_stream
    data_client._client.keep_alive = AsyncMock(side_effect=Exception("Token invalid"))
    data_client._client.reconnect = AsyncMock()
    raw = msgspec.json.encode(
        {
            "op": "status",
            "id": 1,
            "connectionClosed": True,
            "errorCode": "NO_SESSION",
            "errorMessage": "Session expired",
            "statusCode": "FAILURE",
        },
    )

    # Act
    data_client.on_race_stream_update(raw)
    await eventually(lambda: not data_client._is_reconnecting_race_stream)

    # Assert - fell back to full reconnect, race stream recovered
    data_client._client.keep_alive.assert_called_once()
    data_client._client.reconnect.assert_called_once()
    mock_stream.reconnect.assert_called_once()


@pytest.mark.asyncio
async def test_race_stream_connect_failure_does_not_block_market_startup(race_data_client):
    """
    If the race stream fails to connect during startup, market data should still
    function normally with the race stream disabled.
    """
    # Arrange
    race_data_client._race_stream.connect = AsyncMock(
        side_effect=Exception("TPD endpoint unreachable"),
    )
    race_data_client._stream.connect = AsyncMock()
    race_data_client._client.connect = AsyncMock()
    race_data_client._instrument_provider.load_all_async = AsyncMock()

    # Act
    await race_data_client._connect()

    # Assert - market stream connected, race stream disabled
    race_data_client._stream.connect.assert_called_once()
    assert race_data_client._race_stream is None


@pytest.mark.asyncio
async def test_race_stream_max_connection_limit_is_fatal(data_client):
    """
    MAX_CONNECTION_LIMIT_EXCEEDED should disable the race stream rather than looping
    reconnects that can never succeed.
    """
    # Arrange
    mock_stream = MagicMock()
    mock_stream.disconnect = AsyncMock()
    data_client._race_stream = mock_stream
    raw = msgspec.json.encode(
        {
            "op": "status",
            "id": 1,
            "connectionClosed": True,
            "errorCode": "MAX_CONNECTION_LIMIT_EXCEEDED",
            "errorMessage": "Connection limit exceeded",
            "statusCode": "FAILURE",
        },
    )

    # Act
    data_client.on_race_stream_update(raw)
    await eventually(lambda: data_client._race_stream is None)

    # Assert
    mock_stream.disconnect.assert_called_once()
    assert not data_client._is_reconnecting_race_stream


@pytest.mark.asyncio
async def test_race_stream_duplicate_reconnect_suppressed(data_client):
    """
    Back-to-back race stream status errors should result in a single reconnect, not
    overlapping tasks.
    """
    # Arrange - make keep_alive block so the first reconnect is still in-flight
    # when the second status message arrives
    keep_alive_entered = asyncio.Event()
    keep_alive_proceed = asyncio.Event()

    async def slow_keep_alive():
        keep_alive_entered.set()
        await keep_alive_proceed.wait()

    mock_stream = MagicMock()
    mock_stream.reconnect = AsyncMock()
    data_client._race_stream = mock_stream
    data_client._client.keep_alive = slow_keep_alive
    raw = msgspec.json.encode(
        {
            "op": "status",
            "id": 1,
            "connectionClosed": True,
            "errorCode": "NO_SESSION",
            "errorMessage": "Session expired",
            "statusCode": "FAILURE",
        },
    )

    # Act - first status spawns reconnect task
    data_client.on_race_stream_update(raw)
    await keep_alive_entered.wait()
    assert data_client._is_reconnecting_race_stream

    # Second status while first reconnect is in-flight
    data_client.on_race_stream_update(raw)

    # Unblock the first reconnect
    keep_alive_proceed.set()
    await eventually(lambda: not data_client._is_reconnecting_race_stream)

    # Assert - stream reconnected exactly once
    mock_stream.reconnect.assert_called_once()


@pytest.mark.asyncio
async def test_race_reconnect_aborts_when_full_reconnect_starts_during_await(data_client):
    """
    If a full reconnect starts while _reconnect_race_stream is awaiting keep_alive, the
    race reconnect should abort and let the full reconnect handle both streams.
    """
    # Arrange - make keep_alive block so we can simulate a full reconnect starting
    keep_alive_entered = asyncio.Event()
    keep_alive_proceed = asyncio.Event()

    async def slow_keep_alive():
        keep_alive_entered.set()
        await keep_alive_proceed.wait()

    mock_stream = MagicMock()
    mock_stream.reconnect = AsyncMock()
    data_client._race_stream = mock_stream
    data_client._client.keep_alive = slow_keep_alive
    raw = msgspec.json.encode(
        {
            "op": "status",
            "id": 1,
            "connectionClosed": True,
            "errorCode": "NO_SESSION",
            "errorMessage": "Session expired",
            "statusCode": "FAILURE",
        },
    )

    # Act - race reconnect starts and blocks on keep_alive
    data_client.on_race_stream_update(raw)
    await keep_alive_entered.wait()

    # Full reconnect starts while race reconnect is mid-keep_alive
    data_client._is_reconnecting = True

    # Unblock keep_alive so race reconnect can check the flag
    keep_alive_proceed.set()
    await eventually(lambda: not data_client._is_reconnecting_race_stream)

    # Assert - race stream reconnect aborted, deferred to full reconnect
    mock_stream.reconnect.assert_not_called()


@pytest.mark.asyncio
async def test_market_reconnect_proceeds_despite_race_reconnect_in_flight(data_client):
    """
    Market stream recovery always takes priority.

    A full reconnect should proceed even when a race-only reconnect is in-flight,
    subsuming it.

    """
    # Arrange
    data_client._is_reconnecting_race_stream = True
    data_client._client.reconnect = AsyncMock()
    data_client._stream.reconnect = AsyncMock()
    raw = msgspec.json.encode(
        {
            "op": "status",
            "id": 1,
            "connectionClosed": True,
            "errorCode": "NO_SESSION",
            "errorMessage": "Session expired",
            "statusCode": "FAILURE",
        },
    )

    # Act
    data_client.on_market_update(raw)
    await eventually(lambda: data_client._client.reconnect.called)

    # Assert - full reconnect ran, race reconnect flag cleared
    assert not data_client._is_reconnecting_race_stream


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


@pytest.mark.asyncio
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
    ob = create_betfair_order_book(ob_snap.instrument_id)
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


RESOURCES_PATH = Path(__file__).parent / "resources"


def test_rcm_race_runner_data(data_client, mock_data_engine_process):
    # Arrange
    raw = (RESOURCES_PATH / "streaming" / "streaming_rcm.json").read_bytes()

    # Act
    data_client.on_race_stream_update(raw)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    custom_data = [data for data in mock_call_args if isinstance(data, CustomData)]
    runner_data = [
        data.data for data in custom_data if isinstance(data.data, BetfairRaceRunnerData)
    ]

    assert len(runner_data) == 1
    assert runner_data[0].race_id == "28587288.1650"
    assert runner_data[0].selection_id == 7390417
    assert runner_data[0].speed == 17.8
    assert runner_data[0].progress == 2051


def test_rcm_race_progress(data_client, mock_data_engine_process):
    # Arrange
    raw = (RESOURCES_PATH / "streaming" / "streaming_rcm.json").read_bytes()

    # Act
    data_client.on_race_stream_update(raw)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    custom_data = [data for data in mock_call_args if isinstance(data, CustomData)]
    progress_data = [
        data.data for data in custom_data if isinstance(data.data, BetfairRaceProgress)
    ]

    assert len(progress_data) == 1
    assert progress_data[0].race_id == "28587288.1650"
    assert progress_data[0].gate_name == "1f"
    assert progress_data[0].running_time == 46.7
    assert progress_data[0].order == [7390417, 5600338, 11527189, 6395118, 8706072]


def test_rcm_multi_runner(data_client, mock_data_engine_process):
    # Arrange
    raw = (RESOURCES_PATH / "streaming" / "streaming_rcm_multi_runner.json").read_bytes()

    # Act
    data_client.on_race_stream_update(raw)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    custom_data = [data for data in mock_call_args if isinstance(data, CustomData)]
    runner_data = [
        data.data for data in custom_data if isinstance(data.data, BetfairRaceRunnerData)
    ]

    assert len(runner_data) == 5
    assert all(r.race_id == "32908802.0000" for r in runner_data)
    assert {r.selection_id for r in runner_data} == {35467839, 24947967, 299569, 31422647, 41694785}


def test_rcm_with_jumps(data_client, mock_data_engine_process):
    # Arrange
    raw = (RESOURCES_PATH / "streaming" / "streaming_rcm_race_start.json").read_bytes()

    # Act
    data_client.on_race_stream_update(raw)

    # Assert
    mock_call_args = [call.args[0] for call in mock_data_engine_process.call_args_list]
    custom_data = [data for data in mock_call_args if isinstance(data, CustomData)]
    progress_data = [
        data.data for data in custom_data if isinstance(data.data, BetfairRaceProgress)
    ]

    assert len(progress_data) == 1
    assert progress_data[0].jumps is not None
    assert len(progress_data[0].jumps) == 9
    assert progress_data[0].jumps[0] == {"J": 9, "L": 3123.5}


def test_betfair_order_voided_dict_serialization():
    # Arrange
    instrument_id = InstrumentId.from_str("1-123456-789-None.BETFAIR")
    voided = BetfairOrderVoided(
        instrument_id=instrument_id,
        client_order_id="test-order-123",
        venue_order_id="248485109136",
        size_voided=50.0,
        price=1.50,
        size=100.0,
        side="B",
        avg_price_matched=1.50,
        size_matched=50.0,
        reason=None,
        ts_event=1635217893000000000,
        ts_init=1635217893000000001,
    )

    # Act
    result_dict = BetfairOrderVoided.to_dict(voided)
    result = BetfairOrderVoided.from_dict(result_dict)

    # Assert
    assert result.instrument_id == voided.instrument_id
    assert result.client_order_id == voided.client_order_id
    assert result.venue_order_id == voided.venue_order_id
    assert result.size_voided == voided.size_voided
    assert result.price == voided.price
    assert result.size == voided.size
    assert result.side == voided.side
    assert result.avg_price_matched == voided.avg_price_matched
    assert result.size_matched == voided.size_matched
    assert result.reason == voided.reason
    assert result.ts_event == voided.ts_event
    assert result.ts_init == voided.ts_init


def test_betfair_order_voided_dict_serialization_with_reason():
    # Arrange
    instrument_id = InstrumentId.from_str("1-123456-789-None.BETFAIR")
    voided = BetfairOrderVoided(
        instrument_id=instrument_id,
        client_order_id="test-order-123",
        venue_order_id="248485109136",
        size_voided=25.5,
        price=2.0,
        size=100.0,
        side="L",
        reason="VAR_DECISION",
        ts_event=1635217893000000000,
        ts_init=1635217893000000001,
    )

    # Act
    result_dict = BetfairOrderVoided.to_dict(voided)
    result = BetfairOrderVoided.from_dict(result_dict)

    # Assert
    assert result.reason == "VAR_DECISION"
    assert result.size_voided == 25.5


def test_betfair_order_voided_repr():
    # Arrange
    instrument_id = InstrumentId.from_str("1-123456-789-None.BETFAIR")
    voided = BetfairOrderVoided(
        instrument_id=instrument_id,
        client_order_id="test-order-123",
        venue_order_id="248485109136",
        size_voided=50.0,
        price=1.50,
        size=100.0,
        side="B",
        reason=None,
        ts_event=1635217893000000000,
        ts_init=1635217893000000001,
    )

    # Act
    result = repr(voided)

    # Assert
    assert "BetfairOrderVoided" in result
    assert "1-123456-789-None.BETFAIR" in result
    assert "test-order-123" in result
    assert "248485109136" in result
    assert "50.0" in result


def test_betfair_order_voided_equality():
    # Arrange
    instrument_id = InstrumentId.from_str("1-123456-789-None.BETFAIR")
    voided1 = BetfairOrderVoided(
        instrument_id=instrument_id,
        client_order_id="test-order-123",
        venue_order_id="248485109136",
        size_voided=50.0,
        price=1.50,
        size=100.0,
        side="B",
        reason=None,
        ts_event=1635217893000000000,
        ts_init=1635217893000000001,
    )
    voided2 = BetfairOrderVoided(
        instrument_id=instrument_id,
        client_order_id="test-order-123",
        venue_order_id="248485109136",
        size_voided=25.0,
        price=2.0,
        size=100.0,
        side="L",
        reason="DIFFERENT",
        ts_event=1635217893000000002,
        ts_init=1635217893000000003,
    )
    voided3 = BetfairOrderVoided(
        instrument_id=instrument_id,
        client_order_id="different-order",
        venue_order_id="248485109136",
        size_voided=50.0,
        price=1.50,
        size=100.0,
        side="B",
        reason=None,
        ts_event=1635217893000000000,
        ts_init=1635217893000000001,
    )

    # Assert
    assert voided1 == voided2  # Same instrument_id, client_order_id, venue_order_id
    assert voided1 != voided3  # Different client_order_id
    assert voided1 != None  # noqa: E711
    assert voided1 != "not a voided object"


def test_rcm_runner_data_reaches_subscriber_via_data_engine(data_client, data_engine, msgbus):
    """
    Verify that an actor subscribing with DataType(BetfairRaceRunnerData,
    {"selection_id": N}) receives runner data through the full pipeline.
    """
    # Arrange
    received = []
    data_type = DataType(BetfairRaceRunnerData, {"selection_id": 49411491})
    topic = f"data.{data_type.topic}"
    msgbus.subscribe(topic=topic, handler=received.append)

    raw = (RESOURCES_PATH / "streaming" / "streaming_rcm_tpd_live.jsonl").read_bytes()
    lines = raw.splitlines()

    # Act - first message has 3 runners including selection_id 49411491
    data_client.on_race_stream_update(lines[0])

    # Assert
    assert len(received) == 1
    assert isinstance(received[0], BetfairRaceRunnerData)
    assert received[0].selection_id == 49411491
    assert received[0].race_id == "35270435.1830"
    assert received[0].latitude == 52.6056631
    assert received[0].longitude == -2.145348
    assert received[0].speed == 1.73


def test_rcm_runner_data_filtered_by_selection_id(data_client, data_engine, msgbus):
    """
    Verify that subscribing to a specific selection_id only receives data for that
    runner, not others in the same RCM message.
    """
    # Arrange
    received_target = []
    received_other = []
    topic_target = f"data.{DataType(BetfairRaceRunnerData, {'selection_id': 44169412}).topic}"
    topic_other = f"data.{DataType(BetfairRaceRunnerData, {'selection_id': 19080425}).topic}"
    msgbus.subscribe(topic=topic_target, handler=received_target.append)
    msgbus.subscribe(topic=topic_other, handler=received_other.append)

    raw = (RESOURCES_PATH / "streaming" / "streaming_rcm_tpd_live.jsonl").read_bytes()
    lines = raw.splitlines()

    # Act - first message has runners 49411491, 44169412, 19080425
    data_client.on_race_stream_update(lines[0])

    # Assert - each subscriber only gets their runner
    assert len(received_target) == 1
    assert received_target[0].selection_id == 44169412
    assert received_target[0].speed == 1.32

    assert len(received_other) == 1
    assert received_other[0].selection_id == 19080425
    assert received_other[0].speed == 1.45


def test_rcm_progress_data_reaches_subscriber_via_data_engine(data_client, data_engine, msgbus):
    """
    Verify that an actor subscribing with DataType(BetfairRaceProgress) receives race
    progress data through the full pipeline.
    """
    # Arrange
    received = []
    data_type = DataType(BetfairRaceProgress)
    topic = f"data.{data_type.topic}"
    msgbus.subscribe(topic=topic, handler=received.append)

    raw = (RESOURCES_PATH / "streaming" / "streaming_rcm_tpd_live.jsonl").read_bytes()
    lines = raw.splitlines()

    # Act - second message has race progress data
    data_client.on_race_stream_update(lines[1])

    # Assert
    assert len(received) == 1
    assert isinstance(received[0], BetfairRaceProgress)
    assert received[0].race_id == "35270435.1830"
    assert received[0].market_id == "1.254088503"
    assert received[0].progress == 1025


def test_rcm_sequence_delivers_all_updates(data_client, data_engine, msgbus):
    """
    Verify that processing a sequence of RCM messages delivers all runner and progress
    updates to their respective subscribers.
    """
    # Arrange
    runner_data = []
    progress_data = []
    msgbus.subscribe(topic="data.BetfairRaceRunnerData*", handler=runner_data.append)
    msgbus.subscribe(
        topic=f"data.{DataType(BetfairRaceProgress).topic}",
        handler=progress_data.append,
    )

    raw = (RESOURCES_PATH / "streaming" / "streaming_rcm_tpd_live.jsonl").read_bytes()

    # Act - process all 3 messages
    for line in raw.splitlines():
        data_client.on_race_stream_update(line)

    # Assert - 3 runners from msg 0, 1 runner from msg 2, 1 progress from msg 1
    assert len(runner_data) == 4
    assert all(isinstance(d, BetfairRaceRunnerData) for d in runner_data)

    assert len(progress_data) == 1
    assert isinstance(progress_data[0], BetfairRaceProgress)
