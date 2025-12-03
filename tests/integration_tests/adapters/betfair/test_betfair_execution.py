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

from __future__ import annotations

import asyncio
import logging
from functools import partial
from types import SimpleNamespace
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import patch

import msgspec
import pytest
from betfair_parser.spec.betting.enums import ExecutionReportErrorCode
from betfair_parser.spec.betting.enums import ExecutionReportStatus
from betfair_parser.spec.betting.enums import InstructionReportErrorCode
from betfair_parser.spec.betting.enums import InstructionReportStatus
from betfair_parser.spec.streaming import OCM
from betfair_parser.spec.streaming import MatchedOrder
from betfair_parser.spec.streaming import Order as BFOrder
from betfair_parser.spec.streaming import stream_decode

from nautilus_trader.adapters.betfair.client import BetfairHttpClient
from nautilus_trader.adapters.betfair.common import OrderSideParser
from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION
from nautilus_trader.adapters.betfair.constants import BETFAIR_QUANTITY_PRECISION
from nautilus_trader.adapters.betfair.data import BetfairDataClient
from nautilus_trader.adapters.betfair.execution import BetfairExecutionClient
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity
from nautilus_trader.adapters.betfair.parsing import requests as parsing_requests
from nautilus_trader.adapters.betfair.parsing.common import betfair_instrument_id
from nautilus_trader.core.rust.model import OrderSide
from nautilus_trader.core.rust.model import OrderStatus
from nautilus_trader.core.rust.model import TimeInForce
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.currencies import GBP
from nautilus_trader.model.enums import LiquiditySide
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
from nautilus_trader.model.instruments import BettingInstrument
from nautilus_trader.model.instruments.betting import null_handicap
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.commands import TestCommandStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from tests.integration_tests.adapters.betfair.test_kit import BetfairResponses
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming
from tests.integration_tests.adapters.betfair.test_kit import betting_instrument
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
    Ready the engine to test a message from betfair, setting orders into the correct
    state.
    """
    if isinstance(order_change_message, bytes):
        order_change_message = stream_decode(order_change_message)
    for oc in order_change_message.oc:
        for orc in oc.orc:
            if orc.uo is not None:
                for order_update in orc.uo:
                    instrument_id = betfair_instrument_id(
                        market_id=oc.id,
                        selection_id=orc.id,
                        selection_handicap=orc.hc or null_handicap(),
                    )
                    order_id = str(order_update.id)
                    venue_order_id = VenueOrderId(order_id)
                    client_order_id = ClientOrderId(order_id)
                    if not cache.instrument(instrument_id):
                        instrument = betting_instrument(
                            market_id=oc.id,
                            selection_id=orc.id,
                            selection_handicap=orc.hc or null_handicap(),
                        )
                        cache.add_instrument(instrument)
                    if not cache.order(client_order_id):
                        assert strategy is not None, "strategy can't be none if accepting order"
                        instrument = cache.instrument(instrument_id)
                        order = TestExecStubs.limit_order(
                            instrument=instrument,
                            price=betfair_float_to_price(order_update.p),
                            client_order_id=client_order_id,
                        )
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


@pytest.fixture
def setup_order_state(exec_client, cache, strategy, trade_id, venue_order_id):
    return partial(
        _setup_order_state,
        exec_client=exec_client,
        cache=cache,
        strategy=strategy,
        trade_id=trade_id,
        include_fills=False,
    )


@pytest.fixture
def setup_order_state_fills(setup_order_state):
    return partial(setup_order_state, include_fills=True)


async def _submit_order(order, exec_client, strategy, cache):
    # We don't want the execution client to actually do anything here
    exec_client.submit_order = MagicMock()
    strategy.submit_order(order)
    await asyncio.sleep(0)
    assert cache.order(order.client_order_id)
    return order


@pytest.fixture
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


@pytest.fixture
def accept_order(exec_client, strategy, cache):
    return partial(_accept_order, exec_client=exec_client, strategy=strategy, cache=cache)


@pytest.fixture
def fill_order(
    exec_client,
    venue_order_id: VenueOrderId,
    quote_currency: Currency,
    trade_id: str | None = None,
):
    return partial(
        _fill_order,
        exec_client=exec_client,
        venue_order_id=venue_order_id,
        quote_currency=quote_currency,
        trade_id=trade_id,
    )


@pytest.fixture
def test_order(instrument, strategy_id):
    return TestExecStubs.limit_order(
        instrument=instrument,
        price=betfair_float_to_price(2.0),
        quantity=Quantity.from_str("100"),
        strategy_id=strategy_id,
    )


@pytest.mark.asyncio
async def test_submit_order_success(exec_client: BetfairDataClient, strategy, test_order):
    # Arrange

    # Act
    strategy.submit_order(test_order)
    await asyncio.sleep(0)

    # Assert
    _, submitted, accepted = test_order.events
    assert isinstance(submitted, OrderSubmitted)
    assert isinstance(accepted, OrderAccepted)
    assert accepted.venue_order_id == VenueOrderId("228302937743")


@pytest.mark.asyncio
async def test_submit_order_error(
    exec_client: BetfairExecutionClient,
    strategy,
    test_order,
    messages,
):
    # Arrange
    mock_betfair_request(exec_client._client, BetfairResponses.betting_place_order_error())

    # Act
    strategy.submit_order(test_order)
    await asyncio.sleep(0)

    # Assert
    _, submitted, rejected = test_order.events
    assert isinstance(submitted, OrderSubmitted)
    assert isinstance(rejected, OrderRejected)
    # Shows both instruction-level and result-level error codes
    assert "ERROR_IN_ORDER" in rejected.reason
    assert "PERMISSION_DENIED" in rejected.reason


@pytest.mark.asyncio
async def test_modify_order_success(
    exec_client: BetfairDataClient,
    strategy,
    venue_order_id,
    accept_order,
    test_order,
    events,
):
    # Arrange
    await accept_order(test_order, venue_order_id=venue_order_id)

    # Act
    strategy.modify_order(test_order, price=betfair_float_to_price(2.5))
    await asyncio.sleep(0)

    # Assert
    pending_update, updated = events[-2:]
    assert isinstance(pending_update, OrderPendingUpdate)
    assert isinstance(updated, OrderUpdated)
    assert updated.price == betfair_float_to_price(50)


@pytest.mark.asyncio
async def test_modify_order_error_order_doesnt_exist(
    exec_engine: BetfairDataClient,
    exec_client,
    test_order,
):
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
    expected_args = tuple(
        {
            "strategy_id": StrategyId("S-001"),
            "instrument_id": InstrumentId.from_str("1-179082386-50214-None.BETFAIR"),
            "client_order_id": ClientOrderId("O-20210410-022422-001-001-1"),
            "venue_order_id": None,
            "reason": "ORDER NOT IN CACHE",
            "ts_event": 0,
        }.values(),
    )
    assert mock_reject.call_args.args == expected_args


@pytest.mark.asyncio
async def test_modify_order_error_no_venue_id(
    betfair_client: BetfairHttpClient,
    exec_client,
    strategy_id,
    submit_order,
    test_order,
    instrument,
):
    # Arrange
    order = await submit_order(test_order)

    # Act
    command = TestCommandStubs.modify_order_command(price=betfair_float_to_price(2.0), order=order)
    with patch.object(BetfairExecutionClient, "generate_order_modify_rejected") as mock_reject:
        exec_client.modify_order(command)
        await asyncio.sleep(0)

    # Assert
    expected_args = tuple(
        {
            "strategy_id": strategy_id,
            "instrument_id": instrument.id,
            "client_order_id": test_order.client_order_id,
            "venue_order_id": None,
            "reason": "ORDER MISSING VENUE_ORDER_ID",
            "ts_event": 0,
        }.values(),
    )
    assert mock_reject.call_args.args == expected_args


@pytest.mark.asyncio
async def test_cancel_order_success(
    betfair_client: BetfairHttpClient,
    exec_client,
    accept_order,
    test_order,
    venue_order_id,
    strategy_id,
    instrument,
):
    # Arrange
    order = await accept_order(order=test_order, venue_order_id=venue_order_id)

    # Act
    command = TestCommandStubs.cancel_order_command(order=order)
    with patch.object(
        BetfairExecutionClient,
        "generate_order_canceled",
    ) as mock_generate_order_canceled:
        exec_client.cancel_order(command)
        await asyncio.sleep(0)

    # Assert
    expected_args = tuple(
        {
            "strategy_id": strategy_id,
            "instrument_id": instrument.id,
            "client_order_id": test_order.client_order_id,
            "venue_order_id": venue_order_id,
            "ts_event": 0,
        }.values(),
    )
    assert mock_generate_order_canceled.call_args.args == expected_args


@pytest.mark.asyncio
async def test_cancel_order_fail(
    betfair_client: BetfairHttpClient,
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
    expected_args = tuple(
        {
            "strategy_id": strategy_id,
            "instrument_id": instrument.id,
            "client_order_id": test_order.client_order_id,
            "venue_order_id": venue_order_id,
            "reason": "ERROR_IN_ORDER: The action failed because the parent order failed",
            "ts_event": 0,
        }.values(),
    )
    assert mock_generate_order_cancel_rejected.call_args.args == expected_args


@pytest.mark.asyncio
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


@pytest.mark.asyncio
async def test_request_account_state(exec_client, cache, account_id):
    # Arrange, Act
    await exec_client.request_account_state()

    # Assert
    assert cache.account(account_id)


@pytest.mark.asyncio
async def test_check_account_currency(exec_client):
    # Arrange, Act, Assert
    await exec_client.check_account_currency()


@pytest.mark.asyncio
async def test_order_stream_full_image(exec_client, setup_order_state, events):
    # Arrange
    raw = BetfairStreaming.ocm_FULL_IMAGE()
    ocm = stream_decode(raw)
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


@pytest.mark.asyncio
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


@pytest.mark.asyncio
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
    assert len(events) == 4


@pytest.mark.asyncio
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


@pytest.mark.asyncio
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
    assert len(events) == 3


@pytest.mark.asyncio
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
    assert len(events) == 4
    fill: OrderFilled = fill_events[0]
    assert isinstance(fill, OrderFilled)
    assert fill.last_px == betfair_float_to_price(1.10)


@pytest.mark.asyncio
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
        order_id=int(venue_order_id.value),
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
    assert len(events) == 8
    fill1, fill2 = (event for event in events if isinstance(event, OrderFilled))
    assert isinstance(fill1, OrderFilled)
    assert isinstance(fill2, OrderFilled)
    assert fill1.last_px == betfair_float_to_price(1.60)
    assert fill2.last_px == betfair_float_to_price(1.50)


@pytest.mark.asyncio
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


@pytest.mark.asyncio
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
    assert isinstance(fill2, OrderFilled)
    assert fill2.trade_id.value == "5b87a0fad91063d93a3df2fe7a369f6c9a19"
    assert isinstance(fill3, OrderFilled)
    assert fill3.trade_id.value == "75076f6b172799e168869d64df86b4d2717d"


@pytest.mark.parametrize(
    ("side", "price", "quantity", "free"),
    [
        (OrderSide.SELL, Price.from_str("2.0"), Quantity.from_str("100"), 9900),
        (OrderSide.SELL, Price.from_str("5.0"), Quantity.from_str("50"), 9950),
        (OrderSide.BUY, Price.from_str("1.2"), Quantity.from_str("100"), 9980),
        (OrderSide.BUY, Price.from_str("5.0"), Quantity.from_str("100"), 9600),
    ],
)
@pytest.mark.asyncio
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
        instrument=instrument,
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
    exec_client.cancel_order(command)
    await asyncio.sleep(0)
    balance_cancel = account.balances()[GBP]
    await asyncio.sleep(0)

    # Assert
    assert balance_pre_order.free == Money(10000.0, GBP)
    assert balance_order.free == Money(free, GBP)
    assert balance_cancel.free == Money(10000.0, GBP)


@pytest.mark.asyncio
async def test_betfair_order_cancelled_no_timestamp(
    exec_client,
    setup_order_state,
    clock,
    cancel_events,
):
    # Arrange
    update = stream_decode(BetfairStreaming.ocm_error_fill())
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


@pytest.mark.asyncio
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
            price=price,
            size=size,
            side=side,
            status=status,
            **raw,
        )
        exec_client.handle_order_stream_update(msgspec.json.encode(order_change_message))
        await asyncio.sleep(0)

    # Assert
    for msg, _, last_qty in zip(fill_events, updates, last_qtys, strict=False):
        assert isinstance(msg, OrderFilled)
        assert msg.last_qty == last_qty


@pytest.mark.asyncio
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


@pytest.mark.asyncio
async def test_generate_order_status_report_client_id(
    exec_client: BetfairExecutionClient,
    betfair_client,
    instrument_provider,
    instrument: BettingInstrument,
) -> None:
    # Arrange
    mock_betfair_request(
        betfair_client,
        BetfairResponses.list_current_orders_custom(
            market_id=instrument.market_id,
            selection_id=instrument.selection_id,
        ),
    )
    instrument_provider.add(instrument)

    # Act
    command = GenerateOrderStatusReport(
        instrument_id=instrument.id,
        client_order_id=None,
        venue_order_id=VenueOrderId("1"),
        command_id=UUID4(),
        ts_init=0,
    )
    report: OrderStatusReport | None = await exec_client.generate_order_status_report(command)

    # Assert
    assert report
    assert report.order_status == OrderStatus.ACCEPTED
    assert report.price == Price(5.0, BETFAIR_PRICE_PRECISION)
    assert report.quantity == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)
    assert report.filled_qty == Quantity(0.0, BETFAIR_QUANTITY_PRECISION)


@pytest.mark.asyncio
async def test_generate_order_status_report_venue_order_id(
    exec_client: BetfairExecutionClient,
    betfair_client,
    instrument_provider,
    instrument: BettingInstrument,
) -> None:
    # Arrange
    response = BetfairResponses.list_current_orders_execution_complete()
    response["result"]["currentOrders"] = response["result"]["currentOrders"][:1]
    mock_betfair_request(betfair_client, response=response)

    client_order_id = ClientOrderId("O-20231004-0534-001-59723858-5")
    venue_order_id = VenueOrderId("323427122115")

    # Act
    command = GenerateOrderStatusReport(
        instrument_id=instrument.id,
        client_order_id=client_order_id,
        venue_order_id=venue_order_id,
        command_id=UUID4(),
        ts_init=0,
    )
    report: OrderStatusReport | None = await exec_client.generate_order_status_report(command)

    # Assert
    assert report
    assert report.order_status == OrderStatus.ACCEPTED
    assert report.price == Price(5.0, BETFAIR_PRICE_PRECISION)
    assert report.quantity == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)
    assert report.filled_qty == Quantity(0.0, BETFAIR_QUANTITY_PRECISION)


@pytest.mark.asyncio
async def test_check_cache_against_order_image_passes(
    exec_client,
    venue_order_id,
    setup_order_state_fills,
):
    # Arrange
    ocm = BetfairStreaming.generate_order_change_message(
        price=5.8,
        size=20,
        side="B",
        status="E",
        sm=16.19,
        sr=3.809999999999999,
        avp=1.50,
        order_id=int(venue_order_id.value),
        mb=[MatchedOrder(5.8, 20)],
    )
    await setup_order_state_fills(order_change_message=ocm)

    # Act, Assert
    exec_client.check_cache_against_order_image(ocm)


@pytest.mark.asyncio
async def test_fok_order_found_in_cache(exec_client, setup_order_state, strategy, cache):
    # Arrange
    instrument = betting_instrument(
        market_id="1-219194342",
        selection_id=61288616,
        selection_handicap=0.0,
    )
    cache.add_instrument(instrument)
    client_order_id = ClientOrderId("O-20231004-0354-001-61288616-1")
    venue_order_id = VenueOrderId("323421338057")
    limit_order = TestExecStubs.limit_order(
        instrument=instrument,
        order_side=OrderSide.SELL,
        price=Price(9.6000000, BETFAIR_PRICE_PRECISION),
        quantity=Quantity(2.8000, 2),
        time_in_force=TimeInForce.FOK,
        client_order_id=client_order_id,
    )
    await _accept_order(limit_order, venue_order_id, exec_client, strategy, cache)

    # Act
    unmatched_order = BFOrder(
        id=323421338057,
        p=9.6,
        s=2.8,
        side="L",
        status="EC",
        pt="L",
        ot="L",
        pd=1696391679000,
        bsp=None,
        rfo="O-20231004-0354-001",
        rfs="OrderBookImbala",
        rc="REG_LGA",
        rac="",
        md=None,
        cd=1696391679000,
        ld=None,
        avp=None,
        sm=0.0,
        sr=0.0,
        sl=0.0,
        sc=2.8,
        sv=0.0,
        lsrc=None,
    )
    exec_client._handle_stream_execution_complete_order_update(unmatched_order=unmatched_order)

    # Assert
    assert cache.order(client_order_id).status == OrderStatus.CANCELED


@pytest.mark.asyncio
async def test_generate_order_status_reports_executable(exec_client):
    # Arrange
    mock_betfair_request(exec_client._client, BetfairResponses.list_current_orders_executable())

    # Act
    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )
    reports = await exec_client.generate_order_status_reports(command)

    # Assert
    assert len(reports) == 2
    assert reports[0].order_side == OrderSide.SELL
    assert reports[0].price == Price(5.0, BETFAIR_PRICE_PRECISION)
    assert reports[0].quantity == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)
    assert reports[0].order_status == OrderStatus.ACCEPTED
    assert reports[0].filled_qty == 0.0
    assert reports[0].time_in_force == TimeInForce.DAY

    assert reports[1].order_side == OrderSide.BUY
    assert reports[1].price == Price(2.0, BETFAIR_PRICE_PRECISION)
    assert reports[1].quantity == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)
    assert reports[1].order_status == OrderStatus.ACCEPTED
    assert reports[1].filled_qty == 0.0
    assert reports[1].time_in_force == TimeInForce.DAY


@pytest.mark.asyncio
async def test_generate_order_status_reports_executable_limit_on_close(exec_client):
    # Arrange
    mock_betfair_request(
        exec_client._client,
        BetfairResponses.list_current_orders_on_close_execution_complete(),
    )

    # Act
    command = GenerateOrderStatusReports(
        instrument_id=None,
        start=None,
        end=None,
        open_only=False,
        command_id=UUID4(),
        ts_init=0,
    )
    reports = await exec_client.generate_order_status_reports(command)

    # Assert
    assert len(reports) == 2

    # Back - BSP order with bspLiability=20.0 but sizeRemaining=10.0 (stake units)
    assert reports[0].order_side == OrderSide.SELL
    assert reports[0].price == Price(5.0, BETFAIR_PRICE_PRECISION)
    assert reports[0].quantity == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)  # Uses stake units, not liability
    assert reports[0].order_status == OrderStatus.ACCEPTED
    assert reports[0].filled_qty == Quantity(0.0, BETFAIR_QUANTITY_PRECISION)  # sizeMatched=0.0
    assert reports[0].time_in_force == TimeInForce.DAY

    # Lay - BSP order with bspLiability=50.0 but sizeRemaining=10.0 (stake units)
    assert reports[1].order_side == OrderSide.BUY
    assert reports[1].price == Price(1.5, BETFAIR_PRICE_PRECISION)
    assert reports[1].quantity == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)  # Uses stake units, not liability
    assert reports[1].order_status == OrderStatus.ACCEPTED
    assert reports[1].filled_qty == Quantity(0.0, BETFAIR_QUANTITY_PRECISION)  # sizeMatched=0.0
    assert reports[1].time_in_force == TimeInForce.DAY


@pytest.mark.asyncio
async def test_generate_fill_reports(exec_client):
    # Arrange
    mock_betfair_request(
        exec_client._client,
        BetfairResponses.list_current_orders_execution_complete(),
    )

    # Act
    command = GenerateFillReports(
        instrument_id=None,
        venue_order_id=None,
        start=None,
        end=None,
        command_id=UUID4(),
        ts_init=0,
    )
    reports = await exec_client.generate_fill_reports(command)

    # Assert
    assert len(reports) == 2
    assert reports[0].order_side == OrderSide.SELL
    assert reports[0].last_px == Price(1.9, BETFAIR_PRICE_PRECISION)
    assert reports[0].last_qty == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)

    assert reports[1].order_side == OrderSide.BUY
    assert reports[1].last_px == Price(1.92, BETFAIR_PRICE_PRECISION)
    assert reports[1].last_qty == Quantity(10.0, BETFAIR_QUANTITY_PRECISION)


@pytest.mark.asyncio
@pytest.mark.live_components
async def test_reconcile_execution_mass_status(exec_client, exec_engine):
    # Arrange
    mock_betfair_request(
        exec_client._client,
        BetfairResponses.list_current_orders_execution_complete(),
    )

    # Act, Assert
    mass_status = await exec_client.generate_mass_status()
    exec_engine._reconcile_execution_mass_status(mass_status)


# A price far below the allowed minimum (~ -1.7e13)
_NEGATIVE_PRICE = -7.849774150506724e14


class _StubUnmatchedOrder(SimpleNamespace):
    """
    Minimal attribute bag to satisfy the handlers.
    """


@pytest.fixture
def order_and_cache(exec_client, monkeypatch):
    """
    Insert a stub order/instrument into the execution-client cache.
    """
    instrument = betting_instrument()
    order = TestExecStubs.limit_order(instrument=instrument)

    # Insert directly into the real cache (simpler than monkey-patching the
    # Cython-backed methods which are immutable).

    exec_client._cache.add_instrument(instrument)
    exec_client._cache.add_order(order)

    return order, instrument


def _make_unmatched_order(order, *, price: float = _NEGATIVE_PRICE):
    """
    Construct an unmatched-order stub with a deliberately bad price.
    """
    return _StubUnmatchedOrder(
        id=str(order.client_order_id),  # Bet ID
        side="L",  # Unused due to monkey-patch above
        p=order.price.as_double(),
        avp=price,
        s=order.quantity.as_double(),
        sm=order.quantity.as_double(),  # Matched size triggers a fill path
        md=0,
        pt=None,
        ot=None,
        sc=0,
        sl=0,
        sv=0,
        lapse_status_reason_code=None,
    )


@pytest.mark.parametrize(
    "handler_name",
    [
        "_handle_stream_executable_order_update",
        "_handle_stream_execution_complete_order_update",
    ],
)
def test_invalid_price_is_skipped(
    handler_name,
    exec_client: BetfairExecutionClient,
    order_and_cache,
    monkeypatch,
    caplog,
):
    order, _ = order_and_cache

    # Arrange: intercept generate_order_filled and capture warnings
    generate_mock = MagicMock()
    monkeypatch.setattr(exec_client, "generate_order_filled", generate_mock)

    # Monkey-patch helpers ONLY for this test to keep side-effects local
    monkeypatch.setattr(OrderSideParser, "to_nautilus", lambda _side: OrderSide.BUY)
    monkeypatch.setattr(parsing_requests, "order_to_trade_id", lambda _uo: TradeId("TRADE-TEST"))

    unmatched_order = _make_unmatched_order(order)

    caplog.set_level(logging.WARNING, logger=exec_client._log.name)

    # Act
    getattr(exec_client, handler_name)(unmatched_order)

    # Assert: no fill generated. Capturing the exact log record is brittle because the
    # BetfairExecutionClient logger is a custom adapter the important functional
    # guarantee is that we *did not* emit a fill.
    generate_mock.assert_not_called()

# Tests for bug fixes - missing instrument rejection
@pytest.mark.asyncio
async def test_submit_order_missing_instrument_generates_denied(
    exec_client: BetfairExecutionClient,
    strategy,
    cache,
):
    # Arrange - create order for instrument not in cache
    missing_instrument_id = InstrumentId.from_str("1.234567890.MISSING.BETFAIR")
    order = strategy.order_factory.limit(
        instrument_id=missing_instrument_id,
        order_side=OrderSide.BUY,
        quantity=Quantity(10.0, precision=2),
        price=Price(5.0, precision=2),
    )

    # Act
    strategy.submit_order(order)
    await asyncio.sleep(0)

    # Assert - should get OrderDenied event, not hang
    assert len(order.events) >= 2  # OrderInitialized + OrderDenied
    denied_events = [e for e in order.events if e.__class__.__name__ == "OrderDenied"]
    assert len(denied_events) == 1
    # Check that the denial reason mentions the missing instrument
    assert "not found" in denied_events[0].reason.lower() or "INSTRUMENT_NOT_FOUND" in denied_events[0].reason


# Tests for bug fixes - pending update cleanup on exception
@pytest.mark.asyncio
async def test_modify_order_exception_cleans_up_pending_updates(
    exec_client: BetfairExecutionClient,
    strategy,
    test_order,
    cache,
):
    # Arrange - submit and accept order first
    strategy.submit_order(test_order)
    await asyncio.sleep(0)

    # Simulate exception during modify
    with patch.object(exec_client._client, "replace_orders", side_effect=Exception("Network error")):
        pending_key = (test_order.client_order_id, test_order.venue_order_id)

        # Act - modify should fail with exception
        command = TestCommandStubs.modify_order_command(
            instrument_id=test_order.instrument_id,
            client_order_id=test_order.client_order_id,
            price=Price(6.0, precision=2),
        )
        await exec_client._modify_price(command, test_order)

        # Assert - pending key should be cleaned up
        assert pending_key not in exec_client._pending_update_order_client_ids


# Tests for bug fixes - error_code None handling
@pytest.mark.asyncio
async def test_modify_order_handles_none_error_code(
    exec_client: BetfairExecutionClient,
    strategy,
    test_order,
    monkeypatch,
):
    # Arrange - submit and accept order first
    strategy.submit_order(test_order)
    await asyncio.sleep(0)

    # Mock replace_orders to return failure with None error_code
    mock_report = SimpleNamespace(
        status=InstructionReportStatus.FAILURE,
        error_code=None,  # This was causing AttributeError
        cancel_instruction_report=SimpleNamespace(
            instruction=SimpleNamespace(bet_id="12345"),
        ),
        place_instruction_report=SimpleNamespace(
            bet_id="67890",
            instruction=SimpleNamespace(
                limit_order=SimpleNamespace(price=6.0),
            ),
        ),
    )

    mock_response = SimpleNamespace(
        error_code=None,
        instruction_reports=[mock_report],
    )

    with patch.object(exec_client._client, "replace_orders", return_value=mock_response):
        # Act - should not raise AttributeError
        command = TestCommandStubs.modify_order_command(
            instrument_id=test_order.instrument_id,
            client_order_id=test_order.client_order_id,
            price=Price(6.0, precision=2),
        )
        await exec_client._modify_price(command, test_order)

        # Assert - should generate modify_rejected (not order_rejected) with "UNKNOWN_ERROR"
        modify_rejected_events = [e for e in test_order.events if e.__class__.__name__ == "OrderModifyRejected"]
        assert len(modify_rejected_events) > 0
        assert "UNKNOWN_ERROR" in modify_rejected_events[-1].reason


# Tests for bug fixes - venue_order_id None in updates
@pytest.mark.asyncio
async def test_modify_quantity_uses_existing_venue_order_id(
    exec_client: BetfairExecutionClient,
    strategy,
    test_order,
):
    # Arrange - submit and accept order first
    strategy.submit_order(test_order)
    await asyncio.sleep(0)

    original_venue_order_id = test_order.venue_order_id
    assert original_venue_order_id is not None

    # Mock successful size reduction response
    mock_report = SimpleNamespace(
        status=InstructionReportStatus.SUCCESS,
        size_cancelled=2.0,
    )
    mock_response = SimpleNamespace(
        error_code=None,
        instruction_reports=[mock_report],
    )

    # The fix ensures that even when command.venue_order_id would be None,
    # the OrderUpdated event uses existing_order.venue_order_id
    with patch.object(exec_client._client, "cancel_orders", return_value=mock_response):
        # Act - normal modify (command has venue_order_id from cache)
        command = TestCommandStubs.modify_order_command(
            instrument_id=test_order.instrument_id,
            client_order_id=test_order.client_order_id,
            venue_order_id=original_venue_order_id,
            quantity=Quantity(8.0, precision=2),
        )

        await exec_client._modify_quantity(command, test_order)

        # Assert - OrderUpdated should have the venue_order_id from existing_order
        # (the fix changed from using command.venue_order_id to existing_order.venue_order_id)
        updated_events = [e for e in test_order.events if e.__class__.__name__ == "OrderUpdated"]
        assert len(updated_events) > 0
        last_update = updated_events[-1]
        assert last_update.venue_order_id == original_venue_order_id
        assert last_update.venue_order_id is not None
        # This test verifies the fix works - previously command.venue_order_id was used
        # which could be None, now existing_order.venue_order_id is always used


def test_get_matched_timestamp_fallback(exec_client):
    """
    Test that _get_matched_timestamp falls back to clock when md is None.
    """
    # Arrange - order with None matched timestamp
    unmatched_order = SimpleNamespace(md=None)

    # Act - should fallback to clock, not hardcoded 0
    timestamp = exec_client._get_matched_timestamp(unmatched_order)

    # Assert - timestamp equals current clock time (which may be 0 in tests, but that's ok)
    # The key is it uses clock, not hardcoded 0
    expected = exec_client._clock.timestamp_ns()
    assert timestamp == expected

    # Test with valid md - should use the provided timestamp, not clock
    unmatched_order_with_md = SimpleNamespace(md=1616568422000)  # March 2021 in millis
    timestamp_with_md = exec_client._get_matched_timestamp(unmatched_order_with_md)
    assert timestamp_with_md == 1616568422000000000  # Converted to nanos
    # Should NOT equal clock time (proves it uses md, not clock)
    assert timestamp_with_md != exec_client._clock.timestamp_ns()


@pytest.mark.asyncio
async def test_generate_order_status_report_handles_multiple_orders_after_replace(
    exec_client,
    betfair_client,
):
    """
    Test that query handles multiple orders with same customer_order_ref (after
    replace).
    """
    # Arrange - mock Betfair returning 2 orders: old cancelled + new active
    # This happens after a replace (price modification) operation
    response = {
        "jsonrpc": "2.0",
        "id": 1,
        "result": {
            "currentOrders": [
                # Old cancelled order
                {
                    "betId": "228059754671",
                    "marketId": "1.180575118",
                    "selectionId": 39980,
                    "handicap": 0.0,
                    "priceSize": {"price": 5.0, "size": 10.0},
                    "bspLiability": 0.0,
                    "side": "BACK",
                    "status": "EXECUTION_COMPLETE",  # Cancelled/filled
                    "persistenceType": "LAPSE",
                    "orderType": "LIMIT",
                    "placedDate": "2021-03-24T06:47:02.000Z",
                    "averagePriceMatched": 0.0,
                    "sizeMatched": 0.0,
                    "sizeRemaining": 0.0,
                    "sizeLapsed": 0.0,
                    "sizeCancelled": 10.0,
                    "sizeVoided": 0.0,
                },
                # New active order (after replace)
                {
                    "betId": "228059754999",
                    "marketId": "1.180575118",
                    "selectionId": 39980,
                    "handicap": 0.0,
                    "priceSize": {"price": 6.0, "size": 10.0},  # New price
                    "bspLiability": 0.0,
                    "side": "BACK",
                    "status": "EXECUTABLE",  # Active
                    "persistenceType": "LAPSE",
                    "orderType": "LIMIT",
                    "placedDate": "2021-03-24T06:47:10.000Z",  # Later timestamp
                    "averagePriceMatched": 0.0,
                    "sizeMatched": 0.0,
                    "sizeRemaining": 10.0,
                    "sizeLapsed": 0.0,
                    "sizeCancelled": 0.0,
                    "sizeVoided": 0.0,
                },
            ],
            "moreAvailable": False,
        },
    }

    # Mock returns BOTH orders (same customer_order_ref)
    mock_betfair_request(betfair_client, response)

    # Act - query by client_order_id only (no venue_order_id)
    command = GenerateOrderStatusReport(
        instrument_id=InstrumentId.from_str("1-180575118-39980-None.BETFAIR"),
        client_order_id=ClientOrderId("O-20251116-141348-001"),
        venue_order_id=None,  # Force query by client_order_id
        command_id=UUID4(),
        ts_init=0,
    )
    report = await exec_client.generate_order_status_report(command)

    # Assert - should select the EXECUTABLE (active) order, not the EXECUTION_COMPLETE
    assert report is not None
    assert report.venue_order_id.value == "228059754999"  # New active order
    assert report.price.as_double() == 6.0  # New price
    assert report.order_status == OrderStatus.ACCEPTED  # EXECUTABLE maps to ACCEPTED


@pytest.mark.asyncio
async def test_submit_order_result_level_failure_no_instruction_reports(
    exec_client: BetfairExecutionClient,
    strategy,
    test_order,
):
    # Arrange
    mock_response = SimpleNamespace(
        status=ExecutionReportStatus.FAILURE,
        error_code=ExecutionReportErrorCode.INSUFFICIENT_FUNDS,
        instruction_reports=None,
    )

    with patch.object(
        exec_client._client,
        "place_orders",
        new_callable=AsyncMock,
        return_value=mock_response,
    ):
        # Act
        strategy.submit_order(test_order)
        await asyncio.sleep(0)

        # Assert
        assert len(test_order.events) >= 2
        rejected_events = [e for e in test_order.events if e.__class__.__name__ == "OrderRejected"]
        assert len(rejected_events) == 1
        expected_error = "INSUFFICIENT_FUNDS (Account has exceeded its exposure limit or available to bet limit)"
        assert rejected_events[0].reason == expected_error


@pytest.mark.asyncio
async def test_submit_order_instruction_level_failure_shows_both_error_codes(
    exec_client: BetfairExecutionClient,
    strategy,
    test_order,
):
    # Arrange - BET_ACTION_ERROR at result level, INVALID_RUNNER at instruction level
    mock_report = SimpleNamespace(
        status=InstructionReportStatus.FAILURE,
        error_code=InstructionReportErrorCode.INVALID_RUNNER,
        bet_id=None,
    )
    mock_response = SimpleNamespace(
        status=ExecutionReportStatus.FAILURE,
        error_code=ExecutionReportErrorCode.BET_ACTION_ERROR,
        instruction_reports=[mock_report],
    )

    with patch.object(
        exec_client._client,
        "place_orders",
        new_callable=AsyncMock,
        return_value=mock_response,
    ):
        # Act
        strategy.submit_order(test_order)
        await asyncio.sleep(0)

        # Assert - both error codes should be in the reason
        rejected_events = [e for e in test_order.events if e.__class__.__name__ == "OrderRejected"]
        assert len(rejected_events) == 1
        reason = rejected_events[0].reason
        assert "INVALID_RUNNER" in reason
        assert "BET_ACTION_ERROR" in reason


# -- Tests for fill cache sync and duplicate fill prevention ----------------------------------


@pytest.mark.asyncio
async def test_sync_fill_caches_from_orders_populates_caches(
    exec_client: BetfairExecutionClient,
    cache,
    accept_order,
):
    # Arrange
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    order = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(10.0, BETFAIR_QUANTITY_PRECISION),
    )
    venue_order_id = VenueOrderId("12345")
    await accept_order(order, venue_order_id)

    fill_event = TestEventStubs.order_filled(
        order,
        instrument=instrument,
        venue_order_id=venue_order_id,
        trade_id=TradeId("TRADE-001"),
        last_qty=Quantity(5.0, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(2.0, BETFAIR_PRICE_PRECISION),
    )
    order.apply(fill_event)
    cache.update_order(order)

    exec_client._filled_qty_cache.clear()
    exec_client._published_executions.clear()

    # Act
    exec_client._sync_fill_caches_from_orders()

    # Assert
    assert order.client_order_id in exec_client._filled_qty_cache
    assert exec_client._filled_qty_cache[order.client_order_id] == Quantity(5.0, BETFAIR_QUANTITY_PRECISION)
    assert TradeId("TRADE-001") in exec_client._published_executions[order.client_order_id]


@pytest.mark.asyncio
async def test_determine_fill_qty_returns_zero_when_already_filled(
    exec_client: BetfairExecutionClient,
    cache,
    accept_order,
):
    # Arrange
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    order = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(5.33, BETFAIR_QUANTITY_PRECISION),
    )
    venue_order_id = VenueOrderId("12345")
    await accept_order(order, venue_order_id)

    fill_event = TestEventStubs.order_filled(
        order,
        instrument=instrument,
        venue_order_id=venue_order_id,
        trade_id=TradeId("TRADE-001"),
        last_qty=Quantity(5.33, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(2.0, BETFAIR_PRICE_PRECISION),
    )
    order.apply(fill_event)
    cache.update_order(order)

    # Simulate stream sending same matched size (startup scenario)
    unmatched_order = SimpleNamespace(sm=5.33)

    # Act
    exec_client._filled_qty_cache.clear()
    fill_qty = exec_client._determine_fill_qty(unmatched_order, order)

    # Assert
    assert fill_qty == Quantity.zero(BETFAIR_QUANTITY_PRECISION)


@pytest.mark.asyncio
async def test_determine_fill_qty_uses_max_of_cache_and_order_filled(
    exec_client: BetfairExecutionClient,
    cache,
    accept_order,
):
    # Arrange
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    order = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(10.0, BETFAIR_QUANTITY_PRECISION),
    )
    venue_order_id = VenueOrderId("12345")
    await accept_order(order, venue_order_id)

    fill_event = TestEventStubs.order_filled(
        order,
        instrument=instrument,
        venue_order_id=venue_order_id,
        trade_id=TradeId("TRADE-001"),
        last_qty=Quantity(3.0, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(2.0, BETFAIR_PRICE_PRECISION),
    )
    order.apply(fill_event)
    cache.update_order(order)

    # Cache has stale (lower) value
    exec_client._filled_qty_cache[order.client_order_id] = Quantity(2.0, BETFAIR_QUANTITY_PRECISION)

    # Stream reports 5.0 matched
    unmatched_order = SimpleNamespace(sm=5.0)

    # Act
    fill_qty = exec_client._determine_fill_qty(unmatched_order, order)

    # Assert - should use order.filled_qty (3.0) not cache (2.0)
    assert fill_qty == Quantity(2.0, BETFAIR_QUANTITY_PRECISION)


@pytest.mark.parametrize(
    "handler_name",
    [
        "_handle_stream_executable_order_update",
        "_handle_stream_execution_complete_order_update",
    ],
)
def test_duplicate_fill_prevented_on_startup(
    handler_name,
    exec_client: BetfairExecutionClient,
    order_and_cache,
    monkeypatch,
):
    """
    Test that duplicate fills are prevented when stream sends full state on startup.
    """
    order, instrument = order_and_cache

    # Simulate order already accepted
    exec_client.generate_order_accepted(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("12345"),
        ts_event=0,
    )

    # Simulate order fully filled previously
    exec_client.generate_order_filled(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("12345"),
        trade_id=TradeId("TRADE-001"),
        venue_position_id=None,
        order_side=order.side,
        order_type=order.order_type,
        last_qty=order.quantity,
        last_px=order.price,
        quote_currency=instrument.quote_currency,
        commission=Money(0, GBP),
        liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
        ts_event=0,
    )

    # Clear caches to simulate startup/reconnect
    exec_client._filled_qty_cache.clear()
    exec_client._published_executions.clear()

    generate_mock = MagicMock()
    monkeypatch.setattr(exec_client, "generate_order_filled", generate_mock)
    monkeypatch.setattr(OrderSideParser, "to_nautilus", lambda _side: OrderSide.BUY)
    monkeypatch.setattr(parsing_requests, "order_to_trade_id", lambda _uo: TradeId("TRADE-NEW"))

    # Stream sends same matched size (would cause duplicate if not handled)
    unmatched_order = _StubUnmatchedOrder(
        id=str(order.client_order_id),
        side="L",
        p=order.price.as_double(),
        avp=order.price.as_double(),
        s=order.quantity.as_double(),
        sm=order.quantity.as_double(),
        md=0,
        pt=None,
        ot=None,
        sc=0,
        sl=0,
        sv=0,
        lapse_status_reason_code=None,
    )

    # Act
    getattr(exec_client, handler_name)(unmatched_order)

    # Assert - no fill generated since order already fully filled
    generate_mock.assert_not_called()


@pytest.mark.asyncio
async def test_connect_invokes_sync_fill_caches(exec_client: BetfairExecutionClient, monkeypatch):
    sync_called = False

    original_sync = exec_client._sync_fill_caches_from_orders

    def track_sync():
        nonlocal sync_called
        sync_called = True
        original_sync()

    monkeypatch.setattr(exec_client, "_sync_fill_caches_from_orders", track_sync)
    monkeypatch.setattr(exec_client._client, "connect", AsyncMock())
    monkeypatch.setattr(exec_client._stream, "connect", AsyncMock())
    monkeypatch.setattr(exec_client, "check_account_currency", AsyncMock())
    monkeypatch.setattr(exec_client, "request_account_state", AsyncMock())
    monkeypatch.setattr(exec_client, "_send_account_state", MagicMock())

    # Act
    await exec_client._connect()

    # Assert
    assert sync_called


@pytest.mark.asyncio
async def test_reconnect_invokes_sync_fill_caches(exec_client: BetfairExecutionClient, monkeypatch):
    sync_called = False

    original_sync = exec_client._sync_fill_caches_from_orders

    def track_sync():
        nonlocal sync_called
        sync_called = True
        original_sync()

    monkeypatch.setattr(exec_client, "_sync_fill_caches_from_orders", track_sync)
    monkeypatch.setattr(exec_client._client, "reconnect", AsyncMock())
    monkeypatch.setattr(exec_client._stream, "reconnect", AsyncMock())
    monkeypatch.setattr(exec_client, "request_account_state", AsyncMock())
    monkeypatch.setattr(exec_client, "_send_account_state", MagicMock())
    exec_client._update_account_task = None

    # Act
    await exec_client._reconnect()

    # Assert
    assert sync_called


@pytest.mark.parametrize(
    "handler_name",
    [
        "_handle_stream_executable_order_update",
        "_handle_stream_execution_complete_order_update",
    ],
)
def test_overfill_rejection_skips_fill(
    handler_name,
    exec_client: BetfairExecutionClient,
    order_and_cache,
    monkeypatch,
):
    order, instrument = order_and_cache

    exec_client.generate_order_accepted(
        strategy_id=order.strategy_id,
        instrument_id=order.instrument_id,
        client_order_id=order.client_order_id,
        venue_order_id=VenueOrderId("12345"),
        ts_event=0,
    )

    generate_mock = MagicMock()
    monkeypatch.setattr(exec_client, "generate_order_filled", generate_mock)
    monkeypatch.setattr(OrderSideParser, "to_nautilus", lambda _side: OrderSide.BUY)
    monkeypatch.setattr(parsing_requests, "order_to_trade_id", lambda _uo: TradeId("TRADE-NEW"))

    # Stream reports MORE matched than order quantity (would cause overfill)
    overfill_qty = order.quantity.as_double() + 1.0
    unmatched_order = _StubUnmatchedOrder(
        id=str(order.client_order_id),
        side="L",
        p=order.price.as_double(),
        avp=order.price.as_double(),
        s=order.quantity.as_double(),
        sm=overfill_qty,
        md=0,
        pt=None,
        ot=None,
        sc=0,
        sl=0,
        sv=0,
        lapse_status_reason_code=None,
    )

    # Act
    getattr(exec_client, handler_name)(unmatched_order)

    # Assert - no fill generated due to overfill protection
    generate_mock.assert_not_called()


@pytest.mark.parametrize("sm_value", [None, 0, 0.0])
def test_determine_fill_qty_handles_sm_none_and_zero(
    sm_value,
    exec_client: BetfairExecutionClient,
    cache,
):
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    order = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(10.0, BETFAIR_QUANTITY_PRECISION),
    )

    unmatched_order = SimpleNamespace(sm=sm_value)

    # Act
    fill_qty = exec_client._determine_fill_qty(unmatched_order, order)

    # Assert
    assert fill_qty == Quantity.zero(BETFAIR_QUANTITY_PRECISION)


@pytest.mark.asyncio
async def test_determine_fill_qty_cache_ahead_of_order(
    exec_client: BetfairExecutionClient,
    cache,
    accept_order,
):
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    order = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(10.0, BETFAIR_QUANTITY_PRECISION),
    )
    venue_order_id = VenueOrderId("12345")
    await accept_order(order, venue_order_id)

    # Cache has HIGHER value than order.filled_qty (0)
    exec_client._filled_qty_cache[order.client_order_id] = Quantity(4.0, BETFAIR_QUANTITY_PRECISION)

    # Stream reports 5.0 matched
    unmatched_order = SimpleNamespace(sm=5.0)

    # Act
    fill_qty = exec_client._determine_fill_qty(unmatched_order, order)

    # Assert - should only fill 1.0 (5.0 - 4.0 cache value)
    assert fill_qty == Quantity(1.0, BETFAIR_QUANTITY_PRECISION)


@pytest.mark.asyncio
async def test_determine_fill_qty_clears_cache_on_completion(
    exec_client: BetfairExecutionClient,
    cache,
    accept_order,
):
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    order = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(10.0, BETFAIR_QUANTITY_PRECISION),
    )
    venue_order_id = VenueOrderId("12345")
    await accept_order(order, venue_order_id)

    exec_client._filled_qty_cache[order.client_order_id] = Quantity(5.0, BETFAIR_QUANTITY_PRECISION)

    # Stream reports full order quantity matched (completion)
    unmatched_order = SimpleNamespace(sm=10.0)

    # Act
    fill_qty = exec_client._determine_fill_qty(unmatched_order, order)

    # Assert
    assert fill_qty == Quantity(5.0, BETFAIR_QUANTITY_PRECISION)
    assert order.client_order_id not in exec_client._filled_qty_cache


def test_sync_fill_caches_with_multiple_orders(
    exec_client: BetfairExecutionClient,
    cache,
):
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    # Create first order and apply submitted/accepted/filled events directly
    order1 = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(10.0, BETFAIR_QUANTITY_PRECISION),
        client_order_id=ClientOrderId("ORDER-001"),
    )
    submitted1 = TestEventStubs.order_submitted(order1)
    order1.apply(submitted1)
    accepted1 = TestEventStubs.order_accepted(order1, venue_order_id=VenueOrderId("11111"))
    order1.apply(accepted1)

    fill1 = TestEventStubs.order_filled(
        order1,
        instrument=instrument,
        venue_order_id=VenueOrderId("11111"),
        trade_id=TradeId("TRADE-A"),
        last_qty=Quantity(3.0, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(2.0, BETFAIR_PRICE_PRECISION),
    )
    order1.apply(fill1)
    cache.add_order(order1)

    # Create second order with multiple fills
    order2 = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(20.0, BETFAIR_QUANTITY_PRECISION),
        client_order_id=ClientOrderId("ORDER-002"),
    )
    submitted2 = TestEventStubs.order_submitted(order2)
    order2.apply(submitted2)
    accepted2 = TestEventStubs.order_accepted(order2, venue_order_id=VenueOrderId("22222"))
    order2.apply(accepted2)

    fill2a = TestEventStubs.order_filled(
        order2,
        instrument=instrument,
        venue_order_id=VenueOrderId("22222"),
        trade_id=TradeId("TRADE-B1"),
        last_qty=Quantity(5.0, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(2.0, BETFAIR_PRICE_PRECISION),
    )
    order2.apply(fill2a)

    fill2b = TestEventStubs.order_filled(
        order2,
        instrument=instrument,
        venue_order_id=VenueOrderId("22222"),
        trade_id=TradeId("TRADE-B2"),
        last_qty=Quantity(7.0, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(2.0, BETFAIR_PRICE_PRECISION),
    )
    order2.apply(fill2b)
    cache.add_order(order2)

    exec_client._filled_qty_cache.clear()
    exec_client._published_executions.clear()

    # Act
    exec_client._sync_fill_caches_from_orders()

    # Assert - both orders synced
    assert exec_client._filled_qty_cache[order1.client_order_id] == Quantity(
        3.0,
        BETFAIR_QUANTITY_PRECISION,
    )
    assert exec_client._filled_qty_cache[order2.client_order_id] == Quantity(
        12.0,
        BETFAIR_QUANTITY_PRECISION,
    )
    assert TradeId("TRADE-A") in exec_client._published_executions[order1.client_order_id]
    assert TradeId("TRADE-B1") in exec_client._published_executions[order2.client_order_id]
    assert TradeId("TRADE-B2") in exec_client._published_executions[order2.client_order_id]


def test_sync_fill_caches_ignores_orders_without_fills(
    exec_client: BetfairExecutionClient,
    cache,
):
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    # Order with fills
    order_with_fill = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(10.0, BETFAIR_QUANTITY_PRECISION),
        client_order_id=ClientOrderId("ORDER-WITH-FILL"),
    )
    submitted1 = TestEventStubs.order_submitted(order_with_fill)
    order_with_fill.apply(submitted1)
    accepted1 = TestEventStubs.order_accepted(order_with_fill, venue_order_id=VenueOrderId("11111"))
    order_with_fill.apply(accepted1)

    fill = TestEventStubs.order_filled(
        order_with_fill,
        instrument=instrument,
        venue_order_id=VenueOrderId("11111"),
        trade_id=TradeId("TRADE-001"),
        last_qty=Quantity(5.0, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(2.0, BETFAIR_PRICE_PRECISION),
    )
    order_with_fill.apply(fill)
    cache.add_order(order_with_fill)

    # Order without fills (just accepted)
    order_no_fill = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(20.0, BETFAIR_QUANTITY_PRECISION),
        client_order_id=ClientOrderId("ORDER-NO-FILL"),
    )
    submitted2 = TestEventStubs.order_submitted(order_no_fill)
    order_no_fill.apply(submitted2)
    accepted2 = TestEventStubs.order_accepted(order_no_fill, venue_order_id=VenueOrderId("22222"))
    order_no_fill.apply(accepted2)
    cache.add_order(order_no_fill)

    exec_client._filled_qty_cache.clear()
    exec_client._published_executions.clear()

    # Act
    exec_client._sync_fill_caches_from_orders()

    # Assert
    assert order_with_fill.client_order_id in exec_client._filled_qty_cache
    assert order_no_fill.client_order_id not in exec_client._filled_qty_cache
    assert TradeId("TRADE-001") in exec_client._published_executions[order_with_fill.client_order_id]
    assert len(exec_client._published_executions[order_no_fill.client_order_id]) == 0


@pytest.mark.asyncio
async def test_sync_fill_caches_does_not_duplicate_existing_trade_ids(
    exec_client: BetfairExecutionClient,
    cache,
    accept_order,
):
    instrument = betting_instrument()
    cache.add_instrument(instrument)

    order = TestExecStubs.limit_order(
        instrument=instrument,
        quantity=Quantity(10.0, BETFAIR_QUANTITY_PRECISION),
    )
    await accept_order(order, VenueOrderId("12345"))

    fill = TestEventStubs.order_filled(
        order,
        instrument=instrument,
        venue_order_id=VenueOrderId("12345"),
        trade_id=TradeId("TRADE-001"),
        last_qty=Quantity(5.0, BETFAIR_QUANTITY_PRECISION),
        last_px=Price(2.0, BETFAIR_PRICE_PRECISION),
    )
    order.apply(fill)
    cache.update_order(order)

    # Pre-populate cache with the same trade ID
    exec_client._filled_qty_cache[order.client_order_id] = Quantity(5.0, BETFAIR_QUANTITY_PRECISION)
    exec_client._published_executions[order.client_order_id].append(TradeId("TRADE-001"))

    # Act
    exec_client._sync_fill_caches_from_orders()

    # Assert - trade ID should not be duplicated
    trade_ids = exec_client._published_executions[order.client_order_id]
    assert trade_ids.count(TradeId("TRADE-001")) == 1
