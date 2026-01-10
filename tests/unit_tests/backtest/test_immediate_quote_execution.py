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

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.backtest.models import BestPriceFillModel
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.trading.strategy import Strategy


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD", Venue("SIM"))


class ImmediateOrderStrategy(Strategy):
    """
    Strategy that submits an order immediately on the first quote tick.
    """

    def __init__(self):
        super().__init__()
        self.instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        self.quote_count = 0
        self.order_submitted = False
        self.fill_timestamps = []
        self.submit_timestamps = []

    def on_start(self):
        self.subscribe_quote_ticks(self.instrument_id)

    def on_quote_tick(self, tick: QuoteTick):
        self.quote_count += 1
        self._log.warning(
            f"[Strategy] on_quote_tick: Received quote tick at {tick.ts_init}, "
            f"bid={tick.bid_price}, ask={tick.ask_price}, quote_count={self.quote_count}",
        )

        # Submit limit order on first quote tick at mid price rounded up (like in the example)
        # With BestPriceFillModel and immediate execution enabled:
        # - Order is submitted and accepted immediately (use_message_queue=False)
        # - When exchange processes quote, order is already in book
        # - iterate() matches the order on the same quote tick (mid price between bid/ask is matchable)
        # With immediate execution disabled:
        # - Exchange processes quote first (no order in book yet)
        # - Order is queued (use_message_queue=True)
        # - Queue is processed, order accepted but not immediately matchable (mid < ask)
        # - Order only matches when next quote arrives and iterate() is called again
        if not self.order_submitted:
            instrument = self.cache.instrument(self.instrument_id)
            # Calculate mid price and round up to next ask price (like in the example)
            # The key difference is WHEN the order is accepted relative to quote processing:
            # - With immediate execution: order accepted before exchange processes quote,
            #   so order is in book when iterate() is called → fills on same tick
            # - Without immediate execution: exchange processes quote first (iterate() called),
            #   then order queued, then accepted (but iterate() already ran) → waits for next tick
            mid_price_double = (tick.bid_price.as_double() + tick.ask_price.as_double()) / 2.0
            # Use mid price rounded up - BestPriceFillModel allows matching at mid price during iteration
            limit_price = instrument.next_ask_price(mid_price_double, num_ticks=0)

            order = self.order_factory.limit(
                instrument_id=self.instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity.from_int(100_000),
                price=limit_price,
            )
            self._log.warning(
                f"[Strategy] on_quote_tick: Submitting order at {tick.ts_init}, "
                f"order_id={order.client_order_id}, price={limit_price}, side={order.side}",
            )
            self.submit_order(order)
            self.order_submitted = True
            self.submit_timestamps.append(tick.ts_init)

    def on_order_filled(self, event):
        self._log.warning(
            f"[Strategy] on_order_filled: Order filled at {event.ts_init}, "
            f"order_id={event.client_order_id}, filled_qty={event.last_qty}, price={event.last_px}",
        )
        self.fill_timestamps.append(event.ts_init)


class TestImmediateQuoteExecution:
    """
    Test immediate quote execution feature.
    """

    def setup(self):
        # Common setup
        self.instrument = AUDUSD_SIM
        self.venue = Venue("SIM")

    def _create_quotes(self, count: int = 3):
        """
        Create a series of quote ticks.
        """
        quotes = []
        base_price = 0.70000
        for i in range(count):
            bid_price = base_price + (i * 0.00001)
            ask_price = bid_price + 0.00002
            quote = QuoteTick(
                instrument_id=self.instrument.id,
                bid_price=Price.from_str(f"{bid_price:.5f}"),
                ask_price=Price.from_str(f"{ask_price:.5f}"),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=1_000_000_000_000_000_000 + (i * 1_000_000_000),  # 1 second apart
                ts_init=1_000_000_000_000_000_000 + (i * 1_000_000_000),
            )
            quotes.append(quote)
        return quotes

    def test_immediate_quote_execution_enabled_fills_on_same_tick(self):
        """
        Test that with immediate execution enabled, orders fill on the same quote tick.
        """
        # Arrange
        config = BacktestEngineConfig(
            allow_immediate_quote_execution=True,
        )
        engine = BacktestEngine(config=config)

        engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=BestPriceFillModel(),
        )

        engine.add_instrument(self.instrument)

        # Create quotes with 1 minute apart (like in the example: 10:03 vs 10:04)
        quotes = []
        base_ts = 1_000_000_000_000_000_000
        for i in range(2):
            bid_price = 0.70000 + (i * 0.00001)
            ask_price = bid_price + 0.00002
            quote = QuoteTick(
                instrument_id=self.instrument.id,
                bid_price=Price.from_str(f"{bid_price:.5f}"),
                ask_price=Price.from_str(f"{ask_price:.5f}"),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=base_ts + (i * 60_000_000_000),  # 1 minute apart
                ts_init=base_ts + (i * 60_000_000_000),
            )
            quotes.append(quote)

        engine.add_data(quotes)

        strategy = ImmediateOrderStrategy()
        engine.add_strategy(strategy)

        # Act
        engine.run()

        # Assert
        # Order should be filled
        orders = list(strategy.cache.orders())
        assert len(orders) == 1
        order = orders[0]
        assert order.status == OrderStatus.FILLED

        # With immediate execution enabled and use_message_queue=False:
        # - Data engine processes quote 1 first
        # - Strategy submits order, which is immediately accepted (no queue)
        # - Exchange processes quote 1, calls process_quote_tick → iterate(T1)
        # - Order is in book, iterate() matches it → fill at T1
        assert len(strategy.fill_timestamps) == 1
        assert strategy.fill_timestamps[0] == quotes[0].ts_init, (
            f"Expected fill on first quote tick ({quotes[0].ts_init}), "
            f"but got {strategy.fill_timestamps[0]}"
        )

    def test_immediate_quote_execution_disabled_fills_on_next_tick(self):
        """
        Test that with immediate execution disabled, orders fill on the NEXT quote tick.
        """
        # Arrange
        config = BacktestEngineConfig(
            allow_immediate_quote_execution=False,
        )
        engine = BacktestEngine(config=config)

        engine.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=BestPriceFillModel(),
        )

        engine.add_instrument(self.instrument)

        # Create quotes with 1 minute apart (like in the example: 10:03 vs 10:04)
        quotes = []
        base_ts = 1_000_000_000_000_000_000
        for i in range(2):
            bid_price = 0.70000 + (i * 0.00001)
            ask_price = bid_price + 0.00002
            quote = QuoteTick(
                instrument_id=self.instrument.id,
                bid_price=Price.from_str(f"{bid_price:.5f}"),
                ask_price=Price.from_str(f"{ask_price:.5f}"),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=base_ts + (i * 60_000_000_000),  # 1 minute apart
                ts_init=base_ts + (i * 60_000_000_000),
            )
            quotes.append(quote)

        engine.add_data(quotes)

        strategy = ImmediateOrderStrategy()
        engine.add_strategy(strategy)

        # Act
        engine.run()

        # Assert
        # Order should be filled
        orders = list(strategy.cache.orders())
        assert len(orders) == 1
        order = orders[0]
        assert order.status == OrderStatus.FILLED

        # With immediate execution disabled and use_message_queue=True:
        # - Exchange processes quote 1 first, calls process_quote_tick → iterate(T1)
        #   (no order in book yet, so no match)
        # - Data engine processes quote 1
        # - Strategy submits order, which goes to message queue (use_message_queue=True)
        # - exchange.process(T1) processes queue, order is accepted (but book was already updated with quote 1)
        # - Next quote arrives (T2), exchange processes quote 2
        # - process_quote_tick → iterate(T2) → order matches → fill at T2
        assert len(strategy.fill_timestamps) == 1
        assert strategy.fill_timestamps[0] == quotes[1].ts_init, (
            f"Expected fill on second quote tick ({quotes[1].ts_init}), "
            f"but got {strategy.fill_timestamps[0]}"
        )

        # Strategy should have received at least 1 quote tick
        assert strategy.quote_count >= 1

    def test_immediate_quote_execution_comparison_shows_timestamp_difference(self):
        """
        Test that demonstrates the fill timestamp difference between enabled and
        disabled.
        """
        # Create quotes with 1 minute apart to clearly show the difference
        quotes = []
        base_ts = 1_000_000_000_000_000_000
        for i in range(2):
            bid_price = 0.70000 + (i * 0.00001)
            ask_price = bid_price + 0.00002
            quote = QuoteTick(
                instrument_id=self.instrument.id,
                bid_price=Price.from_str(f"{bid_price:.5f}"),
                ask_price=Price.from_str(f"{ask_price:.5f}"),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=base_ts + (i * 60_000_000_000),  # 1 minute apart
                ts_init=base_ts + (i * 60_000_000_000),
            )
            quotes.append(quote)

        # Test with immediate execution enabled
        config_enabled = BacktestEngineConfig(allow_immediate_quote_execution=True)
        engine_enabled = BacktestEngine(config=config_enabled)
        engine_enabled.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=BestPriceFillModel(),
        )
        engine_enabled.add_instrument(self.instrument)
        engine_enabled.add_data(quotes)
        strategy_enabled = ImmediateOrderStrategy()
        engine_enabled.add_strategy(strategy_enabled)
        engine_enabled.run()

        # Test with immediate execution disabled
        config_disabled = BacktestEngineConfig(allow_immediate_quote_execution=False)
        engine_disabled = BacktestEngine(config=config_disabled)
        engine_disabled.add_venue(
            venue=self.venue,
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=USD,
            starting_balances=[Money(1_000_000, USD)],
            fill_model=BestPriceFillModel(),
        )
        engine_disabled.add_instrument(self.instrument)
        engine_disabled.add_data(quotes)
        strategy_disabled = ImmediateOrderStrategy()
        engine_disabled.add_strategy(strategy_disabled)
        engine_disabled.run()

        # Assert both orders are filled
        orders_enabled = list(strategy_enabled.cache.orders())
        orders_disabled = list(strategy_disabled.cache.orders())
        assert len(orders_enabled) == 1
        assert len(orders_disabled) == 1
        assert orders_enabled[0].status == OrderStatus.FILLED
        assert orders_disabled[0].status == OrderStatus.FILLED

        # With BestPriceFillModel and limit at mid price, the difference is clear:
        # - Enabled: Order fills on FIRST quote tick (T1) because order is in book when exchange processes quote
        # - Disabled: Order fills on SECOND quote tick (T2) because order is queued after exchange processed T1
        assert len(strategy_enabled.fill_timestamps) == 1
        assert len(strategy_disabled.fill_timestamps) == 1

        # Verify the timestamp difference - this is the key assertion
        assert strategy_enabled.fill_timestamps[0] == quotes[0].ts_init, (
            f"With immediate execution enabled, expected fill on first quote tick ({quotes[0].ts_init}), "
            f"but got {strategy_enabled.fill_timestamps[0]}"
        )
        assert strategy_disabled.fill_timestamps[0] == quotes[1].ts_init, (
            f"With immediate execution disabled, expected fill on second quote tick ({quotes[1].ts_init}), "
            f"but got {strategy_disabled.fill_timestamps[0]}"
        )

        # The timestamps should differ by 1 minute
        assert strategy_disabled.fill_timestamps[0] > strategy_enabled.fill_timestamps[0], (
            f"Disabled fill timestamp ({strategy_disabled.fill_timestamps[0]}) should be greater than "
            f"enabled fill timestamp ({strategy_enabled.fill_timestamps[0]})"
        )
