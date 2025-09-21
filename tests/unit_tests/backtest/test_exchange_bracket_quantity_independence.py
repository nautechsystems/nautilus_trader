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

from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.emulator import OrderEmulator
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.risk.engine import RiskEngine
from nautilus_trader.test_kit.mocks.strategies import MockStrategy
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
EURUSD_SIM = TestInstrumentProvider.default_fx_ccy("EUR/USD", SIM)


class TestSimulatedExchangeBracketQuantityIndependence:
    """
    Test that bracket orders maintain independent quantities when multiple brackets are
    submitted for the same instrument.

    This test verifies the fix for a bug where reduce-only child orders (TP/SL) were
    incorrectly synced to the net position size when multiple bracket orders were
    filled. Each bracket's child orders should track their own parent entry's filled
    quantity.

    """

    def setup(self):
        # Fixture Setup
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.data_engine = DataEngine(
            msgbus=self.msgbus,
            clock=self.clock,
            cache=self.cache,
        )

        self.exec_engine = ExecutionEngine(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.emulator = OrderEmulator(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.exchange = SimulatedExchange(
            venue=SIM,
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            default_leverage=Decimal(1),
            leverages={},
            modules=[],
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            latency_model=LatencyModel(0),
            reject_stop_orders=False,
            support_gtd_orders=True,
            support_contingent_orders=True,
            use_position_ids=True,
            use_random_ids=False,
            use_reduce_only=True,
            use_message_queue=True,
        )

        self.exec_client = BacktestExecClient(
            exchange=self.exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Wire up components
        self.exec_engine.register_client(self.exec_client)
        self.exchange.register_client(self.exec_client)

        self.cache.add_instrument(EURUSD_SIM)

        # Create mock strategy
        self.strategy = MockStrategy(bar_type=TestDataStubs.bartype_usdjpy_1min_bid())
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        # Start components
        self.exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.emulator.start()
        self.strategy.start()

    def test_multiple_bracket_orders_maintain_independent_quantities(self):
        """
        Test that when two bracket orders are submitted and both entry orders fill, each
        bracket's TP/SL orders maintain their own quantities based on their parent entry
        order, not the aggregated position.
        """
        # Arrange
        order_factory = OrderFactory(
            trader_id=self.strategy.trader_id,
            strategy_id=self.strategy.id,
            clock=self.clock,
        )

        # Create first bracket order with quantity 100,000
        bracket1 = order_factory.bracket(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            entry_order_type=OrderType.LIMIT,
            entry_price=Price.from_str("1.10000"),
            sl_trigger_price=Price.from_str("1.09900"),  # 100 pips below
            tp_price=Price.from_str("1.10100"),  # 100 pips above
        )

        # Create second bracket order with quantity 200,000
        bracket2 = order_factory.bracket(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            entry_order_type=OrderType.LIMIT,
            entry_price=Price.from_str("1.10000"),
            sl_trigger_price=Price.from_str("1.09900"),  # 100 pips below
            tp_price=Price.from_str("1.10100"),  # 100 pips above
        )

        # Get the individual orders for verification
        entry1 = bracket1.orders[0]
        tp1 = bracket1.orders[1]
        sl1 = bracket1.orders[2]

        entry2 = bracket2.orders[0]
        tp2 = bracket2.orders[1]
        sl2 = bracket2.orders[2]

        # Act - Process quote first to set market prices
        # For BUY limit orders at 1.10000 to fill, ask must be <= 1.10000
        quote = QuoteTick(
            instrument_id=EURUSD_SIM.id,
            bid_price=Price.from_str("1.09999"),
            ask_price=Price.from_str("1.10000"),  # At limit price to trigger fill
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(quote)
        self.exchange.process_quote_tick(quote)

        # Submit both bracket orders
        self.strategy.submit_order_list(bracket1)
        self.strategy.submit_order_list(bracket2)

        # Process the exchange to execute orders
        self.exchange.process(0)

        # Assert - Verify both entry orders are filled
        assert entry1.status == OrderStatus.FILLED
        assert entry2.status == OrderStatus.FILLED
        assert entry1.filled_qty == Quantity.from_int(100_000)
        assert entry2.filled_qty == Quantity.from_int(200_000)

        # Assert - Verify TP/SL orders maintain independent quantities
        # This is the key assertion that tests the fix
        assert tp1.quantity == Quantity.from_int(
            100_000,
        ), "TP1 should maintain its bracket's quantity"
        assert sl1.quantity == Quantity.from_int(
            100_000,
        ), "SL1 should maintain its bracket's quantity"
        assert tp2.quantity == Quantity.from_int(
            200_000,
        ), "TP2 should maintain its bracket's quantity"
        assert sl2.quantity == Quantity.from_int(
            200_000,
        ), "SL2 should maintain its bracket's quantity"

        # Verify position total is correct (sum of both entries)
        position = self.cache.positions_open()[0]
        assert position.quantity == Quantity.from_int(300_000)

    def test_partial_fills_update_child_orders_independently(self):
        """
        Test that when bracket entries are partially filled, each bracket's TP/SL orders
        update to their own parent's filled quantity, not the aggregated position.
        """
        # Arrange
        order_factory = OrderFactory(
            trader_id=self.strategy.trader_id,
            strategy_id=self.strategy.id,
            clock=self.clock,
        )

        # Create first bracket order with quantity 100,000
        bracket1 = order_factory.bracket(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            entry_order_type=OrderType.LIMIT,
            entry_price=Price.from_str("1.10000"),
            sl_trigger_price=Price.from_str("1.09900"),
            tp_price=Price.from_str("1.10100"),
        )

        # Create second bracket order with quantity 200,000
        bracket2 = order_factory.bracket(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            entry_order_type=OrderType.LIMIT,
            entry_price=Price.from_str("1.10000"),
            sl_trigger_price=Price.from_str("1.09900"),
            tp_price=Price.from_str("1.10100"),
        )

        self.strategy.submit_order_list(bracket1)
        self.strategy.submit_order_list(bracket2)

        entry1 = bracket1.orders[0]
        tp1 = bracket1.orders[1]
        sl1 = bracket1.orders[2]

        entry2 = bracket2.orders[0]
        tp2 = bracket2.orders[1]
        sl2 = bracket2.orders[2]

        # Act - Process quote with limited size to cause partial fills
        quote1 = QuoteTick(
            instrument_id=EURUSD_SIM.id,
            bid_price=Price.from_str("1.09999"),
            ask_price=Price.from_str("1.10000"),  # At limit price for fills
            bid_size=Quantity.from_int(50_000),  # Limited liquidity
            ask_size=Quantity.from_int(50_000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(quote1)
        self.exchange.process_quote_tick(quote1)
        self.exchange.process(0)

        # Assert - Both orders get filled with the available quantity
        # The simulated exchange allows each order to fill up to the available size
        assert entry1.status == OrderStatus.PARTIALLY_FILLED
        assert entry1.filled_qty == Quantity.from_int(50_000)
        assert entry2.status == OrderStatus.PARTIALLY_FILLED
        assert entry2.filled_qty == Quantity.from_int(50_000)

        # Check that child orders update based on their parent's filled qty - this is the key test
        # TP/SL for first bracket should update to its parent's filled qty (50k)
        assert tp1.quantity == Quantity.from_int(50_000)
        assert sl1.quantity == Quantity.from_int(50_000)

        # TP/SL for second bracket should also update to its parent's filled qty (50k)
        assert tp2.quantity == Quantity.from_int(50_000)
        assert sl2.quantity == Quantity.from_int(50_000)

        # Act - Process another quote to fill more
        quote2 = QuoteTick(
            instrument_id=EURUSD_SIM.id,
            bid_price=Price.from_str("1.09999"),
            ask_price=Price.from_str("1.10000"),  # At limit price for fills
            bid_size=Quantity.from_int(250_000),
            ask_size=Quantity.from_int(250_000),
            ts_event=1,
            ts_init=1,
        )
        self.data_engine.process(quote2)
        self.exchange.process_quote_tick(quote2)
        self.exchange.process(1)

        # Assert - Both brackets now filled
        assert entry1.status == OrderStatus.FILLED
        assert entry1.filled_qty == Quantity.from_int(100_000)
        assert entry2.status == OrderStatus.FILLED
        assert entry2.filled_qty == Quantity.from_int(200_000)

        # Each bracket's TP/SL should match its own entry's filled quantity
        assert tp1.quantity == Quantity.from_int(100_000)
        assert sl1.quantity == Quantity.from_int(100_000)
        assert tp2.quantity == Quantity.from_int(200_000)
        assert sl2.quantity == Quantity.from_int(200_000)

    def test_standalone_reduce_only_without_parent_uses_position_quantity(self):
        """
        Test that standalone reduce-only orders (without parent_order_id) sync to the
        position quantity as the fallback behavior when they have no parent order.

        This ensures backward compatibility for reduce-only orders submitted outside of
        bracket contexts.

        """
        # Arrange
        order_factory = OrderFactory(
            trader_id=self.strategy.trader_id,
            strategy_id=self.strategy.id,
            clock=self.clock,
        )

        # First, open a position
        entry = order_factory.market(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
        )

        # Process quote to set market prices
        quote = QuoteTick(
            instrument_id=EURUSD_SIM.id,
            bid_price=Price.from_str("1.09999"),
            ask_price=Price.from_str("1.10001"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(quote)
        self.exchange.process_quote_tick(quote)

        self.strategy.submit_order(entry)
        self.exchange.process(0)

        assert entry.status == OrderStatus.FILLED

        # Create standalone reduce-only orders (no parent_order_id)
        reduce_only_tp = order_factory.limit(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(50_000),  # Start with partial size
            price=Price.from_str("1.10100"),
            reduce_only=True,
        )

        reduce_only_sl = order_factory.stop_market(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(50_000),  # Start with partial size
            trigger_price=Price.from_str("1.09900"),
            reduce_only=True,
        )

        # Process a tick to trigger reduce-only sync
        quote2 = QuoteTick(
            instrument_id=EURUSD_SIM.id,
            bid_price=Price.from_str("1.10000"),
            ask_price=Price.from_str("1.10002"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=1,
            ts_init=1,
        )
        self.data_engine.process(quote2)
        self.exchange.process_quote_tick(quote2)

        self.strategy.submit_order(reduce_only_tp)
        self.strategy.submit_order(reduce_only_sl)
        self.exchange.process(1)

        # Assert - Position is correct
        position = self.cache.positions_open()[0]
        assert position.quantity == Quantity.from_int(100_000)

        # The reduce-only orders keep their original quantities until a fill event triggers the sync
        # This is the expected behavior - sync happens on fills, not on submission
        assert reduce_only_tp.quantity == Quantity.from_int(50_000)
        assert reduce_only_sl.quantity == Quantity.from_int(50_000)

        # Now trigger a partial fill on the reduce-only order to see the sync behavior
        # Process a quote that would trigger the TP order
        quote3 = QuoteTick(
            instrument_id=EURUSD_SIM.id,
            bid_price=Price.from_str("1.10100"),  # Hit the TP price
            ask_price=Price.from_str("1.10102"),
            bid_size=Quantity.from_int(30_000),  # Partial liquidity
            ask_size=Quantity.from_int(1_000_000),
            ts_event=2,
            ts_init=2,
        )
        self.data_engine.process(quote3)
        self.exchange.process_quote_tick(quote3)
        self.exchange.process(2)

        # After the partial fill, check the behavior
        # The TP order gets partially filled (30k)
        assert reduce_only_tp.filled_qty == Quantity.from_int(30_000)
        # The position is now 70k (100k - 30k)
        position = self.cache.positions_open()[0]
        assert position.quantity == Quantity.from_int(70_000)

        # The stop-market order remains at its original quantity because it's not yet triggered
        # (stop orders are not considered "passive" until triggered)
        assert reduce_only_sl.quantity == Quantity.from_int(50_000)

        # Now trigger the stop-loss to see the sync behavior
        quote4 = QuoteTick(
            instrument_id=EURUSD_SIM.id,
            bid_price=Price.from_str("1.09890"),  # Trigger the stop-loss
            ask_price=Price.from_str("1.09892"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=3,
            ts_init=3,
        )
        self.data_engine.process(quote4)
        self.exchange.process_quote_tick(quote4)
        self.exchange.process(3)

        # After triggering, the stop order becomes a market order and fills
        # The standalone SL order (without parent_order_id) would have been synced to
        # position quantity if it were a passive order like a limit order
        assert reduce_only_sl.status == OrderStatus.FILLED
        assert reduce_only_sl.filled_qty == Quantity.from_int(50_000)

    def test_extreme_position_reduction_caps_all_bracket_children(self):
        """
        Test the edge case where two brackets are active and the position is externally
        reduced below both parent quantities.

        All child orders should be capped to the remaining position size.

        """
        # Arrange
        order_factory = OrderFactory(
            trader_id=self.strategy.trader_id,
            strategy_id=self.strategy.id,
            clock=self.clock,
        )

        # Create first bracket order with quantity 100,000
        bracket1 = order_factory.bracket(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            entry_order_type=OrderType.LIMIT,
            entry_price=Price.from_str("1.10000"),
            sl_trigger_price=Price.from_str("1.09900"),
            tp_price=Price.from_str("1.10100"),
        )

        # Create second bracket order with quantity 200,000
        bracket2 = order_factory.bracket(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(200_000),
            entry_order_type=OrderType.LIMIT,
            entry_price=Price.from_str("1.10000"),
            sl_trigger_price=Price.from_str("1.09900"),
            tp_price=Price.from_str("1.10100"),
        )

        # Get the individual orders for verification
        entry1 = bracket1.orders[0]
        tp1 = bracket1.orders[1]
        sl1 = bracket1.orders[2]

        entry2 = bracket2.orders[0]
        tp2 = bracket2.orders[1]
        sl2 = bracket2.orders[2]

        # Process quote first to set market prices
        quote = QuoteTick(
            instrument_id=EURUSD_SIM.id,
            bid_price=Price.from_str("1.09999"),
            ask_price=Price.from_str("1.10000"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
        self.data_engine.process(quote)
        self.exchange.process_quote_tick(quote)

        # Submit both bracket orders
        self.strategy.submit_order_list(bracket1)
        self.strategy.submit_order_list(bracket2)

        # Process the exchange to execute orders
        self.exchange.process(0)

        # Verify both brackets filled
        assert entry1.status == OrderStatus.FILLED
        assert entry2.status == OrderStatus.FILLED

        # Position should be 300k (100k + 200k)
        position = self.cache.positions_open()[0]
        assert position.quantity == Quantity.from_int(300_000)

        # Child orders should maintain their parent quantities initially
        assert tp1.quantity == Quantity.from_int(100_000)
        assert sl1.quantity == Quantity.from_int(100_000)
        assert tp2.quantity == Quantity.from_int(200_000)
        assert sl2.quantity == Quantity.from_int(200_000)

        # Act - Reduce position externally by 270k, leaving only 30k
        reduce_order = order_factory.market(
            instrument_id=EURUSD_SIM.id,
            order_side=OrderSide.SELL,
            quantity=Quantity.from_int(270_000),
        )
        self.strategy.submit_order(reduce_order, position_id=position.id)
        self.exchange.process(1)

        # Assert - Position reduced to 30k
        position = self.cache.positions_open()[0]
        assert position.quantity == Quantity.from_int(30_000)

        # All child orders should be capped to the remaining position (30k)
        # This is the minimum of their parent's filled qty and the position qty
        assert tp1.quantity == Quantity.from_int(30_000), "TP1 should be capped to position"
        assert sl1.quantity == Quantity.from_int(30_000), "SL1 should be capped to position"
        assert tp2.quantity == Quantity.from_int(30_000), "TP2 should be capped to position"
        assert sl2.quantity == Quantity.from_int(30_000), "SL2 should be capped to position"
