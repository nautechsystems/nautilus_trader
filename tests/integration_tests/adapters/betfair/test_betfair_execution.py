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
from functools import partial
from typing import Optional
from unittest.mock import MagicMock
from unittest.mock import patch

import msgspec
import pytest
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import STREAM_DECODER
from betfair_parser.spec.streaming.ocm import MatchedOrder

from nautilus_trader.adapters.betfair.common import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.common import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id
from nautilus_trader.core.rust.model import OrderSide
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderPendingUpdate
from nautilus_trader.model.events.order import OrderRejected
from nautilus_trader.model.events.order import OrderSubmitted
from nautilus_trader.model.events.order import OrderUpdated
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import format_current_orders
from tests.integration_tests.adapters.betfair.test_kit import mock_betfair_request


async def _setup_order_state(
    order_change_message,
    exec_client,
    cache,
    strategy,
    trade_id,
    include_fills: bool = False,
):
    """
    Ready the engine to test a message from betfair, setting orders into the correct state
    """
    if isinstance(order_change_message, bytes):
        order_change_message = STREAM_DECODER.decode(order_change_message)
    for oc in order_change_message.oc:
        for orc in oc.orc:
            for order_update in orc.uo:
                instrument_id = betfair_instrument_id(
                    market_id=oc.id,
                    selection_id=str(orc.id),
                    selection_handicap=str(orc.hc),
                )
                order_id = order_update.id
                venue_order_id = VenueOrderId(order_id)
                client_order_id = ClientOrderId(order_id)
                if not cache.instrument(instrument_id):
                    instrument = TestInstrumentProvider.betting_instrument(
                        market_id=oc.id,
                        selection_id=str(orc.id),
                        selection_handicap=str(orc.hc),
                    )
                    cache.add_instrument(instrument)
                if not cache.order(client_order_id):
                    assert strategy is not None, "strategy can't be none if accepting order"
                    order = TestExecStubs.limit_order(
                        instrument_id=instrument_id,
                        price=betfair_float_to_price(order_update.p),
                        client_order_id=client_order_id,
                    )
                    exec_client.venue_order_id_to_client_order_id[venue_order_id] = client_order_id
                    await _accept_order(order, venue_order_id, exec_client, strategy, cache)

                    if include_fills and order_update.sm:
                        await _fill_order(
                            order,
                            exec_client=exec_client,
                            fill_price=order_update.avp or order_update.p,
                            fill_qty=order_update.sm,
                            venue_order_id=venue_order_id,
                            trade_id=trade_id,
                            quote_currency=GBP,
                        )


@pytest.fixture()
def setup_order_state(exec_client, cache, strategy, trade_id, venue_order_id):
    return partial(
        _setup_order_state,
        exec_client=exec_client,
        cache=cache,
        strategy=strategy,
        trade_id=trade_id,
        include_fills=False,
    )


@pytest.fixture()
def setup_order_state_fills(setup_order_state):
    return partial(setup_order_state, include_fills=True)


async def _submit_order(order, exec_client, strategy, cache):
    # We don't want the execution client to actually do anything here
    exec_client.submit_order = MagicMock()
    strategy.submit_order(order)
    await asyncio.sleep(0)
    assert cache.order(order.client_order_id)
    return order


@pytest.fixture()
def submit_order(exec_client, strategy, cache):
    return partial(_submit_order, exec_client=exec_client, strategy=strategy, cache=cache)


async def _fill_order(
    order,
    exec_client,
    fill_price: float,
    fill_qty: float,
    venue_order_id,
    trade_id,
    quote_currency,
):
    exec_client.generate_order_filled(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=venue_order_id,
        trade_id=trade_id or TradeId("1"),
        venue_position_id=None,
        order_side=order.side,
        order_type=order.order_type,
        last_qty=betfair_float_to_quantity(fill_qty),
        last_px=betfair_float_to_price(fill_price),
        quote_currency=quote_currency,
        commission=Money.from_str(f"0 {quote_currency.code}"),
        liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
        ts_event=0,
    )
    await asyncio.sleep(0)
    return order


async def _accept_order(order, venue_order_id: VenueOrderId, exec_client, strategy, cache):
    await _submit_order(order, exec_client=exec_client, strategy=strategy, cache=cache)
    exec_client.generate_order_accepted(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=venue_order_id or order.venue_order_id,
        ts_event=0,
    )
    await asyncio.sleep(0)
    return order


@pytest.fixture()
def accept_order(exec_client, strategy, cache):
    return partial(_accept_order, exec_client=exec_client, strategy=strategy, cache=cache)


@pytest.fixture()
def fill_order(
    exec_client,
    venue_order_id: VenueOrderId,
    quote_currency: Currency,
    trade_id: Optional[str] = None,
):
    return partial(
        _fill_order,
        exec_client=exec_client,
        venue_order_id=venue_order_id,
        quote_currency=quote_currency,
        trade_id=trade_id,
    )


@pytest.fixture()
def test_order(instrument, strategy_id):
    return TestExecStubs.limit_order(
        instrument_id=instrument.id,
        price=betfair_float_to_price(2.0),
        quantity=Quantity.from_str("100"),
        strategy_id=strategy_id,
    )


@pytest.mark.asyncio()
async def test_submit_order_success(betfair_client, strategy, test_order):
    # Arrange
    mock_betfair_request(betfair_client, BetfairResponses.betting_place_order_success())

    # Act
    strategy.submit_order(test_order)
    await asyncio.sleep(0)

    # Assert
    _, submitted, accepted = test_order.events
    assert isinstance(submitted, OrderSubmitted)
    assert isinstance(accepted, OrderAccepted)
    assert accepted.venue_order_id == VenueOrderId("228302937743")


@pytest.mark.asyncio()
async def test_submit_order_error(betfair_client, strategy, test_order, messages):
    # Arrange
    mock_betfair_request(betfair_client, BetfairResponses.betting_place_order_error())

    # Act
    strategy.submit_order(test_order)
    await asyncio.sleep(0)

    # Assert
    _, submitted, rejected = test_order.events
    assert isinstance(submitted, OrderSubmitted)
    assert isinstance(rejected, OrderRejected)
    assert rejected.reason == "PERMISSION_DENIED: ERROR_IN_ORDER"


@pytest.mark.asyncio()
async def test_modify_order_success(
    betfair_client,
    strategy,
    venue_order_id,
    accept_order,
    test_order,
    events,
):
    # Arrange
    mock_betfair_request(betfair_client, BetfairResponses.betting_replace_orders_success())
    await accept_order(test_order, venue_order_id=venue_order_id)

    # Act
    strategy.modify_order(test_order, price=betfair_float_to_price(2.5))
    await asyncio.sleep(0)

    # Assert
    pending_update, _, updated = events[-3:]
    assert isinstance(pending_update, OrderPendingUpdate)
    assert isinstance(updated, OrderUpdated)
    assert updated.price == betfair_float_to_price(50)


@pytest.mark.asyncio()
async def test_modify_order_error_order_doesnt_exist(betfair_client, exec_client, test_order):
    # Arrange
    command = TestCommandStubs.modify_order_command(
        order=test_order,
        price=betfair_float_to_price(10),
    )
    # Act
    with patch.object(BetfairExecutionClient, "generate_order_modify_rejected") as mock_reject:
        exec_client.modify_order(command)
        await asyncio.sleep(0)

    # Assert
    expected_kw = {
        "strategy_id": StrategyId("S-001"),
        "instrument_id": InstrumentId.from_str("1.179082386|50214|None.BETFAIR"),
        "client_order_id": ClientOrderId("O-20210410-022422-001-001-1"),
        "venue_order_id": None,
        "reason": "ORDER NOT IN CACHE",
        "ts_event": 0,
    }
    assert mock_reject.call_args.kwargs == expected_kw


@pytest.mark.asyncio()
async def test_modify_order_error_no_venue_id(
    betfair_client,
    exec_client,
    strategy_id,
    submit_order,
    test_order,
    instrument,
):
    # Arrange
    order = await submit_order(test_order)
    mock_betfair_request(betfair_client, BetfairResponses.betting_replace_orders_success())

    # Act
    command = TestCommandStubs.modify_order_command(price=betfair_float_to_price(2.0), order=order)
    with patch.object(BetfairExecutionClient, "generate_order_modify_rejected") as mock_reject:
        exec_client.modify_order(command)
        await asyncio.sleep(0)

    # Assert
    expected_kw = {
        "strategy_id": strategy_id,
        "instrument_id": instrument.id,
        "client_order_id": test_order.client_order_id,
        "venue_order_id": None,
        "reason": "ORDER MISSING VENUE_ORDER_ID",
        "ts_event": 0,
    }
    assert mock_reject.call_args.kwargs == expected_kw


@pytest.mark.asyncio()
async def test_cancel_order_success(
    betfair_client,
    exec_client,
    accept_order,
    test_order,
    venue_order_id,
    strategy_id,
    instrument,
):
    # Arrange
    order = await accept_order(order=test_order, venue_order_id=venue_order_id)
    mock_betfair_request(betfair_client, BetfairResponses.betting_cancel_orders_success())

    # Act
    with patch.object(
        BetfairExecutionClient,
        "generate_order_canceled",
    ) as mock_generate_order_canceled:
        command = TestCommandStubs.cancel_order_command(order=order)
        exec_client.cancel_order(command)
        await asyncio.sleep(0)

    # Assert
    expected_kw = {
        "strategy_id": strategy_id,
        "instrument_id": instrument.id,
        "client_order_id": test_order.client_order_id,
        "venue_order_id": venue_order_id,
        "ts_event": 0,
    }
    assert mock_generate_order_canceled.call_args.kwargs == expected_kw


@pytest.mark.asyncio()
async def test_cancel_order_fail(
    betfair_client,
    exec_client,
    venue_order_id,
    strategy_id,
    venue,
    test_order,
    instrument,
    accept_order,
):
    # Arrange
    order = await accept_order(order=test_order, venue_order_id=venue_order_id)
    mock_betfair_request(betfair_client, BetfairResponses.betting_cancel_orders_error())

    # Act
    command = TestCommandStubs.cancel_order_command(
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=venue_order_id,
    )

    with patch.object(
        BetfairExecutionClient,
        "generate_order_cancel_rejected",
    ) as mock_generate_order_cancel_rejected:
        exec_client.cancel_order(command)
        await asyncio.sleep(0)

    # Assert
    expected_kw = {
        "strategy_id": strategy_id,
        "instrument_id": instrument.id,
        "client_order_id": test_order.client_order_id,
        "venue_order_id": venue_order_id,
        "reason": "Error: ERROR_IN_ORDER",
        "ts_event": 0,
    }
    assert mock_generate_order_cancel_rejected.call_args.kwargs == expected_kw


@pytest.mark.asyncio()
async def test_order_multiple_fills(exec_client, setup_order_state, events):
    # Arrange
    for ocm in BetfairStreaming.ocm_multiple_fills():
        await setup_order_state(order_change_message=ocm)

    # Act
    for order_change_message in BetfairStreaming.ocm_multiple_fills():
        exec_client.handle_order_stream_update(order_change_message)
        await asyncio.sleep(0.0)

    # Assert
    result = [fill.last_qty for fill in events if isinstance(fill, OrderFilled)]
    expected = [
        betfair_float_to_quantity(16.1900),
        betfair_float_to_quantity(0.77),
        betfair_float_to_quantity(0.77),
    ]
    assert result == expected


@pytest.mark.asyncio()
async def test_connection_account_state(exec_client, cache, account_id):
    # Arrange, Act
    await exec_client.connection_account_state()

    # Assert
    assert cache.account(account_id)


@pytest.mark.asyncio()
async def test_check_account_currency(exec_client):
    # Arrange, Act, Assert
    await exec_client.check_account_currency()


@pytest.mark.asyncio()
async def test_order_stream_full_image(exec_client, setup_order_state, events):
    # Arrange
    raw = BetfairStreaming.ocm_FULL_IMAGE()
    ocm = STREAM_DECODER.decode(raw)
    await setup_order_state(ocm, include_fills=True)
    exec_client._check_order_update = MagicMock()

    # Act
    exec_client.handle_order_stream_update(
        BetfairStreaming.ocm_FULL_IMAGE(),
    )
    await asyncio.sleep(0)

    # Assert
    fills = [event for event in events if isinstance(event, OrderFilled)]
    assert len(fills) == 4


@pytest.mark.asyncio()
async def test_order_stream_empty_image(exec_client, events):
    # Arrange
    order_change_message = BetfairStreaming.ocm_EMPTY_IMAGE()

    # Act
    exec_client.handle_order_stream_update(
        order_change_message,
    )
    await asyncio.sleep(0)

    # Assert
    assert len(events) == 0


@pytest.mark.asyncio()
async def test_order_stream_new_full_image(exec_client, setup_order_state, cache, events):
    # Arrange
    raw = BetfairStreaming.ocm_NEW_FULL_IMAGE()
    ocm = msgspec.json.decode(raw, type=OCM)
    await setup_order_state(ocm)
    order = cache.orders()[0]
    exec_client.generate_order_filled(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=order.venue_order_id,
        trade_id=TradeId("1"),
        venue_position_id=None,
        order_side=order.side,
        order_type=order.order_type,
        last_px=betfair_float_to_price(12.0),
        last_qty=betfair_float_to_quantity(4.75),
        quote_currency=GBP,
        commission=Money.from_str("0 GBP"),
        liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
        ts_event=0,
    )

    # Act
    exec_client.handle_order_stream_update(raw)
    await asyncio.sleep(0)
    assert len(events) == 6


@pytest.mark.asyncio()
async def test_order_stream_sub_image(exec_client, setup_order_state, events):
    # Arrange
    order_change_message = BetfairStreaming.ocm_SUB_IMAGE()
    await setup_order_state(order_change_message=order_change_message)

    # Act
    exec_client.handle_order_stream_update(
        order_change_message,
    )
    await asyncio.sleep(0)

    # Assert
    assert len(events) == 0


@pytest.mark.asyncio()
async def test_order_stream_update(exec_client, setup_order_state, events):
    # Arrange
    order_change_message = BetfairStreaming.ocm_UPDATE()
    await setup_order_state(order_change_message=order_change_message)

    # Act
    exec_client.handle_order_stream_update(
        order_change_message,
    )
    await asyncio.sleep(0)

    # Assert
    assert len(events) == 5


@pytest.mark.asyncio()
async def test_order_stream_filled(exec_client, setup_order_state, events, fill_events) -> None:
    # Arrange
    order_change_message = BetfairStreaming.ocm_FILLED()
    await setup_order_state(order_change_message=order_change_message)

    # Act
    exec_client.handle_order_stream_update(
        order_change_message,
    )
    await asyncio.sleep(0)

    # Assert
    assert len(events) == 6
    fill: OrderFilled = fill_events[0]
    assert isinstance(fill, OrderFilled)
    assert fill.last_px == betfair_float_to_price(1.10)


@pytest.mark.asyncio()
async def test_order_stream_filled_multiple_prices(
    exec_client,
    setup_order_state,
    cache,
    venue_order_id,
    events,
):
    # Arrange
    order_change_message = BetfairStreaming.generate_order_change_message(
        price=1.50,
        size=20,
        side="B",
        status="E",
        sm=10,
        avp=1.60,
        order_id=venue_order_id.value,
    )
    await setup_order_state(order_change_message)
    exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
    await asyncio.sleep(0)
    order = cache.order(client_order_id=ClientOrderId("1"))
    assert order

    # Act
    order_change_message = BetfairStreaming.generate_order_change_message(
        price=1.50,
        size=20,
        side="B",
        status="EC",
        sm=20,
        avp=1.50,
    )
    await setup_order_state(order_change_message)
    exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
    await asyncio.sleep(0)

    # Assert
    assert len(events) == 12
    fill1, fill2 = (event for event in events if isinstance(event, OrderFilled))
    assert isinstance(fill1, OrderFilled)
    assert isinstance(fill2, OrderFilled)
    assert fill1.last_px == betfair_float_to_price(1.60)
    assert fill2.last_px == betfair_float_to_price(1.50)


@pytest.mark.asyncio()
async def test_order_stream_mixed(exec_client, setup_order_state, fill_events, cancel_events):
    # Arrange
    order_change_message = BetfairStreaming.ocm_MIXED()
    await setup_order_state(order_change_message=order_change_message)

    # Act
    exec_client.handle_order_stream_update(
        order_change_message,
    )
    await asyncio.sleep(0)

    # Assert
    fill1, fill2 = fill_events
    (cancel,) = cancel_events
    assert isinstance(fill1, OrderFilled)
    assert fill1.venue_order_id.value == "229430281341"
    assert isinstance(fill2, OrderFilled)
    assert fill2.venue_order_id.value == "229430281339"
    assert isinstance(cancel, OrderCanceled)
    assert cancel.venue_order_id.value == "229430281339"


@pytest.mark.asyncio()
async def test_duplicate_trade_id(exec_client, setup_order_state, fill_events, cancel_events):
    # Arrange
    for update in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
        await setup_order_state(update)

    # Act
    for order_change_message in BetfairStreaming.ocm_DUPLICATE_EXECUTION():
        exec_client.handle_order_stream_update(order_change_message)
        await asyncio.sleep(0)

    # Assert
    fill1, fill2, fill3 = fill_events
    (cancel,) = cancel_events
    # First order example, partial fill followed by remainder canceled
    assert isinstance(fill1, OrderFilled)
    assert isinstance(cancel, OrderCanceled)
    # Second order example, partial fill followed by remainder filled
    assert (
        isinstance(fill2, OrderFilled)
        and fill2.trade_id.value == "c18af83bb4ca0ac45000fa380a2a5887a1bf3e75"
    )
    assert (
        isinstance(fill3, OrderFilled)
        and fill3.trade_id.value == "561879891c1645e8627cf97ed825d16e43196408"
    )


@pytest.mark.parametrize(
    ("side", "price", "quantity", "free"),
    [
        (OrderSide.BUY, Price.from_str("2.0"), Quantity.from_str("100"), 900),
        (OrderSide.BUY, Price.from_str("5.0"), Quantity.from_str("50"), 950),
        (OrderSide.SELL, Price.from_str("1.2"), Quantity.from_str("100"), 980),
        (OrderSide.SELL, Price.from_str("5.0"), Quantity.from_str("100"), 600),
    ],
)
@pytest.mark.asyncio()
async def test_betfair_back_order_reduces_balance(
    side,
    price,
    quantity,
    free,
    exec_client,
    betfair_client,
    cache,
    account,
    venue,
    accept_order,
    test_order,
    instrument,
    venue_order_id,
    strategy_id,
):
    # Arrange
    order = TestExecStubs.limit_order(
        instrument_id=instrument.id,
        order_side=side,
        price=price,
        quantity=quantity,
        strategy_id=strategy_id,
    )
    balance_pre_order = account.balances()[GBP]

    # Act - Send order
    await accept_order(order, venue_order_id)
    await asyncio.sleep(0)
    balance_order = cache.account_for_venue(venue).balances()[GBP]

    # Act - Cancel the order, balance should return
    command = TestCommandStubs.cancel_order_command(
        instrument_id=instrument.id,
        client_order_id=order.client_order_id,
        venue_order_id=order.venue_order_id,
    )
    mock_betfair_request(betfair_client, BetfairResponses.betting_cancel_orders_success())
    exec_client.cancel_order(command)
    await asyncio.sleep(0)
    balance_cancel = account.balances()[GBP]
    await asyncio.sleep(0)

    # Assert
    assert balance_pre_order.free == Money(1000.0, GBP)
    assert balance_order.free == Money(free, GBP)
    assert balance_cancel.free == Money(1000.0, GBP)


@pytest.mark.asyncio()
async def test_betfair_order_cancelled_no_timestamp(
    exec_client,
    setup_order_state,
    clock,
    cancel_events,
):
    # Arrange
    update = STREAM_DECODER.decode(BetfairStreaming.ocm_error_fill())
    await setup_order_state(update)
    clock.set_time(1)

    # Act
    for unmatched_order in update.oc[0].orc[0].uo:
        exec_client._handle_stream_execution_complete_order_update(
            unmatched_order=unmatched_order,
        )
        await asyncio.sleep(0)

    # Assert
    cancel1, cancel2 = cancel_events
    assert isinstance(cancel1, OrderCanceled)
    assert isinstance(cancel2, OrderCanceled)
    assert cancel1.ts_init == 1
    assert cancel2.ts_init == 1


@pytest.mark.asyncio()
@pytest.mark.parametrize(
    ("price", "size", "side", "status", "updates", "last_qtys"),
    [
        (1.50, 50, "B", "EC", [{"sm": 50}], (50,)),
        (1.50, 50, "B", "E", [{"sm": 10}, {"sm": 15}], (10, 5)),
    ],
)
async def test_various_betfair_order_fill_scenarios(
    setup_order_state,
    price,
    size,
    side,
    status,
    updates,
    last_qtys,
    exec_client,
    fill_events,
):
    # Arrange
    update = BetfairStreaming.ocm_filled_different_price()
    await setup_order_state(update)

    # Act
    for raw in updates:
        order_change_message = BetfairStreaming.generate_order_change_message(
            price=price, size=size, side=side, status=status, **raw
        )
        exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
        await asyncio.sleep(0)

    # Assert
    for msg, _, last_qty in zip(fill_events, updates, last_qtys):
        assert isinstance(msg, OrderFilled)
        assert msg.last_qty == last_qty


@pytest.mark.asyncio()
async def test_order_filled_avp_update(exec_client, setup_order_state):
    # Arrange
    update = BetfairStreaming.ocm_filled_different_price()
    await setup_order_state(update)

    # Act
    order_change_message = BetfairStreaming.generate_order_change_message(
        price=1.50,
        size=20,
        side="B",
        status="E",
        avp=1.50,
        sm=10,
    )
    exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
    await asyncio.sleep(0)

    order_change_message = BetfairStreaming.generate_order_change_message(
        price=1.30,
        size=20,
        side="B",
        status="E",
        avp=1.50,
        sm=10,
    )
    exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
    await asyncio.sleep(0)


@pytest.mark.asyncio()
async def test_generate_order_status_report_client_id(
    mocker,
    exec_client,
    betfair_client,
    instrument_provider,
) -> None:
    # Arrange
    order_resp = format_current_orders()
    instrument_provider.add(
        TestInstrumentProvider.betting_instrument(
            market_id=str(order_resp[0]["marketId"]),
            selection_id=str(order_resp[0]["selectionId"]),
            selection_handicap=str(order_resp[0]["handicap"]),
        ),
    )
    venue_order_id = VenueOrderId("1")

    mocker.patch.object(betfair_client, "list_current_orders", return_value=order_resp)

    # Act
    report: OrderStatusReport = await exec_client.generate_order_status_report(
        venue_order_id=venue_order_id,
        client_order_id=None,
        instrument_id=None,
    )

    # Assert
    assert report.order_status == OrderStatus.ACCEPTED
    assert report.price == Price(5.0, BETFAIR_PRICE_PRECISION)
    assert report.quantity == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)
    assert report.filled_qty == Quantity(0.0, BETFAIR_QUANTITY_PRECISION)


def test_check_cache_against_order_image(exec_client, venue_order_id):
    # Arrange
    ocm = BetfairStreaming.generate_order_change_message(
        price=5.8,
        size=20,
        side="B",
        status="E",
        sm=16.19,
        sr=3.809999999999999,
        avp=1.50,
        order_id=venue_order_id.value,
        mb=[MatchedOrder(5.0, 100)],
    )

    # Act, Assert
    with pytest.raises(RuntimeError):
        exec_client.check_cache_against_order_image(ocm)
