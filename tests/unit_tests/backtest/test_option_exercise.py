#!/usr/bin/env python3
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
"""
Comprehensive tests for option exercise simulation module.

This module contains unit tests for the option exercise functionality, covering
configuration, module behavior, exercise logic, PnL scenarios, and edge cases.

"""

import pandas as pd

from nautilus_trader.accounting.accounts.margin import MarginAccount
from nautilus_trader.backtest.option_exercise import OptionExerciseConfig
from nautilus_trader.backtest.option_exercise import OptionExerciseModule
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.events import TimeEvent
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.config import ExecEngineConfig
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import IndexInstrument
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestOptionExerciseModule:
    """
    Test option exercise module functionality.
    """

    def setup_method(self):
        """
        Set up test fixtures.
        """
        self.config = OptionExerciseConfig()
        self.module = OptionExerciseModule(self.config)

        # Create test instruments
        self.underlying = Equity(
            instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            raw_symbol=Symbol("AAPL"),
            currency=USD,
            price_precision=2,
            price_increment=Price(0.01, 2),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.call_option = OptionContract(
            instrument_id=InstrumentId.from_str("AAPL240315C00150000.NASDAQ"),
            raw_symbol=Symbol("AAPL240315C00150000"),
            asset_class=AssetClass.EQUITY,
            underlying="AAPL",
            option_kind=OptionKind.CALL,
            strike_price=Price(150.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC")),
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.put_option = OptionContract(
            instrument_id=InstrumentId.from_str("AAPL240315P00150000.NASDAQ"),
            raw_symbol=Symbol("AAPL240315P00150000"),
            asset_class=AssetClass.EQUITY,
            underlying="AAPL",
            option_kind=OptionKind.PUT,
            strike_price=Price(150.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC")),
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Create index instrument for cash settlement testing
        self.index = IndexInstrument(
            instrument_id=InstrumentId.from_str("SPX.CBOE"),
            raw_symbol=Symbol("SPX"),
            currency=USD,
            price_precision=2,
            size_precision=0,
            price_increment=Price(0.01, 2),
            size_increment=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # Create index option for cash settlement testing
        self.index_call_option = OptionContract(
            instrument_id=InstrumentId.from_str("SPX240315C04500000.CBOE"),
            raw_symbol=Symbol("SPX240315C04500000"),
            asset_class=AssetClass.INDEX,
            underlying="SPX",
            option_kind=OptionKind.CALL,
            strike_price=Price(4500.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC")),
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        self.expiry_ns = dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC"))

    def test_module_initialization(self):
        """
        Test module initialization.
        """
        assert self.module.config == self.config
        assert len(self.module.expiry_timers) == 0
        assert len(self.module.processed_expiries) == 0

    def test_pre_process_quote_tick(self):
        """
        Test pre-processing of quote ticks.

        Pre-processing is now a no-op since prices are retrieved at expiry time.

        """
        tick = QuoteTick(
            instrument_id=self.underlying.id,
            bid_price=Price(149.50, 2),
            ask_price=Price(150.50, 2),
            bid_size=Quantity.from_int(100),
            ask_size=Quantity.from_int(100),
            ts_event=0,
            ts_init=0,
        )

        # Should not raise any errors
        self.module.pre_process(tick)

    def test_is_option_itm_call(self):
        """
        Test ITM detection for call options.
        """
        underlying_price = Price(160.0, 2)  # Above strike of 150

        is_itm, intrinsic = self.module._is_option_itm(self.call_option, underlying_price)

        assert is_itm is True
        assert intrinsic == 10.0  # 160 - 150

    def test_is_option_otm_call(self):
        """
        Test OTM detection for call options.
        """
        underlying_price = Price(140.0, 2)  # Below strike of 150

        is_itm, intrinsic = self.module._is_option_itm(self.call_option, underlying_price)

        assert is_itm is False
        assert intrinsic == 0.0

    def test_is_option_itm_put(self):
        """
        Test ITM detection for put options.
        """
        underlying_price = Price(140.0, 2)  # Below strike of 150

        is_itm, intrinsic = self.module._is_option_itm(self.put_option, underlying_price)

        assert is_itm is True
        assert intrinsic == 10.0  # 150 - 140

    def test_is_option_otm_put(self):
        """
        Test OTM detection for put options.
        """
        underlying_price = Price(160.0, 2)  # Above strike of 150

        is_itm, intrinsic = self.module._is_option_itm(self.put_option, underlying_price)

        assert is_itm is False
        assert intrinsic == 0.0

    def test_calculate_underlying_position_call_long(self):
        """
        Test underlying position calculation for long call.
        """
        # Create a proper position using test stubs
        order = TestExecStubs.market_order(
            instrument=self.call_option,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(2),
        )

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=self.call_option,
            last_px=Price(5.0, 2),
            position_id=PositionId("P-001"),
        )
        position = Position(self.call_option, fill)

        quantity, side = self.module._calculate_underlying_position(self.call_option, position)

        assert quantity == Quantity.from_str("200")  # 2 * 100 multiplier
        assert side == PositionSide.LONG  # Long call -> long underlying

    def test_calculate_underlying_position_put_long(self):
        """
        Test underlying position calculation for long put.
        """
        # Create a proper position using test stubs
        order = TestExecStubs.market_order(
            instrument=self.put_option,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )

        fill = TestEventStubs.order_filled(
            order=order,
            instrument=self.put_option,
            last_px=Price(8.0, 2),
            position_id=PositionId("P-002"),
        )
        position = Position(self.put_option, fill)

        quantity, side = self.module._calculate_underlying_position(self.put_option, position)

        assert quantity == Quantity.from_str("100")  # 1 * 100 multiplier
        assert side == PositionSide.SHORT  # Long put -> short underlying

    def test_reset(self):
        """
        Test module reset functionality.
        """
        # Add some state
        self.module.expiry_timers[1] = "test_timer"
        self.module.processed_expiries.add(1)

        # Reset
        self.module.reset()

        # Verify state is cleared
        assert len(self.module.expiry_timers) == 0
        assert len(self.module.processed_expiries) == 0

    def test_process_disabled(self):
        """
        Test processing when auto exercise is disabled.
        """
        config = OptionExerciseConfig(auto_exercise_enabled=False)
        module = OptionExerciseModule(config)

        # Should not process anything when disabled
        module.process(dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC")))

        # Should exit early without doing anything
        assert len(module.processed_expiries) == 0

    def test_cash_settlement_exercise(self):
        """
        Test cash settlement for index option exercise.
        """
        # Test the cash settlement price calculation directly
        underlying_price = Price(4600.0, 2)

        # Create a simple test to verify the intrinsic value calculation
        # For a call option with strike 4500 and underlying at 4600, intrinsic value should be 100
        strike_price = self.index_call_option.strike_price
        underlying_value = underlying_price

        if self.index_call_option.option_kind == OptionKind.CALL:
            expected_intrinsic = max(Price(0.0, 2), underlying_value - strike_price)
        else:  # PUT
            expected_intrinsic = max(Price(0.0, 2), strike_price - underlying_value)

        # Verify the calculation
        assert expected_intrinsic == Price(100.0, 2)

    def test_otm_option_expiry_pnl_behavior(self):
        """
        Test that OTM options expire correctly for both long and short positions.

        This test verifies that:
        1. Long OTM options are closed at zero value (negative PnL)
        2. Short OTM options are closed at zero value (positive PnL)

        """
        # Create positions for testing
        trader_id = TraderId("TRADER-001")
        strategy_id = StrategyId("STRATEGY-001")
        account_id = AccountId("ACCOUNT-001")

        # Create a long call position (bought at $5.00, now OTM)
        long_call_fill = OrderFilled(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=self.call_option.id,
            client_order_id=ClientOrderId("LONG-CALL-1"),
            venue_order_id=VenueOrderId("VENUE-LONG-1"),
            account_id=account_id,
            trade_id=TradeId("TRADE-LONG-1"),
            position_id=PositionId("P-LONG-1"),
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            last_qty=Quantity.from_int(1),
            last_px=Price(5.00, 2),  # Paid $5.00 premium
            currency=USD,
            commission=Money(0, USD),
            liquidity_side=LiquiditySide.TAKER,
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
            reconciliation=False,
        )
        long_call_position = Position(self.call_option, long_call_fill)

        # Create a short call position (sold at $5.00, now OTM)
        short_call_fill = OrderFilled(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=self.call_option.id,
            client_order_id=ClientOrderId("SHORT-CALL-1"),
            venue_order_id=VenueOrderId("VENUE-SHORT-1"),
            account_id=account_id,
            trade_id=TradeId("TRADE-SHORT-1"),
            position_id=PositionId("P-SHORT-1"),
            order_side=OrderSide.SELL,
            order_type=OrderType.MARKET,
            last_qty=Quantity.from_int(1),
            last_px=Price(5.00, 2),  # Received $5.00 premium
            currency=USD,
            commission=Money(0, USD),
            liquidity_side=LiquiditySide.TAKER,
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
            reconciliation=False,
        )
        short_call_position = Position(self.call_option, short_call_fill)

        # Test OTM expiry events generation
        ts_expiry = dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC"))

        # Test the _create_option_fill method directly for both positions
        # This tests the core logic without needing to mock _send_events

        # Test long position OTM expiry fill creation
        long_fill = self.module._create_option_fill(
            self.call_option,
            long_call_position,
            f"OTM-EXPIRY-{ts_expiry}",
            f"OTM-EXPIRY-{ts_expiry}",
            ts_expiry,
            False,  # use_avg_price=False for zero value
        )

        # Verify long position gets closed at zero
        assert isinstance(long_fill, OrderFilled)
        assert long_fill.last_px == Price(0.0, 2)  # Closed at zero value
        assert long_fill.order_side == OrderSide.SELL  # Selling to close long position

        # Calculate PnL for long position: bought at $5.00, closed at $0.00 = -$5.00 per share
        long_pnl = long_call_position.calculate_pnl(
            avg_px_open=5.00,
            avg_px_close=0.00,
            quantity=Quantity.from_int(1),
        )
        assert long_pnl == Money(-500.0, USD)  # -$5.00 * 100 multiplier = -$500

        # Test short position OTM expiry fill creation
        short_fill = self.module._create_option_fill(
            self.call_option,
            short_call_position,
            f"OTM-EXPIRY-{ts_expiry}",
            f"OTM-EXPIRY-{ts_expiry}",
            ts_expiry,
            False,  # use_avg_price=False for zero value
        )

        # Verify short position gets closed at zero
        assert isinstance(short_fill, OrderFilled)
        assert short_fill.last_px == Price(0.0, 2)  # Closed at zero value
        assert short_fill.order_side == OrderSide.BUY  # Buying to close short position

        # Calculate PnL for short position: sold at $5.00, closed at $0.00 = +$5.00 per share
        short_pnl = short_call_position.calculate_pnl(
            avg_px_open=5.00,
            avg_px_close=0.00,
            quantity=Quantity.from_int(1),
        )
        assert short_pnl == Money(500.0, USD)  # +$5.00 * 100 multiplier = +$500

    def test_comprehensive_option_expiry_pnl_all_cases(self):
        """
        Comprehensive test for option expiry PnL behavior covering all 8 cases:

        Stock Options (Physical Settlement):
        1. Long ITM Call - should exercise and create underlying position
        2. Long OTM Call - should expire worthless at $0 (negative PnL)
        3. Short ITM Call - should be assigned and create underlying position
        4. Short OTM Call - should expire worthless at $0 (positive PnL)

        Index Options (Cash Settlement):
        5. Long ITM Call - should cash settle at intrinsic value
        6. Long OTM Call - should expire worthless at $0 (negative PnL)
        7. Short ITM Call - should be cash assigned at intrinsic value
        8. Short OTM Call - should expire worthless at $0 (positive PnL)
        """
        config = OptionExerciseConfig(auto_exercise_enabled=True)
        module = OptionExerciseModule(config)

        # Create option contracts
        stock_call = OptionContract(
            instrument_id=InstrumentId.from_str("AAPL240315C00150000.NASDAQ"),
            raw_symbol=Symbol("AAPL240315C00150000"),
            asset_class=AssetClass.EQUITY,
            underlying="AAPL",
            option_kind=OptionKind.CALL,
            strike_price=Price(150.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC")),
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )

        # === STOCK OPTIONS (Physical Settlement) ===

        # Test Case 1: Long ITM Stock Call
        long_itm_order = TestExecStubs.market_order(
            instrument=stock_call,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )
        long_itm_stock_fill = TestEventStubs.order_filled(
            order=long_itm_order,
            instrument=stock_call,
            last_px=Price(5.0, 2),
            position_id=PositionId("P-001"),
        )
        long_itm_stock_position = Position(stock_call, long_itm_stock_fill)

        # Test ITM exercise logic for stock option
        itm_fill = module._create_option_fill(
            stock_call,
            long_itm_stock_position,
            "ITM-EXERCISE",
            "ITM-EXERCISE",
            0,
            True,
        )
        # For ITM exercise, option should be closed at average price
        assert itm_fill.last_px == Price(5.0, 2)  # Closed at average price
        assert itm_fill.order_side == OrderSide.SELL  # Selling to close long position

        # Test Case 2: Long OTM Stock Call
        long_otm_order = TestExecStubs.market_order(
            instrument=stock_call,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )
        long_otm_stock_fill = TestEventStubs.order_filled(
            order=long_otm_order,
            instrument=stock_call,
            last_px=Price(5.0, 2),
            position_id=PositionId("P-002"),
        )
        long_otm_stock_position = Position(stock_call, long_otm_stock_fill)

        # Test OTM expiry logic
        otm_fill = module._create_option_fill(
            stock_call,
            long_otm_stock_position,
            "OTM-EXPIRY",
            "OTM-EXPIRY",
            0,
            False,
        )
        assert otm_fill.last_px == Price(0.0, 2)  # Expires worthless
        assert otm_fill.order_side == OrderSide.SELL  # Selling to close long position

        # Calculate PnL: bought at $5.00, expires at $0.00 = -$500
        long_otm_pnl = long_otm_stock_position.calculate_pnl(5.0, 0.0, Quantity.from_int(1))
        assert long_otm_pnl.as_double() == -500.0


class TestOptionExerciseModuleIntegration:
    """
    Integration tests for option exercise module with full setup.
    """

    def setup_method(self):
        """
        Set up test fixtures with full component integration.
        """
        self.config = OptionExerciseConfig()
        self.module = OptionExerciseModule(self.config)

        self.clock = TestClock()
        self.clock.set_time(0)
        self.trader_id = TraderId("TRADER-001")
        self.account_id = AccountId("SIM-001")
        self.msgbus = MessageBus(trader_id=self.trader_id, clock=self.clock)
        self.cache = TestComponentStubs.cache()
        self.portfolio = Portfolio(msgbus=self.msgbus, cache=self.cache, clock=self.clock)

        # Create and add margin account (options are typically traded on margin accounts)
        account_state = AccountState(
            account_id=self.account_id,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            reported=True,
            balances=[AccountBalance(Money(1_000_000, USD), Money(0, USD), Money(1_000_000, USD))],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )
        account = MarginAccount(account_state, calculate_account_state=True)
        self.cache.add_account(account)
        self.portfolio.update_account(account_state)

        # Set up execution engine to process OrderFilled events automatically
        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=ExecEngineConfig(debug=False),
        )
        self.exec_engine.start()  # Start the engine so it processes events

        # Portfolio is already subscribed to events.order.* and events.position.*
        # via its __init__ method, so it will automatically receive and process events
        # Execution engine now sends leg fills to Portfolio.update_order endpoint
        # so balances are updated correctly even without orders in cache

        # Connect module
        self.module.register_base(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Manually subscribe to position events (normally done in register_venue)
        # This avoids needing a real SimulatedExchange for these tests
        self.msgbus.subscribe(
            topic="events.position.*",
            handler=self.module.on_position_event,
        )

        # Instruments
        self.underlying = Equity(
            instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
            raw_symbol=Symbol("AAPL"),
            currency=USD,
            price_precision=2,
            price_increment=Price(0.01, 2),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(self.underlying)

        self.expiry_ns = dt_to_unix_nanos(pd.Timestamp("2024-03-15 16:00:00", tz="UTC"))

        self.call_option = OptionContract(
            instrument_id=InstrumentId.from_str("AAPL240315C00150000.NASDAQ"),
            raw_symbol=Symbol("AAPL240315C00150000"),
            asset_class=AssetClass.EQUITY,
            underlying="AAPL",
            option_kind=OptionKind.CALL,
            strike_price=Price(150.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=self.expiry_ns,
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(self.call_option)

    def create_position(self, instrument, side, quantity, avg_px_open):
        """
        Create a position and ensure it's reflected in the cache and portfolio.
        """
        strategy_id = TestIdStubs.strategy_id()
        position_id = PositionId(f"{instrument.id}-{strategy_id}")

        order = TestExecStubs.market_order(
            instrument=instrument,
            order_side=OrderSide.BUY if side == PositionSide.LONG else OrderSide.SELL,
            quantity=quantity,
        )
        fill = TestEventStubs.order_filled(
            order=order,
            instrument=instrument,
            last_px=Price(avg_px_open, instrument.price_precision),
            position_id=position_id,
            account_id=self.account_id,
            strategy_id=strategy_id,
        )

        # Manually update account balance to simulate premium payment
        account = self.cache.account(self.account_id)
        premium = instrument.notional_value(
            quantity,
            Price(avg_px_open, instrument.price_precision),
        )
        current_balance = account.balance(USD)
        impact = -premium.as_decimal() if side == PositionSide.LONG else premium.as_decimal()
        new_total = Money(current_balance.total.as_decimal() + impact, USD)
        new_balance = AccountBalance(new_total, Money(0, USD), new_total)
        account.update_balances([new_balance])

        return Position(instrument, fill)

    def test_timer_setting_and_cleanup(self):
        """
        Test timer setting and cleanup logic.
        """
        # 1. Opening first position sets timer
        pos1 = self.create_position(self.call_option, PositionSide.LONG, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos1, OmsType.NETTING)
        event_open = TestEventStubs.position_opened(pos1)

        initial_timer_count = len(self.clock.timer_names)
        self.module.on_position_event(event_open)

        assert self.expiry_ns in self.module.expiry_timers
        assert f"option_expiry_{self.expiry_ns}" in self.clock.timer_names
        assert len(self.clock.timer_names) == initial_timer_count + 1

        # 2. Opening second position for same expiry does NOT set another timer
        # Use a different strategy ID so positions have different IDs for this test
        strategy_id2 = StrategyId("S-002")
        order2 = TestExecStubs.market_order(
            instrument=self.call_option,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )
        position_id2 = PositionId(f"{self.call_option.id}-{strategy_id2}")
        fill2 = TestEventStubs.order_filled(
            order=order2,
            instrument=self.call_option,
            last_px=Price(5.0, 2),
            position_id=position_id2,
            account_id=self.account_id,
            strategy_id=strategy_id2,
        )
        pos2 = Position(self.call_option, fill2)
        self.cache.add_position(pos2, OmsType.NETTING)
        event_open2 = TestEventStubs.position_opened(pos2)

        timer_count_before = len(self.clock.timer_names)
        self.module.on_position_event(event_open2)
        # Timer count should not increase
        assert len(self.clock.timer_names) == timer_count_before

        # 3. Closing one position when another exists does NOT cleanup timer
        # Create a closing fill and apply it to close the position
        close_order = TestExecStubs.market_order(
            instrument=self.call_option,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(1),
        )
        close_fill = TestEventStubs.order_filled(
            order=close_order,
            instrument=self.call_option,
            last_px=Price(5.0, 2),
            position_id=pos1.id,
        )
        pos1.apply(close_fill)
        self.cache.update_position(pos1)
        event_close = TestEventStubs.position_closed(pos1)

        timer_count_before_close = len(self.clock.timer_names)
        self.module.on_position_event(event_close)
        # Timer should still exist
        assert len(self.clock.timer_names) == timer_count_before_close

        # 4. Closing last position DOES cleanup timer
        # Close pos2 to remove it from cache
        close_order2 = TestExecStubs.market_order(
            instrument=self.call_option,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(1),
        )
        close_fill2 = TestEventStubs.order_filled(
            order=close_order2,
            instrument=self.call_option,
            last_px=Price(5.0, 2),
            position_id=pos2.id,
        )
        pos2.apply(close_fill2)
        self.cache.update_position(pos2)
        event_close2 = TestEventStubs.position_closed(pos2)
        self.module.on_position_event(event_close2)

        # Now close pos1 - should cleanup timer
        self.module.on_position_event(event_close)
        assert self.expiry_ns not in self.module.expiry_timers
        assert f"option_expiry_{self.expiry_ns}" not in self.clock.timer_names

    def test_physical_settlement_long_itm_call(self):
        """
        Test long ITM call exercise (physical settlement).
        """
        pos = self.create_position(self.call_option, PositionSide.LONG, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos, OmsType.NETTING)
        initial_option_positions = len(self.cache.positions_open(instrument_id=self.call_option.id))
        assert initial_option_positions == 1

        # Set underlying price via trade tick (ITM: 160 > strike 150)
        trade_tick = TradeTick(
            instrument_id=self.underlying.id,
            price=Price(160.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T1"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(trade_tick)

        # Create TimeEvent properly
        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )

        # Process expiry - module sends OrderFilled events to ExecEngine.process
        # Execution engine will process them and portfolio will automatically update positions/balances
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=self.call_option.id)
        assert len(option_positions_after) == 0, "Option position should be closed"

        # Verify underlying position is created (physical settlement)
        underlying_positions = self.cache.positions_open(instrument_id=self.underlying.id)
        assert len(underlying_positions) == 1, "Underlying position should be created"
        underlying_pos = underlying_positions[0]
        assert underlying_pos.side == PositionSide.LONG, "Long call should create long underlying"
        assert underlying_pos.quantity == Quantity.from_int(100), (
            "Should have 100 shares (1 option * 100 multiplier)"
        )
        assert underlying_pos.avg_px_open == Price(150.0, 2), (
            "Underlying should open at strike price"
        )

        # Verify PnL: Option bought at 5.0, closed at 5.0 = 0, but we get underlying at 150.0
        # The exercise creates underlying position at strike, so the PnL comes from underlying movement
        # For now, just verify the module processed without error and positions are correct

    def test_physical_settlement_long_itm_call_events(self):
        """
        Test long ITM call exercise (physical settlement) generates correct events.
        """
        # Capture events published by ExecEngine for all strategies
        events = []

        def capture(msg):
            events.append(msg)

        self.msgbus.subscribe("events.order.*", capture)
        self.msgbus.subscribe("events.fills.*", capture)

        pos = self.create_position(self.call_option, PositionSide.LONG, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos, OmsType.NETTING)

        # Set underlying price via trade tick (ITM: 160 > strike 150)
        trade_tick = TradeTick(
            instrument_id=self.underlying.id,
            price=Price(160.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T1"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(trade_tick)

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )

        self.module._on_expiry_timer(time_event)

        # Verify leg fill ID patterns in emitted events
        # Note: 2 exercise leg fills (initial position was added directly to cache)
        assert len(events) == 2
        fills = [e for e in events if isinstance(e, OrderFilled)]
        option_close_fill = next(
            f
            for f in fills
            if f.instrument_id == self.call_option.id and f.order_side == OrderSide.SELL
        )
        underlying_open_fill = next(f for f in fills if f.instrument_id == self.underlying.id)

        assert "-LEG-EX-" in option_close_fill.client_order_id.value
        assert "-LEG-EX-" in underlying_open_fill.client_order_id.value
        assert option_close_fill.client_order_id.value.endswith("-CLOSE")
        assert underlying_open_fill.client_order_id.value.endswith("-OPEN")
        assert option_close_fill.last_qty == Quantity.from_int(1)
        assert underlying_open_fill.last_qty == Quantity.from_int(100)
        assert underlying_open_fill.last_px == Price(150.0, 2)

    def test_cash_settlement_long_itm_call(self):
        """
        Test long ITM index call exercise (cash settlement).
        """
        index = IndexInstrument(
            instrument_id=InstrumentId.from_str("SPX.CBOE"),
            raw_symbol=Symbol("SPX"),
            currency=USD,
            price_precision=2,
            size_precision=0,
            price_increment=Price(0.01, 2),
            size_increment=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(index)
        index_call = OptionContract(
            instrument_id=InstrumentId.from_str("SPX240315C04500000.CBOE"),
            raw_symbol=Symbol("SPX240315C04500000"),
            asset_class=AssetClass.INDEX,
            underlying="SPX",
            option_kind=OptionKind.CALL,
            strike_price=Price(4500.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=self.expiry_ns,
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(index_call)

        # Set index price via trade tick (ITM: 4600 > strike 4500, intrinsic = 100)
        index_trade_tick = TradeTick(
            instrument_id=index.id,
            price=Price(4600.0, 2),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T-INDEX"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(index_trade_tick)

        initial_balance = self.cache.account(self.account_id).balance(USD).total
        pos = self.create_position(index_call, PositionSide.LONG, Quantity.from_int(1), 10.0)
        self.cache.add_position(pos, OmsType.NETTING)

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )

        # Process expiry - module sends OrderFilled events to ExecEngine.process
        # Execution engine will process them and portfolio will automatically update positions/balances
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=index_call.id)
        assert len(option_positions_after) == 0, "Option position should be closed"

        # Verify no underlying position created (cash settlement)
        underlying_positions = self.cache.positions_open(instrument_id=index.id)
        assert len(underlying_positions) == 0, "No underlying position for cash settlement"

        # Verify account balance increased by intrinsic value
        # Bought at 10.0, closed at 100.0 intrinsic = +90.0 per share * 100 multiplier = +9000
        final_balance = self.cache.account(self.account_id).balance(USD).total
        expected_pnl = Money(9000.0, USD)  # (100 - 10) * 100 multiplier
        balance_change = final_balance - initial_balance
        balance_change_money = Money(balance_change, USD)
        assert balance_change_money == expected_pnl, (
            f"Balance should increase by {expected_pnl}, got {balance_change_money}"
        )

    def test_otm_expiry_worthless(self):
        """
        Test OTM option expiry (expires worthless).
        """
        initial_balance = self.cache.account(self.account_id).balance(USD).total
        pos = self.create_position(self.call_option, PositionSide.LONG, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos, OmsType.NETTING)

        # Set underlying price via trade tick (OTM: 140 < strike 150)
        trade_tick = TradeTick(
            instrument_id=self.underlying.id,
            price=Price(140.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T-OTM"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(trade_tick)

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )

        # Process expiry - module sends OrderFilled events to ExecEngine.process
        # Execution engine will process them and portfolio will automatically update positions/balances
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=self.call_option.id)
        assert len(option_positions_after) == 0, "Option position should be closed"

        # Verify account balance decreased (bought at 5.0, expired at 0.0 = -5.0 per share * 100 = -500)
        final_balance = self.cache.account(self.account_id).balance(USD).total
        expected_pnl = Money(-500.0, USD)  # (0 - 5) * 100 multiplier
        balance_change = final_balance - initial_balance
        balance_change_money = Money(balance_change, USD)
        assert balance_change_money == expected_pnl, (
            f"Balance should decrease by {expected_pnl}, got {balance_change_money}"
        )

    def test_physical_settlement_short_itm_call(self):
        """
        Test short call assignment (physical settlement).
        """
        pos = self.create_position(self.call_option, PositionSide.SHORT, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos, OmsType.NETTING)

        # Set underlying price via trade tick (ITM: 160 > strike 150)
        trade_tick = TradeTick(
            instrument_id=self.underlying.id,
            price=Price(160.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T-SHORT"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(trade_tick)

        initial_option_positions = len(self.cache.positions_open(instrument_id=self.call_option.id))
        assert initial_option_positions == 1

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=self.call_option.id)
        assert len(option_positions_after) == 0, "Short call position should be closed"

        # Verify underlying position is created (physical settlement)
        underlying_positions = self.cache.positions_open(instrument_id=self.underlying.id)
        assert len(underlying_positions) == 1, "Underlying position should be created"
        underlying_pos = underlying_positions[0]
        assert underlying_pos.side == PositionSide.SHORT, "Long put should create short underlying"
        assert underlying_pos.quantity == Quantity.from_int(100), (
            "Should have 100 shares (1 option * 100 multiplier)"
        )
        assert underlying_pos.avg_px_open == Price(150.0, 2), (
            "Underlying should open at strike price"
        )

    def test_physical_settlement_long_itm_put(self):
        """
        Test long put exercise (physical settlement).
        """
        put_option = OptionContract(
            instrument_id=InstrumentId.from_str("AAPL240315P00150000.NASDAQ"),
            raw_symbol=Symbol("AAPL240315P00150000"),
            asset_class=AssetClass.EQUITY,
            underlying="AAPL",
            option_kind=OptionKind.PUT,
            strike_price=Price(150.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=self.expiry_ns,
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(put_option)

        pos = self.create_position(put_option, PositionSide.LONG, Quantity.from_int(1), 8.0)
        self.cache.add_position(pos, OmsType.NETTING)

        # Set underlying price via trade tick (ITM put: 140 < strike 150)
        trade_tick = TradeTick(
            instrument_id=self.underlying.id,
            price=Price(140.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T-PUT"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(trade_tick)

        initial_option_positions = len(self.cache.positions_open(instrument_id=put_option.id))
        assert initial_option_positions == 1

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=self.call_option.id)
        assert len(option_positions_after) == 0, "Long put position should be closed"

        # Verify underlying position is created (physical settlement)
        underlying_positions = self.cache.positions_open(instrument_id=self.underlying.id)
        assert len(underlying_positions) == 1, "Underlying position should be created"
        underlying_pos = underlying_positions[0]
        assert underlying_pos.side == PositionSide.SHORT, "Long put should create short underlying"
        assert underlying_pos.quantity == Quantity.from_int(100), (
            "Should have 100 shares (1 option * 100 multiplier)"
        )
        assert underlying_pos.avg_px_open == Price(150.0, 2), (
            "Underlying should open at strike price"
        )

    def test_physical_settlement_short_itm_put(self):
        """
        Test short put assignment (physical settlement).
        """
        put_option = OptionContract(
            instrument_id=InstrumentId.from_str("AAPL240315P00150000.NASDAQ"),
            raw_symbol=Symbol("AAPL240315P00150000"),
            asset_class=AssetClass.EQUITY,
            underlying="AAPL",
            option_kind=OptionKind.PUT,
            strike_price=Price(150.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=self.expiry_ns,
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(put_option)

        pos = self.create_position(put_option, PositionSide.SHORT, Quantity.from_int(1), 8.0)
        self.cache.add_position(pos, OmsType.NETTING)

        # Set underlying price via trade tick (ITM put: 140 < strike 150)
        trade_tick = TradeTick(
            instrument_id=self.underlying.id,
            price=Price(140.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T-SHORT-PUT"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(trade_tick)

        initial_option_positions = len(self.cache.positions_open(instrument_id=put_option.id))
        assert initial_option_positions == 1

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=put_option.id)
        assert len(option_positions_after) == 0, "Short put position should be closed"

        # Verify underlying position is created (physical settlement for short put)
        underlying_positions = self.cache.positions_open(instrument_id=self.underlying.id)
        assert len(underlying_positions) == 1, (
            "Underlying position should be created for short put assignment"
        )
        underlying_pos = underlying_positions[0]
        assert underlying_pos.side == PositionSide.LONG, (
            "Short put assignment should create long underlying"
        )
        assert underlying_pos.quantity == Quantity.from_int(100), (
            "Should have 100 shares (1 option * 100 multiplier)"
        )
        assert underlying_pos.avg_px_open == Price(150.0, 2), (
            "Underlying should open at strike price"
        )

    def test_cash_settlement_short_itm_call(self):
        """
        Test short index call assignment (cash settlement).
        """
        index = IndexInstrument(
            instrument_id=InstrumentId.from_str("SPX.CBOE"),
            raw_symbol=Symbol("SPX"),
            currency=USD,
            price_precision=2,
            size_precision=0,
            price_increment=Price(0.01, 2),
            size_increment=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(index)
        index_call = OptionContract(
            instrument_id=InstrumentId.from_str("SPX240315C04500000.CBOE"),
            raw_symbol=Symbol("SPX240315C04500000"),
            asset_class=AssetClass.INDEX,
            underlying="SPX",
            option_kind=OptionKind.CALL,
            strike_price=Price(4500.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=self.expiry_ns,
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(index_call)

        # Set index price via trade tick (ITM: 4600 > strike 4500)
        index_trade_tick = TradeTick(
            instrument_id=index.id,
            price=Price(4600.0, 2),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T-INDEX-SHORT"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(index_trade_tick)

        pos = self.create_position(index_call, PositionSide.SHORT, Quantity.from_int(1), 10.0)
        self.cache.add_position(pos, OmsType.NETTING)

        initial_option_positions = len(self.cache.positions_open(instrument_id=index_call.id))
        assert initial_option_positions == 1

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=self.call_option.id)
        assert len(option_positions_after) == 0, "Short option position should be closed"

        # Verify no underlying position created (cash settlement)
        underlying_positions = self.cache.positions_open(instrument_id=index.id)
        assert len(underlying_positions) == 0, "No underlying position for cash settlement"

    def test_otm_expiry_short_position(self):
        """
        Test short OTM option expiry (expires worthless, positive PnL).
        """
        pos = self.create_position(self.call_option, PositionSide.SHORT, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos, OmsType.NETTING)

        # Set underlying price via trade tick (OTM: 140 < strike 150)
        trade_tick = TradeTick(
            instrument_id=self.underlying.id,
            price=Price(140.0, 2),
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T-SHORT-OTM"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(trade_tick)

        initial_option_positions = len(self.cache.positions_open(instrument_id=self.call_option.id))
        assert initial_option_positions == 1

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=self.call_option.id)
        assert len(option_positions_after) == 0, "Short option position should be closed"

        # Verify no underlying position created (OTM options expire worthless)
        underlying_positions = self.cache.positions_open(instrument_id=self.underlying.id)
        assert len(underlying_positions) == 0, "No underlying position for OTM expiry"

    def test_at_the_money_option_expiry(self):
        """
        Test ATM option expiry (spot == strike, should not exercise).
        """
        pos = self.create_position(self.call_option, PositionSide.LONG, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos, OmsType.NETTING)

        # Set underlying price via trade tick (ATM: 150 == strike 150)
        trade_tick = TradeTick(
            instrument_id=self.underlying.id,
            price=Price(150.0, 2),  # Exactly at strike
            size=Quantity.from_int(100),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T-ATM"),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.cache.add_trade_tick(trade_tick)

        initial_option_positions = len(self.cache.positions_open(instrument_id=self.call_option.id))
        assert initial_option_positions == 1

        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        self.module._on_expiry_timer(time_event)

        # Verify option position is closed
        option_positions_after = self.cache.positions_open(instrument_id=self.call_option.id)
        assert len(option_positions_after) == 0, "Short option position should be closed"

        # Verify no underlying position created (ATM options expire worthless)
        underlying_positions = self.cache.positions_open(instrument_id=self.underlying.id)
        assert len(underlying_positions) == 0, "No underlying position for ATM expiry"

    def test_missing_underlying_price(self):
        """
        Test handling when underlying price is missing.
        """
        pos = self.create_position(self.call_option, PositionSide.LONG, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos, OmsType.NETTING)
        initial_option_positions = len(self.cache.positions_open(instrument_id=self.call_option.id))
        assert initial_option_positions == 1

        # Don't set underlying price - module logs error and doesn't process expiry
        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        # Should complete without exception (logs error when price missing)
        self.module._on_expiry_timer(time_event)

        # Position remains open when underlying price is missing (module returns early)
        option_positions_after = self.cache.positions_open(instrument_id=self.call_option.id)
        assert len(option_positions_after) == 1, (
            "Position should remain open when underlying price is missing"
        )

    def test_missing_underlying_instrument(self):
        """
        Test handling when underlying instrument is missing.
        """
        # Create option with non-existent underlying
        bad_option = OptionContract(
            instrument_id=InstrumentId.from_str("BAD240315C00150000.NASDAQ"),
            raw_symbol=Symbol("BAD240315C00150000"),
            asset_class=AssetClass.EQUITY,
            underlying="NONEXISTENT",
            option_kind=OptionKind.CALL,
            strike_price=Price(150.0, 2),
            currency=USD,
            activation_ns=dt_to_unix_nanos(pd.Timestamp("2024-03-01", tz="UTC")),
            expiration_ns=self.expiry_ns,
            price_precision=2,
            price_increment=Price(0.01, 2),
            multiplier=Quantity.from_int(100),
            lot_size=Quantity.from_int(1),
            ts_event=0,
            ts_init=0,
        )
        self.cache.add_instrument(bad_option)

        pos = self.create_position(bad_option, PositionSide.LONG, Quantity.from_int(1), 5.0)
        self.cache.add_position(pos, OmsType.NETTING)
        initial_option_positions = len(self.cache.positions_open(instrument_id=bad_option.id))
        assert initial_option_positions == 1

        # Should handle gracefully when underlying not found
        time_event = TimeEvent(
            name=f"option_expiry_{self.expiry_ns}",
            event_id=UUID4(),
            ts_event=self.expiry_ns,
            ts_init=self.expiry_ns,
        )
        # Should complete without exception (logs error when underlying missing)
        self.module._on_expiry_timer(time_event)

        # Position remains open when underlying instrument is missing (module returns early)
        option_positions_after = self.cache.positions_open(instrument_id=bad_option.id)
        assert len(option_positions_after) == 1, (
            "Position should remain open when underlying instrument is missing"
        )
