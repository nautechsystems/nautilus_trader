# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
import os

import betfairlightweight
import orjson
import pytest

from nautilus_trader.adapters.betfair.common import BETFAIR_VENUE
from nautilus_trader.adapters.betfair.data import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.data import InstrumentSearch
from nautilus_trader.adapters.betfair.data import on_market_update
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import DeltaType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.events import InstrumentClosePrice
from nautilus_trader.model.events import InstrumentStatusEvent
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.tick import TradeTick
from tests.integration_tests.adapters.betfair.test_kit import BetfairTestStubs


@pytest.mark.asyncio
@pytest.mark.skip  # Only runs locally, comment to run
async def test_betfair_data_client(betfair_data_client, data_engine):
    """
    Local test only, ensure we can connect to betfair and receive some market data
    """
    betfair_client = betfairlightweight.APIClient(
        username=os.environ["BETFAIR_USERNAME"],
        password=os.environ["BETFAIR_PW"],
        app_key=os.environ["BETFAIR_APP_KEY"],
        certs=os.environ["BETFAIR_CERT_DIR"],
    )
    betfair_client.login()

    def printer(x):
        print(x)

    # TODO - mock betfairlightweight.APIClient.login won't let this pass, need to comment out to run
    socket = BetfairMarketStreamClient(client=betfair_client, message_handler=printer)
    await socket.connect()
    await socket.send_subscription_message(market_ids=["1.180634014"])
    await socket.start()


def test_individual_market_subscriptions():
    # TODO - Subscribe to a couple of markets individually
    pass


def test_market_heartbeat(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_HEARTBEAT())


def test_market_sub_image_market_def(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_SUB_IMAGE())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 7
    assert result == expected
    # Check prices are probabilities
    result = set(
        float(order[0])
        for ob_snap in data_engine.events
        for order in ob_snap.bids + ob_snap.asks
    )
    expected = set(
        [
            0.02174,
            0.39370,
            0.36765,
            0.21739,
            0.00102,
            0.17241,
            0.00102,
            0.55556,
            0.45872,
            0.21739,
            0.00769,
            0.02381,
        ]
    )
    assert result == expected


def test_market_sub_image_no_market_def(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(
        BetfairTestStubs.streaming_mcm_SUB_IMAGE_no_market_def()
    )
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 270
    assert result == expected


def test_market_resub_delta(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_RESUB_DELTA())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookDeltas"] * 269
    assert result == expected


def test_market_update(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_UPDATE())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookDeltas"] * 1
    assert result == expected
    result = [op.type for op in data_engine.events[0].deltas]
    expected = [DeltaType.UPDATE, DeltaType.DELETE]
    assert result == expected
    # Ensure order prices are coming through as probability
    update_op = data_engine.events[0].deltas[0]
    assert update_op.order.price == 0.21277


# TODO - waiting for market status implementation
@pytest.mark.skip
def test_market_update_md(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_UPDATE_md())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["OrderBookSnapshot"] * 7
    assert result == expected


@pytest.mark.skip  # We don't do anything with traded volume at this stage
def test_market_update_tv(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_UPDATE_tv())
    result = [type(event).__name__ for event in data_engine.events]
    expected = [] * 7
    assert result == expected


def test_market_update_live_image(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_live_IMAGE())
    result = [type(event).__name__ for event in data_engine.events]
    expected = (
        ["OrderBookSnapshot"]
        + ["TradeTick"] * 13
        + ["OrderBookSnapshot"]
        + ["TradeTick"] * 17
    )
    assert result == expected


def test_market_update_live_update(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_live_UPDATE())
    result = [type(event).__name__ for event in data_engine.events]
    expected = ["TradeTick", "OrderBookDeltas"]
    assert result == expected


@pytest.mark.asyncio
async def test_request_search_instruments(betfair_data_client, data_engine, uuid):
    req = DataType(
        data_type=InstrumentSearch,
        metadata={"event_type_id": "7"},
    )
    betfair_data_client.request(req, uuid)
    await asyncio.sleep(0)
    resp = data_engine.responses[0]
    assert len(resp.data.instruments) == 9383


def test_orderbook_repr(betfair_data_client, data_engine):
    betfair_data_client._on_market_update(BetfairTestStubs.streaming_mcm_live_IMAGE())
    ob_snap = data_engine.events[14]
    ob = L2OrderBook(InstrumentId(Symbol("1"), BETFAIR_VENUE), 5, 5)
    ob.apply_snapshot(ob_snap)
    print(ob.pprint())
    assert ob.best_ask_price() == 0.58824
    assert ob.best_bid_price() == 0.58480


def test_orderbook_updates(betfair_data_client):
    order_books = {}
    for raw in BetfairTestStubs.streaming_market_updates():
        for update in on_market_update(
            update=orjson.loads(raw),
            instrument_provider=betfair_data_client.instrument_provider(),
        ):
            if len(order_books) > 1 and update.instrument_id != list(order_books)[1]:
                continue
            print(update)
            if isinstance(update, OrderBookSnapshot):
                order_books[update.instrument_id] = L2OrderBook(
                    instrument_id=update.instrument_id,
                    price_precision=4,
                    size_precision=4,
                )
                order_books[update.instrument_id].apply_snapshot(update)
            elif isinstance(update, OrderBookDeltas):
                order_books[update.instrument_id].apply_deltas(update)
            elif isinstance(update, TradeTick):
                pass
            else:
                raise KeyError

    book = order_books[list(order_books)[0]]
    assert (
        book.pprint()
        == """bids       price   asks
--------  -------  ---------
          0.8621   [932.64]
          0.8547   [1275.83]
          0.8475   [151.96]
[147.79]  0.8403
[156.74]  0.8333
[11.19]   0.8197"""
    )


def test_instrument_opening_events(betfair_data_client, data_engine):
    updates = BetfairTestStubs.raw_market_updates()
    messages = on_market_update(
        instrument_provider=betfair_data_client.instrument_provider(), update=updates[0]
    )
    assert len(messages) == 2
    assert (
        isinstance(messages[0], InstrumentStatusEvent)
        and messages[0].status == InstrumentStatus.PRE_OPEN
    )
    assert (
        isinstance(messages[1], InstrumentStatusEvent)
        and messages[0].status == InstrumentStatus.PRE_OPEN
    )


def test_instrument_in_play_events(betfair_data_client, data_engine):
    events = [
        msg
        for update in BetfairTestStubs.raw_market_updates()
        for msg in on_market_update(
            instrument_provider=betfair_data_client.instrument_provider(), update=update
        )
        if isinstance(msg, InstrumentStatusEvent)
    ]
    assert len(events) == 14
    result = [ev.status for ev in events]
    expected = [
        InstrumentStatus.PRE_OPEN.value,
        InstrumentStatus.PRE_OPEN.value,
        InstrumentStatus.PRE_OPEN.value,
        InstrumentStatus.PRE_OPEN.value,
        InstrumentStatus.PRE_OPEN.value,
        InstrumentStatus.PRE_OPEN.value,
        InstrumentStatus.PAUSE.value,
        InstrumentStatus.PAUSE.value,
        InstrumentStatus.OPEN.value,
        InstrumentStatus.OPEN.value,
        InstrumentStatus.PAUSE.value,
        InstrumentStatus.PAUSE.value,
        InstrumentStatus.CLOSED.value,
        InstrumentStatus.CLOSED.value,
    ]
    assert result == expected


def test_instrument_closing_events(data_engine, betfair_data_client):
    updates = BetfairTestStubs.raw_market_updates()
    messages = on_market_update(
        instrument_provider=betfair_data_client.instrument_provider(),
        update=updates[-1],
    )
    assert len(messages) == 4
    assert (
        isinstance(messages[0], InstrumentStatusEvent)
        and messages[0].status == InstrumentStatus.CLOSED
    )
    assert (
        isinstance(messages[1], InstrumentClosePrice)
        and messages[1].close_price == 1.0000
    )
    assert (
        isinstance(messages[1], InstrumentClosePrice)
        and messages[1].close_type == InstrumentCloseType.EXPIRED
    )
    assert (
        isinstance(messages[2], InstrumentStatusEvent)
        and messages[2].status == InstrumentStatus.CLOSED
    )
    assert (
        isinstance(messages[3], InstrumentClosePrice) and messages[3].close_price == 0.0
    )
    assert (
        isinstance(messages[3], InstrumentClosePrice)
        and messages[3].close_type == InstrumentCloseType.EXPIRED
    )


#  TODO - Awaiting a response from betfair
@pytest.mark.skip
def test_duplicate_trades(betfair_data_client):
    messages = []
    for update in BetfairTestStubs.raw_market_updates(
        market="1.180305278", runner1="2696769", runner2="4297085"
    ):
        messages.extend(
            on_market_update(
                instrument_provider=betfair_data_client.instrument_provider(),
                update=update,
            )
        )
        if update["pt"] >= 1615222877785:
            break
    trades = [
        m
        for m in messages
        if isinstance(m, TradeTick) and m.price == Price.from_str("0.69930")
    ]
    assert len(trades) == 5
