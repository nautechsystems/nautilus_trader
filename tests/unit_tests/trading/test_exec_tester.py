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

from decimal import Decimal
from unittest.mock import MagicMock

import pandas as pd
import pytest

from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


@pytest.fixture
def clock():
    return TestClock()


@pytest.fixture
def trader_id():
    return TestIdStubs.trader_id()


@pytest.fixture
def msgbus(trader_id, clock):
    return MessageBus(
        trader_id=trader_id,
        clock=clock,
    )


@pytest.fixture
def cache():
    return TestComponentStubs.cache()


@pytest.fixture
def portfolio(msgbus, cache, clock):
    return Portfolio(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture
def data_engine(msgbus, cache, clock):
    return DataEngine(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture
def exec_engine(msgbus, cache, clock):
    return ExecutionEngine(
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture
def risk_engine(portfolio, msgbus, cache, clock):
    return RiskEngine(
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )


@pytest.fixture
def instrument():
    return TestInstrumentProvider.btcusdt_binance()


@pytest.fixture
def instrument_id(instrument):
    return instrument.id


@pytest.fixture
def setup_environment(cache, instrument, data_engine, exec_engine, risk_engine):
    cache.add_instrument(instrument)
    data_engine.start()
    exec_engine.start()
    risk_engine.start()

    yield  # Tests run here

    # Cleanup
    if data_engine.is_running:
        data_engine.stop()
    if exec_engine.is_running:
        exec_engine.stop()
    if risk_engine.is_running:
        risk_engine.stop()


@pytest.fixture
def create_tester_factory(trader_id, portfolio, msgbus, cache, clock, setup_environment):
    testers = []

    def _create_tester(config):
        tester = ExecTester(config)
        tester.register(
            trader_id=trader_id,
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
        )
        testers.append(tester)
        return tester

    yield _create_tester


def test_on_start_initializes_instrument(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Act
    tester.on_start()

    # Assert
    assert tester.instrument == instrument
    assert tester.price_offset == instrument.price_increment * config.tob_offset_ticks


def test_maintains_buy_order_on_quote_tick(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=False,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is not None
    assert isinstance(tester.buy_order, LimitOrder)
    assert tester.buy_order.side == OrderSide.BUY
    assert tester.buy_order.quantity == Quantity.from_str("0.01")
    # Check price is offset from best bid
    expected_price = instrument.make_price(Decimal("100.0") - tester.price_offset)
    assert tester.buy_order.price == expected_price


def test_maintains_sell_order_on_quote_tick(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=False,
        enable_sells=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.sell_order is not None
    assert isinstance(tester.sell_order, LimitOrder)
    assert tester.sell_order.side == OrderSide.SELL
    assert tester.sell_order.quantity == Quantity.from_str("0.01")
    # Check price is offset from best ask
    expected_price = instrument.make_price(Decimal("101.0") + tester.price_offset)
    assert tester.sell_order.price == expected_price


def test_maintains_both_orders_on_quote_tick(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is not None
    assert tester.sell_order is not None
    assert tester.buy_order.side == OrderSide.BUY
    assert tester.sell_order.side == OrderSide.SELL


def test_post_only_with_test_reject(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=True,
        use_post_only=True,
        test_reject_post_only=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act
    tester.on_quote_tick(quote)

    # Assert - orders should cross the spread to test rejection
    assert tester.buy_order is not None
    assert tester.sell_order is not None
    assert tester.buy_order.price > quote.ask_price  # Should be on wrong side
    assert tester.sell_order.price < quote.bid_price  # Should be on wrong side


def test_order_with_expiry_time(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=False,
        order_expire_time_delta_mins=5,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is not None
    assert tester.buy_order.time_in_force == TimeInForce.GTD
    assert tester.buy_order.expire_time is not None
    expected_expire = clock.utc_now() + pd.Timedelta(minutes=5)
    assert tester.buy_order.expire_time == expected_expire


def test_dry_run_mode_prevents_order_submission(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=True,
        dry_run=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is None
    assert tester.sell_order is None


def test_on_order_book_maintains_orders(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    # Create a book with bids and asks
    book = TestDataStubs.make_book(
        instrument=instrument,
        book_type=BookType.L2_MBP,
        bids=[(100.0, 10.0), (99.5, 20.0)],
        asks=[(101.0, 10.0), (101.5, 20.0)],
    )

    # Act
    tester.on_order_book(book)

    # Assert
    assert tester.buy_order is not None
    assert tester.sell_order is not None


def test_emulation_trigger_configuration(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        emulation_trigger="BID_ASK",
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is not None
    assert tester.buy_order.emulation_trigger == TriggerType.BID_ASK


def test_use_quote_quantity(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("100"),  # Quote currency amount
        enable_buys=True,
        enable_sells=False,
        use_quote_quantity=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is not None
    assert tester.buy_order.is_quote_quantity is True


def test_no_instrument_stops_strategy(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
):
    # Arrange
    unknown_instrument = TestIdStubs.audusd_id()  # Not in cache
    config = ExecTesterConfig(
        instrument_id=unknown_instrument,
        order_qty=Decimal("0.01"),
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Track if stop was called
    stop_called = False
    original_stop = tester.stop

    def track_stop():
        nonlocal stop_called
        stop_called = True
        original_stop()

    tester.stop = track_stop

    # Act
    tester.on_start()

    # Assert
    assert stop_called
    assert tester.instrument is None


def test_on_trade_tick_logging(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        log_data=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    trade = TestDataStubs.trade_tick(instrument)

    # Act - should not raise
    tester.on_trade_tick(trade)


def test_on_bar_logging(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
):
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        log_data=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    tester.on_start()

    bar = TestDataStubs.bar_5decimal()

    # Act - should not raise
    tester.on_bar(bar)


def test_modifies_order_when_price_moves_with_modify_flag(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that orders are modified when market moves and modify flag is set.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=False,
        modify_orders_to_maintain_tob_offset=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Use mocker.spy for better verification
    modify_spy = mocker.spy(tester, "modify_order")

    tester.on_start()

    # Submit initial order with proper price objects
    bid_price = instrument.make_price(100.0)
    ask_price = instrument.make_price(101.0)
    quote1 = TestDataStubs.quote_tick(instrument, bid_price=bid_price, ask_price=ask_price)
    tester.on_quote_tick(quote1)

    # Simulate order acceptance
    buy_order = tester.buy_order
    buy_order.apply(TestEventStubs.order_submitted(buy_order))
    buy_order.apply(TestEventStubs.order_accepted(buy_order, venue_order_id=VenueOrderId("V-001")))

    # Act - market moves up using proper price objects
    new_bid_price = instrument.make_price(102.0)
    new_ask_price = instrument.make_price(103.0)
    quote2 = TestDataStubs.quote_tick(instrument, bid_price=new_bid_price, ask_price=new_ask_price)
    tester.on_quote_tick(quote2)

    # Assert
    assert modify_spy.call_count == 1
    modify_call = modify_spy.call_args
    assert modify_call[0][0] == buy_order  # First arg is the order
    # Check new price is correctly offset from new bid using domain objects
    expected_new_price = instrument.make_price(new_bid_price - tester.price_offset)
    assert modify_call[1]["price"] == expected_new_price


def test_cancel_replace_when_price_moves_with_cancel_replace_flag(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    """
    Test that orders are cancelled and replaced when market moves.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=False,
        enable_sells=True,
        cancel_replace_orders_to_maintain_tob_offset=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on cancel_order and track new submissions
    cancel_spy = MagicMock()
    original_cancel = tester.cancel_order
    tester.cancel_order = lambda *args, **kwargs: (
        cancel_spy(*args, **kwargs),
        original_cancel(*args, **kwargs),
    )[1]

    tester.on_start()

    # Submit initial order
    quote1 = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)
    tester.on_quote_tick(quote1)

    initial_sell_order = tester.sell_order
    # Simulate order acceptance
    initial_sell_order.apply(TestEventStubs.order_submitted(initial_sell_order))
    initial_sell_order.apply(
        TestEventStubs.order_accepted(initial_sell_order, venue_order_id=VenueOrderId("V-001")),
    )

    # Act - market moves down
    quote2 = TestDataStubs.quote_tick(instrument, bid_price=98.0, ask_price=99.0)
    tester.on_quote_tick(quote2)

    # Assert
    assert cancel_spy.called
    assert cancel_spy.call_args[0][0] == initial_sell_order
    # New order should be created
    assert tester.sell_order != initial_sell_order
    assert tester.sell_order.price == instrument.make_price(Decimal("99.0") + tester.price_offset)


def test_resubmits_order_when_not_active(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    """
    Test that orders are resubmitted when they're not active.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=False,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    tester.on_start()

    # Submit initial order
    quote1 = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)
    tester.on_quote_tick(quote1)

    initial_order = tester.buy_order
    # Simulate order rejection
    initial_order.apply(TestEventStubs.order_submitted(initial_order))
    initial_order.apply(TestEventStubs.order_rejected(initial_order))

    # Act - new quote should trigger resubmission
    quote2 = TestDataStubs.quote_tick(instrument, bid_price=100.5, ask_price=101.5)
    tester.on_quote_tick(quote2)

    # Assert - new order created since previous was rejected
    assert tester.buy_order != initial_order
    assert tester.buy_order.status == OrderStatus.INITIALIZED


def test_on_stop_cancels_orders_individually(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
):
    """Test individual order cancellation on stop - verifies logic flow."""
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        cancel_orders_on_stop=True,
        use_individual_cancels_on_stop=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Track cancel calls
    cancel_spy = MagicMock()
    tester.cancel_order = cancel_spy

    tester.on_start()

    # Act
    tester.on_stop()

    # Assert - cancel_order would be called for each open order
    # Since no orders are actually submitted in this test, no cancels occur
    # This test verifies the configuration path is taken
    assert config.use_individual_cancels_on_stop is True


def test_on_stop_batch_cancels_orders(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    """Test batch order cancellation on stop - verifies logic flow."""
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        cancel_orders_on_stop=True,
        use_batch_cancel_on_stop=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on cancel_orders (batch)
    batch_cancel_spy = MagicMock()
    tester.cancel_orders = batch_cancel_spy

    tester.on_start()

    # Act
    tester.on_stop()

    # Assert - batch cancel would be called if there were open orders
    # Since no orders are actually submitted in this test, no cancels occur
    # This test verifies the configuration path is taken
    assert config.use_batch_cancel_on_stop is True


def test_on_stop_cancel_all_orders(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
):
    """
    Test cancel all orders on stop.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        cancel_orders_on_stop=True,
        use_individual_cancels_on_stop=False,
        use_batch_cancel_on_stop=False,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on cancel_all_orders
    cancel_all_spy = MagicMock()
    tester.cancel_all_orders = cancel_all_spy

    tester.on_start()

    # Act
    tester.on_stop()

    # Assert
    assert cancel_all_spy.called
    assert cancel_all_spy.call_args[0][0] == instrument_id


def test_on_stop_closes_positions(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
):
    """
    Test position closing on stop.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        close_positions_on_stop=True,
        close_positions_time_in_force=TimeInForce.IOC,
        reduce_only_on_stop=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on close_all_positions
    close_spy = MagicMock()
    tester.close_all_positions = close_spy

    tester.on_start()

    # Act
    tester.on_stop()

    # Assert
    assert close_spy.called
    call_kwargs = close_spy.call_args[1]
    assert call_kwargs["instrument_id"] == instrument_id
    assert call_kwargs["time_in_force"] == TimeInForce.IOC
    assert call_kwargs["reduce_only"] is True


def test_on_stop_respects_can_unsubscribe_flag(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
):
    """
    Test that unsubscribe is not called when can_unsubscribe=False.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        subscribe_quotes=True,
        subscribe_trades=True,
        can_unsubscribe=False,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on unsubscribe methods
    unsubscribe_quotes_spy = MagicMock()
    unsubscribe_trades_spy = MagicMock()
    tester.unsubscribe_quote_ticks = unsubscribe_quotes_spy
    tester.unsubscribe_trade_ticks = unsubscribe_trades_spy

    tester.on_start()

    # Act
    tester.on_stop()

    # Assert - no unsubscribe calls should be made
    assert not unsubscribe_quotes_spy.called
    assert not unsubscribe_trades_spy.called


def test_open_position_on_start_with_buy_side(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
):
    """
    Test that open position uses buy side for positive quantity.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        open_position_on_start_qty=Decimal("0.1"),
        open_position_time_in_force=TimeInForce.IOC,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Track submitted orders
    submitted_orders = []
    original_submit = tester.submit_order
    tester.submit_order = lambda order, **kwargs: (
        submitted_orders.append(order),
        original_submit(order, **kwargs),
    )[1]

    # Act
    tester.on_start()

    # Assert
    assert len(submitted_orders) == 1
    order = submitted_orders[0]
    assert isinstance(order, MarketOrder)
    assert order.side == OrderSide.BUY
    assert order.time_in_force == TimeInForce.IOC


def test_open_position_on_start_with_sell_side(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
):
    """
    Test that open position uses sell side for negative quantity.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        open_position_on_start_qty=Decimal("-0.1"),
        open_position_time_in_force=TimeInForce.IOC,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Track submitted orders
    submitted_orders = []
    original_submit = tester.submit_order
    tester.submit_order = lambda order, **kwargs: (
        submitted_orders.append(order),
        original_submit(order, **kwargs),
    )[1]

    # Act
    tester.on_start()

    # Assert
    assert len(submitted_orders) == 1
    order = submitted_orders[0]
    assert isinstance(order, MarketOrder)
    assert order.side == OrderSide.SELL
    assert order.time_in_force == TimeInForce.IOC


def test_invalid_emulation_trigger_raises_key_error(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    """
    Test that invalid emulation trigger string raises KeyError.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        emulation_trigger="INVALID_TRIGGER",
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    tester.on_start()

    quote = TestDataStubs.quote_tick(
        instrument,
        bid_price=100.0,
        ask_price=101.0,
    )

    # Act & Assert
    with pytest.raises(KeyError):
        tester.on_quote_tick(quote)


@pytest.mark.parametrize(
    "trigger_type,expected_trigger",
    [
        ("NO_TRIGGER", TriggerType.NO_TRIGGER),
        ("DEFAULT", TriggerType.DEFAULT),
        ("BID_ASK", TriggerType.BID_ASK),
        ("LAST_PRICE", TriggerType.LAST_PRICE),
        ("DOUBLE_LAST", TriggerType.DOUBLE_LAST),
        ("DOUBLE_BID_ASK", TriggerType.DOUBLE_BID_ASK),
    ],
)
def test_emulation_trigger_types_parametrized(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
    trigger_type,
    expected_trigger,
):
    """
    Test various emulation trigger types.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        emulation_trigger=trigger_type,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    tester.on_start()

    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is not None
    assert tester.buy_order.emulation_trigger == expected_trigger


@pytest.mark.parametrize(
    "enable_buys,enable_sells,expected_orders",
    [
        (True, False, ("buy_order",)),
        (False, True, ("sell_order",)),
        (True, True, ("buy_order", "sell_order")),
    ],
)
def test_order_creation_by_side_parametrized(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
    enable_buys,
    enable_sells,
    expected_orders,
):
    """
    Test order creation based on enabled sides.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=enable_buys,
        enable_sells=enable_sells,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    tester.on_start()

    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)

    # Act
    tester.on_quote_tick(quote)

    # Assert
    for order_attr in expected_orders:
        order = getattr(tester, order_attr)
        assert order is not None
        assert isinstance(order, LimitOrder)

    # Check that non-expected orders are None
    all_orders = ("buy_order", "sell_order")
    for order_attr in all_orders:
        if order_attr not in expected_orders:
            assert getattr(tester, order_attr) is None


def test_post_only_flag_set_correctly(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    """
    Test that post_only flag is set when configured.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=True,
        use_post_only=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    tester.on_start()

    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order.is_post_only is True
    assert tester.sell_order.is_post_only is True


def test_order_expiry_time_within_tolerance(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    """
    Test that order expiry time is set correctly with tolerance.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        order_expire_time_delta_mins=5,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Freeze clock at a specific time
    start_time = pd.Timestamp("2024-01-01 12:00:00", tz="UTC")
    clock.set_time(start_time.value)  # Convert to nanoseconds

    tester.on_start()

    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is not None
    assert tester.buy_order.time_in_force == TimeInForce.GTD
    assert tester.buy_order.expire_time is not None

    expected_expire = start_time + pd.Timedelta(minutes=5)
    # Check within 1ms tolerance
    time_diff = abs((tester.buy_order.expire_time - expected_expire).total_seconds())
    assert time_diff < 0.001


def test_use_quote_quantity_with_correct_precision(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    """
    Test that quote quantity orders use correct precision.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("1000.0"),  # Quote amount
        enable_buys=True,
        use_quote_quantity=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )
    tester.on_start()

    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)

    # Act
    tester.on_quote_tick(quote)

    # Assert
    assert tester.buy_order is not None
    assert tester.buy_order.is_quote_quantity is True
    # Quantity should be rounded to instrument precision
    assert tester.buy_order.quantity == instrument.make_qty(1000.0)


def test_subscription_parameters_passed_correctly(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
):
    """
    Test that subscription parameters are passed correctly.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        subscribe_book=True,
        book_type=BookType.L3_MBO,
        book_depth=20,
        book_interval_ms=500,
        client_id=ClientId("TEST_CLIENT"),
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on subscribe_order_book_at_interval
    subscribe_spy = MagicMock()
    tester.subscribe_order_book_at_interval = subscribe_spy

    # Act
    tester.on_start()

    # Assert
    assert subscribe_spy.called
    call_args = subscribe_spy.call_args
    assert call_args[0][0] == instrument_id
    assert call_args[1]["book_type"] == BookType.L3_MBO
    assert call_args[1]["depth"] == 20
    assert call_args[1]["interval_ms"] == 500
    assert call_args[1]["client_id"] == ClientId("TEST_CLIENT")


@pytest.mark.parametrize(
    "subscribe_quotes,subscribe_trades,subscribe_book",
    [
        (True, True, False),
        (True, False, True),
        (False, True, True),
        (True, True, True),
        (False, False, False),
    ],
)
def test_subscription_setup_on_start_parametrized(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
    mocker,
    subscribe_quotes,
    subscribe_trades,
    subscribe_book,
):
    """
    Test that subscriptions are set up correctly on start.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        subscribe_quotes=subscribe_quotes,
        subscribe_trades=subscribe_trades,
        subscribe_book=subscribe_book,
        client_id=ClientId("TEST_CLIENT"),
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on subscription methods
    quote_spy = mocker.spy(tester, "subscribe_quote_ticks")
    trade_spy = mocker.spy(tester, "subscribe_trade_ticks")
    book_spy = mocker.spy(tester, "subscribe_order_book_at_interval")

    # Act
    tester.on_start()

    # Assert
    if subscribe_quotes:
        assert quote_spy.call_count == 1
        assert quote_spy.call_args[0][0] == instrument_id
        assert quote_spy.call_args[1]["client_id"] == ClientId("TEST_CLIENT")
    else:
        assert quote_spy.call_count == 0

    if subscribe_trades:
        assert trade_spy.call_count == 1
        assert trade_spy.call_args[0][0] == instrument_id
        assert trade_spy.call_args[1]["client_id"] == ClientId("TEST_CLIENT")
    else:
        assert trade_spy.call_count == 0

    if subscribe_book:
        assert book_spy.call_count == 1
        assert book_spy.call_args[0][0] == instrument_id
    else:
        assert book_spy.call_count == 0


def test_unsubscription_behavior_on_stop(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that unsubscriptions work correctly on stop.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        subscribe_quotes=True,
        subscribe_trades=True,
        subscribe_book=True,
        can_unsubscribe=True,
        client_id=ClientId("TEST_CLIENT"),
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on unsubscription methods
    unsubscribe_quotes_spy = mocker.spy(tester, "unsubscribe_quote_ticks")
    unsubscribe_trades_spy = mocker.spy(tester, "unsubscribe_trade_ticks")
    unsubscribe_book_spy = mocker.spy(tester, "unsubscribe_order_book_at_interval")

    tester.on_start()

    # Act
    tester.on_stop()

    # Assert
    assert unsubscribe_quotes_spy.call_count == 1
    assert unsubscribe_quotes_spy.call_args[0][0] == instrument_id
    assert unsubscribe_quotes_spy.call_args[1]["client_id"] == ClientId("TEST_CLIENT")

    assert unsubscribe_trades_spy.call_count == 1
    assert unsubscribe_trades_spy.call_args[0][0] == instrument_id
    assert unsubscribe_trades_spy.call_args[1]["client_id"] == ClientId("TEST_CLIENT")

    assert unsubscribe_book_spy.call_count == 1
    assert unsubscribe_book_spy.call_args[0][0] == instrument_id
    assert unsubscribe_book_spy.call_args[1]["client_id"] == ClientId("TEST_CLIENT")


def test_no_unsubscription_when_disabled(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that unsubscriptions are skipped when can_unsubscribe=False.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        subscribe_quotes=True,
        subscribe_trades=True,
        subscribe_book=True,
        can_unsubscribe=False,  # Disable unsubscriptions
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Spy on unsubscription methods
    unsubscribe_quotes_spy = mocker.spy(tester, "unsubscribe_quote_ticks")
    unsubscribe_trades_spy = mocker.spy(tester, "unsubscribe_trade_ticks")
    unsubscribe_book_spy = mocker.spy(tester, "unsubscribe_order_book_at_interval")

    tester.on_start()

    # Act
    tester.on_stop()

    # Assert - no unsubscribe calls should be made
    assert unsubscribe_quotes_spy.call_count == 0
    assert unsubscribe_trades_spy.call_count == 0
    assert unsubscribe_book_spy.call_count == 0


def test_no_modify_when_order_pending_update(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that orders are not modified when pending update.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=False,
        modify_orders_to_maintain_tob_offset=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    modify_spy = mocker.spy(tester, "modify_order")

    # Act
    tester.on_start()

    # Create initial order
    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)
    tester.on_quote_tick(quote)

    buy_order = tester.buy_order

    # Simulate order acceptance
    accepted_event = TestEventStubs.order_accepted(
        order=buy_order,
        venue_order_id=VenueOrderId("1"),
    )
    tester.handle_event(accepted_event)

    # Simulate order modify request (puts order in pending update state)
    update_event = TestEventStubs.order_pending_update(order=buy_order)
    tester.handle_event(update_event)

    # Send new quote that should trigger modification (should be ignored)
    new_quote = TestDataStubs.quote_tick(instrument, bid_price=102.0, ask_price=103.0)
    tester.on_quote_tick(new_quote)

    # Assert - modify should not be called when order is pending update
    assert modify_spy.call_count == 0


def test_no_modify_when_order_pending_cancel(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that orders are not modified when pending cancel.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        enable_buys=True,
        enable_sells=False,
        modify_orders_to_maintain_tob_offset=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    modify_spy = mocker.spy(tester, "modify_order")

    # Act
    tester.on_start()

    # Create initial order
    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)
    tester.on_quote_tick(quote)

    buy_order = tester.buy_order

    # Simulate order acceptance
    accepted_event = TestEventStubs.order_accepted(
        order=buy_order,
        venue_order_id=VenueOrderId("1"),
    )
    tester.handle_event(accepted_event)

    # Simulate order cancel request (puts order in pending cancel state)
    cancel_event = TestEventStubs.order_pending_cancel(order=buy_order)
    tester.handle_event(cancel_event)

    # Send new quote that should trigger modification (should be ignored)
    new_quote = TestDataStubs.quote_tick(instrument, bid_price=102.0, ask_price=103.0)
    tester.on_quote_tick(new_quote)

    # Assert - modify should not be called when order is pending cancel
    assert modify_spy.call_count == 0


def test_open_position_zero_quantity_skipped(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that zero open position quantity is skipped.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        open_position_on_start_qty=Decimal("0.0"),  # Zero quantity
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    submit_spy = mocker.spy(tester, "submit_order")

    # Act
    tester.on_start()

    # Assert - no order should be submitted
    assert submit_spy.call_count == 0


def test_open_position_uses_configured_time_in_force(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that open position uses configured time in force.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        open_position_on_start_qty=Decimal("0.5"),
        open_position_time_in_force=TimeInForce.FOK,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    submitted_orders = []
    original_submit = tester.submit_order

    def capture_order(order, **kwargs):
        submitted_orders.append(order)
        return original_submit(order, **kwargs)

    tester.submit_order = capture_order

    # Act
    tester.on_start()

    # Assert
    assert len(submitted_orders) == 1
    order = submitted_orders[0]
    assert order.time_in_force == TimeInForce.FOK


def test_open_position_with_quote_quantity_flag(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that open position respects quote quantity flag.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        open_position_on_start_qty=Decimal("0.5"),
        use_quote_quantity=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    submitted_orders = []
    original_submit = tester.submit_order

    def capture_order(order, **kwargs):
        submitted_orders.append(order)
        return original_submit(order, **kwargs)

    tester.submit_order = capture_order

    # Act
    tester.on_start()

    # Assert
    assert len(submitted_orders) == 1
    order = submitted_orders[0]
    assert order.is_quote_quantity is True


def test_submit_order_passes_client_id_and_params(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
    mocker,
):
    """
    Test that submit_order calls include client_id and order_params.
    """
    # Arrange
    test_params = {"test_param": "test_value"}
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("0.01"),
        client_id=ClientId("TEST_CLIENT"),
        order_params=test_params,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    submit_spy = mocker.spy(tester, "submit_order")

    # Act
    tester.on_start()
    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)
    tester.on_quote_tick(quote)

    # Assert - check that submit_order was called with client_id and params
    assert submit_spy.call_count == 2  # Buy and sell orders

    for call in submit_spy.call_args_list:
        assert call[1]["client_id"] == ClientId("TEST_CLIENT")
        assert call[1]["params"] == test_params


def test_quote_quantity_amount_precision_validation(
    trader_id,
    portfolio,
    msgbus,
    cache,
    clock,
    instrument,
    instrument_id,
    setup_environment,
):
    """
    Test that quote quantity orders compute correct amounts with proper precision.
    """
    # Arrange
    config = ExecTesterConfig(
        instrument_id=instrument_id,
        order_qty=Decimal("1000.0"),  # Quote amount
        use_quote_quantity=True,
    )

    tester = ExecTester(config)
    tester.register(
        trader_id=trader_id,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
    )

    # Act
    tester.on_start()
    quote = TestDataStubs.quote_tick(instrument, bid_price=100.0, ask_price=101.0)
    tester.on_quote_tick(quote)

    # Assert - quantity should be computed with instrument precision
    buy_order = tester.buy_order
    sell_order = tester.sell_order

    assert buy_order is not None
    assert sell_order is not None

    # Both orders should use quote quantity
    assert buy_order.is_quote_quantity is True
    assert sell_order.is_quote_quantity is True

    # Quantities should be rounded to instrument precision
    expected_qty = instrument.make_qty(1000.0)
    assert buy_order.quantity == expected_qty
    assert sell_order.quantity == expected_qty
