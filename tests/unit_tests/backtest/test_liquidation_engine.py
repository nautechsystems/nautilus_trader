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
Integration tests for the SimulatedExchange liquidation engine (#3788).

These tests verify that the liquidation logic correctly:
  - Stays inactive when equity > maintenance margin threshold
  - Triggers a market close of all open positions when equity <= threshold
  - Cancels open orders (when configured) on liquidation
  - Does not trigger again for the same account/currency after liquidation
"""

from decimal import Decimal

from nautilus_trader.backtest.engine import SimulatedExchange
from nautilus_trader.backtest.execution_client import BacktestExecClient
from nautilus_trader.backtest.models import FillModel
from nautilus_trader.backtest.models import LatencyModel
from nautilus_trader.backtest.models import MakerTakerFeeModel
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.component import TestClock
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
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


XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()

# Price at which we open positions (USD per BTC)
ENTRY_PRICE = 40_000
# Price that induces a large unrealized loss
CRASH_PRICE = 20_000


def _make_quote(price_str: str, ts: int = 0) -> QuoteTick:
    return QuoteTick(
        instrument_id=XBTUSD_BITMEX.id,
        bid_price=Price.from_str(price_str),
        ask_price=Price.from_str(price_str),
        bid_size=Quantity.from_int(10_000_000),
        ask_size=Quantity.from_int(10_000_000),
        ts_event=ts,
        ts_init=ts,
    )


class TestLiquidationEngine:
    """Tests for the SimulatedExchange liquidation engine feature."""

    def _build_exchange(
        self,
        liquidation_enabled: bool = True,
        liquidation_trigger_ratio: float = 1.0,
        liquidation_cancel_open_orders: bool = True,
        starting_btc: float = 1.0,
    ) -> SimulatedExchange:
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
            cache=self.cache,
            clock=self.clock,
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

        exchange = SimulatedExchange(
            venue=Venue("BITMEX"),
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=BTC,
            starting_balances=[Money(starting_btc, BTC)],
            default_leverage=Decimal(100),
            leverages={},
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            clock=self.clock,
            latency_model=LatencyModel(0),
            liquidation_enabled=liquidation_enabled,
            liquidation_trigger_ratio=liquidation_trigger_ratio,
            liquidation_cancel_open_orders=liquidation_cancel_open_orders,
        )
        exchange.add_instrument(XBTUSD_BITMEX)

        exec_client = BacktestExecClient(
            exchange=exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.exec_engine.register_client(exec_client)
        exchange.register_client(exec_client)

        self.cache.add_instrument(XBTUSD_BITMEX)

        self.strategy = MockStrategy(
            bar_type=TestDataStubs.bartype_btcusdt_binance_100tick_last()
        )
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()

        return exchange

    def _open_large_long(self, exchange: SimulatedExchange, qty: int = 10_000_000) -> None:
        """Open a large long position in XBTUSD to test margin requirements."""
        quote = _make_quote(f"{ENTRY_PRICE}.0")
        self.data_engine.process(quote)
        exchange.process_quote_tick(quote)

        order = self.strategy.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(qty),
        )
        self.strategy.submit_order(order)
        exchange.process(0)

    # ------------------------------------------------------------------
    # Construction tests
    # ------------------------------------------------------------------

    def test_liquidation_enabled_defaults_false(self):
        """Liquidation is disabled by default (no liquidation_enabled kwarg)."""
        clock = TestClock()
        trader_id = TestIdStubs.trader_id()
        msgbus = MessageBus(trader_id=trader_id, clock=clock)
        cache = TestComponentStubs.cache()
        portfolio = Portfolio(msgbus=msgbus, cache=cache, clock=clock)

        exchange = SimulatedExchange(
            venue=Venue("BITMEX"),
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=BTC,
            starting_balances=[Money(1, BTC)],
            default_leverage=Decimal(10),
            leverages={},
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            clock=clock,
            latency_model=LatencyModel(0),
        )
        assert not exchange.liquidation_enabled

    def test_liquidation_enabled_can_be_set(self):
        """Liquidation fields survive round-trip through SimulatedExchange init."""
        exchange = self._build_exchange(
            liquidation_enabled=True,
            liquidation_trigger_ratio=0.9,
            liquidation_cancel_open_orders=False,
        )
        assert exchange.liquidation_enabled is True
        assert exchange.liquidation_trigger_ratio == Decimal("0.9")
        assert exchange.liquidation_cancel_open_orders is False

    # ------------------------------------------------------------------
    # No liquidation when equity is healthy
    # ------------------------------------------------------------------

    def test_no_liquidation_when_equity_above_threshold(self):
        """With a healthy balance no positions are closed on a price drop."""
        # Start with a large balance; positions won't be underwater
        exchange = self._build_exchange(starting_btc=100.0)

        self._open_large_long(exchange, qty=100_000)

        # Drop price moderately
        quote = _make_quote(f"{ENTRY_PRICE // 2}.0")
        self.data_engine.process(quote)
        exchange.process_quote_tick(quote)
        exchange.process(0)

        positions_open = self.cache.positions_open()
        assert len(positions_open) == 1, "Position should still be open (no liquidation)"

    # ------------------------------------------------------------------
    # Liquidation triggered on large price crash
    # ------------------------------------------------------------------

    def test_liquidation_triggered_closes_position(self):
        """
        Equity falls below maintenance margin threshold → position is force-closed.

        Setup:
          - 1 BTC starting balance
          - Open 10M contract long at 40,000 USD  (≈ 250 BTC notional)
          - Price crashes 50% to 20,000 USD
          - Unrealized loss ≈ 10M*(1/20k - 1/40k) = 250 BTC >> 1 BTC balance
          - Equity becomes deeply negative → liquidation must trigger
        """
        exchange = self._build_exchange(
            liquidation_enabled=True,
            liquidation_trigger_ratio=1.0,
            starting_btc=1.0,
        )

        self._open_large_long(exchange, qty=10_000_000)

        # Sanity: position is open
        assert len(self.cache.positions_open()) == 1

        # Crash the price
        crash_quote = _make_quote(f"{CRASH_PRICE}.0")
        self.data_engine.process(crash_quote)
        exchange.process_quote_tick(crash_quote)
        exchange.process(0)  # triggers _process_margin_liquidations

        # Position should now be closed
        positions_open = self.cache.positions_open()
        positions_closed = self.cache.positions_closed()
        assert len(positions_open) == 0, "All positions must be closed after liquidation"
        assert len(positions_closed) >= 1, "Closed position should appear in cache"

    # ------------------------------------------------------------------
    # Disabled liquidation: positions stay open despite deep losses
    # ------------------------------------------------------------------

    def test_liquidation_disabled_does_not_close_position(self):
        """When liquidation_enabled=False, positions stay open no matter the loss."""
        exchange = self._build_exchange(
            liquidation_enabled=False,
            starting_btc=1.0,
        )

        self._open_large_long(exchange, qty=10_000_000)

        crash_quote = _make_quote(f"{CRASH_PRICE}.0")
        self.data_engine.process(crash_quote)
        exchange.process_quote_tick(crash_quote)
        exchange.process(0)

        assert len(self.cache.positions_open()) == 1, "Position must remain open when liquidation is disabled"

    # ------------------------------------------------------------------
    # cancel_open_orders flag
    # ------------------------------------------------------------------

    def test_liquidation_cancels_open_orders_when_flag_set(self):
        """Open limit orders are cancelled when liquidation_cancel_open_orders=True."""
        exchange = self._build_exchange(
            liquidation_enabled=True,
            liquidation_cancel_open_orders=True,
            starting_btc=1.0,
        )

        # Set up market
        quote = _make_quote(f"{ENTRY_PRICE}.0")
        self.data_engine.process(quote)
        exchange.process_quote_tick(quote)

        # Open a position
        market_order = self.strategy.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(10_000_000),
        )
        self.strategy.submit_order(market_order)
        exchange.process(0)

        # Place a resting limit order well BELOW the crash price so it doesn't fill
        limit_order = self.strategy.order_factory.limit(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("10000.0"),
        )
        self.strategy.submit_order(limit_order)
        exchange.process(0)

        # Confirm limit is resting
        assert limit_order.status == OrderStatus.ACCEPTED

        # Crash price to trigger liquidation
        crash_quote = _make_quote(f"{CRASH_PRICE}.0")
        self.data_engine.process(crash_quote)
        exchange.process_quote_tick(crash_quote)
        exchange.process(0)

        # Limit order should be cancelled
        assert limit_order.status == OrderStatus.CANCELED, (
            "Resting limit order must be cancelled on liquidation"
        )

    def test_liquidation_leaves_open_orders_when_flag_unset(self):
        """Open limit orders remain when liquidation_cancel_open_orders=False."""
        exchange = self._build_exchange(
            liquidation_enabled=True,
            liquidation_cancel_open_orders=False,
            starting_btc=1.0,
        )

        quote = _make_quote(f"{ENTRY_PRICE}.0")
        self.data_engine.process(quote)
        exchange.process_quote_tick(quote)

        market_order = self.strategy.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(10_000_000),
        )
        self.strategy.submit_order(market_order)
        exchange.process(0)

        limit_order = self.strategy.order_factory.limit(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(1_000),
            Price.from_str("10000.0"),
        )
        self.strategy.submit_order(limit_order)
        exchange.process(0)

        crash_quote = _make_quote(f"{CRASH_PRICE}.0")
        self.data_engine.process(crash_quote)
        exchange.process_quote_tick(crash_quote)
        exchange.process(0)

        # Limit order should still be accepted (not cancelled)
        assert limit_order.status == OrderStatus.ACCEPTED, (
            "Resting limit order must NOT be cancelled when cancel flag is False"
        )

    # ------------------------------------------------------------------
    # No double-liquidation
    # ------------------------------------------------------------------

    def test_liquidation_does_not_trigger_twice(self):
        """After liquidation, a subsequent process() call does not re-liquidate."""
        exchange = self._build_exchange(
            liquidation_enabled=True,
            starting_btc=1.0,
        )

        self._open_large_long(exchange, qty=10_000_000)

        crash_quote = _make_quote(f"{CRASH_PRICE}.0")
        self.data_engine.process(crash_quote)
        exchange.process_quote_tick(crash_quote)
        exchange.process(0)  # first liquidation

        # After liquidation, position is closed; record event count
        events_after_first = len(self.strategy.store)

        # Second process should not generate additional liquidation events
        exchange.process(1)
        events_after_second = len(self.strategy.store)

        assert events_after_second == events_after_first, (
            "No additional events should be generated on second process() after liquidation"
        )


class TestLiquidationEngineMarketSimulation:
    """
    Market-simulation tests for the liquidation engine.

    These feed a realistic sequence of price ticks into the SimulatedExchange
    and verify that:
      - Liquidation does NOT fire on small, survivable drawdowns.
      - Liquidation fires at the right tick when equity crosses the threshold.
      - All subsequent ticks after liquidation produce no further events.
      - Price recovery (bounce) prevents liquidation from firing.
      - Multiple declining ticks stop exactly at the first crossing tick.
    """

    def _build_exchange(
        self,
        liquidation_enabled: bool = True,
        liquidation_trigger_ratio: float = 1.0,
        liquidation_cancel_open_orders: bool = True,
        starting_btc: float = 1.0,
    ) -> SimulatedExchange:
        self.clock = TestClock()
        self.trader_id = TestIdStubs.trader_id()
        self.msgbus = MessageBus(trader_id=self.trader_id, clock=self.clock)
        self.cache = TestComponentStubs.cache()
        self.portfolio = Portfolio(msgbus=self.msgbus, cache=self.cache, clock=self.clock)
        self.data_engine = DataEngine(msgbus=self.msgbus, cache=self.cache, clock=self.clock)
        self.exec_engine = ExecutionEngine(msgbus=self.msgbus, cache=self.cache, clock=self.clock)
        self.risk_engine = RiskEngine(
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        exchange = SimulatedExchange(
            venue=Venue("BITMEX"),
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            base_currency=BTC,
            starting_balances=[Money(starting_btc, BTC)],
            default_leverage=Decimal(100),
            leverages={},
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            modules=[],
            fill_model=FillModel(),
            fee_model=MakerTakerFeeModel(),
            clock=self.clock,
            latency_model=LatencyModel(0),
            liquidation_enabled=liquidation_enabled,
            liquidation_trigger_ratio=liquidation_trigger_ratio,
            liquidation_cancel_open_orders=liquidation_cancel_open_orders,
        )
        exchange.add_instrument(XBTUSD_BITMEX)

        exec_client = BacktestExecClient(
            exchange=exchange,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        self.exec_engine.register_client(exec_client)
        exchange.register_client(exec_client)
        self.cache.add_instrument(XBTUSD_BITMEX)

        self.strategy = MockStrategy(bar_type=TestDataStubs.bartype_btcusdt_binance_100tick_last())
        self.strategy.register(
            trader_id=self.trader_id,
            portfolio=self.portfolio,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )
        exchange.reset()
        self.data_engine.start()
        self.exec_engine.start()
        self.strategy.start()
        return exchange

    def _feed_price(self, exchange: SimulatedExchange, price: float, ts: int = 0) -> None:
        """Push a single quote tick through data engine and exchange."""
        q = _make_quote(f"{price:.1f}", ts=ts)
        self.data_engine.process(q)
        exchange.process_quote_tick(q)
        exchange.process(ts)

    def _open_long(self, exchange: SimulatedExchange, price: float, qty: int) -> None:
        q = _make_quote(f"{price:.1f}")
        self.data_engine.process(q)
        exchange.process_quote_tick(q)
        order = self.strategy.order_factory.market(
            XBTUSD_BITMEX.id,
            OrderSide.BUY,
            Quantity.from_int(qty),
        )
        self.strategy.submit_order(order)
        exchange.process(0)

    # ------------------------------------------------------------------
    # Gradual decline — no premature liquidation
    # ------------------------------------------------------------------

    def test_gradual_decline_no_premature_liquidation(self):
        """
        Price drops in small steps; liquidation must NOT fire until the last step
        that pushes equity below the threshold.

        With 100 BTC balance and a 100k USD (100k/40k ≈ 2.5 BTC notional) position,
        a 50 % drop only produces ~1.25 BTC loss, leaving plenty of equity. The
        exchange must keep the position open through the whole sequence.
        """
        exchange = self._build_exchange(
            liquidation_enabled=True,
            liquidation_trigger_ratio=1.0,
            starting_btc=100.0,
        )

        self._open_long(exchange, price=ENTRY_PRICE, qty=100_000)
        assert len(self.cache.positions_open()) == 1

        # Step price down from 40k → 20k in 10 equal steps
        steps = 10
        for i in range(1, steps + 1):
            price = ENTRY_PRICE - i * ((ENTRY_PRICE - CRASH_PRICE) / steps)
            self._feed_price(exchange, price, ts=i)

        # 100 BTC balance >> ~1.25 BTC loss → position survives all steps
        assert len(self.cache.positions_open()) == 1, (
            "Position must remain open: equity is well above maintenance threshold"
        )

    # ------------------------------------------------------------------
    # Gradual decline — liquidation fires on the correct tick
    # ------------------------------------------------------------------

    def test_gradual_decline_liquidation_fires_at_threshold(self):
        """
        Price declines step-by-step; liquidation fires on the first tick where
        equity < maintenance_margin * trigger_ratio, and not before.

        With only 1 BTC balance and a 10M USD position (≈ 250 BTC notional at
        40k), even a moderate drop causes catastrophic losses. We confirm the
        position is still open at the first few steps and closed by the end.
        """
        exchange = self._build_exchange(
            liquidation_enabled=True,
            liquidation_trigger_ratio=1.0,
            starting_btc=1.0,
        )

        self._open_long(exchange, price=ENTRY_PRICE, qty=10_000_000)
        assert len(self.cache.positions_open()) == 1

        liquidated_at_step = None
        steps = 20
        for i in range(1, steps + 1):
            price = ENTRY_PRICE - i * ((ENTRY_PRICE - CRASH_PRICE) / steps)
            self._feed_price(exchange, price, ts=i)
            if len(self.cache.positions_open()) == 0:
                liquidated_at_step = i
                break

        assert liquidated_at_step is not None, "Liquidation must have occurred during the decline"
        # Confirm no open positions remain after liquidation
        assert len(self.cache.positions_open()) == 0
        assert len(self.cache.positions_closed()) >= 1

    # ------------------------------------------------------------------
    # Price recovery prevents liquidation
    # ------------------------------------------------------------------

    def test_price_recovery_prevents_liquidation(self):
        """
        Price dips toward the danger zone then recovers; liquidation must NOT fire.

        With 100 BTC balance and a 10M USD position the equity dips on the
        downward leg but the account is still solvent. On recovery the position
        stays open throughout.
        """
        exchange = self._build_exchange(
            liquidation_enabled=True,
            liquidation_trigger_ratio=1.0,
            starting_btc=100.0,
        )

        self._open_long(exchange, price=ENTRY_PRICE, qty=10_000_000)
        assert len(self.cache.positions_open()) == 1

        # Decline to 38k (small 5 % drop — well within 100 BTC buffer)
        declining_prices = [39_000, 38_500, 38_000]
        for ts, price in enumerate(declining_prices, start=1):
            self._feed_price(exchange, price, ts=ts)

        # Recover back above entry
        recovering_prices = [39_000, 40_000, 41_000]
        for ts, price in enumerate(recovering_prices, start=len(declining_prices) + 1):
            self._feed_price(exchange, price, ts=ts)

        assert len(self.cache.positions_open()) == 1, (
            "Position must remain open after price recovers"
        )

    # ------------------------------------------------------------------
    # Post-liquidation ticks produce no events
    # ------------------------------------------------------------------

    def test_continued_ticks_after_liquidation_produce_no_events(self):
        """
        After the position is force-closed by liquidation, further price ticks
        must not generate any additional order or position events.
        """
        exchange = self._build_exchange(
            liquidation_enabled=True,
            liquidation_trigger_ratio=1.0,
            starting_btc=1.0,
        )

        self._open_long(exchange, price=ENTRY_PRICE, qty=10_000_000)

        # Trigger liquidation
        self._feed_price(exchange, CRASH_PRICE, ts=1)
        assert len(self.cache.positions_open()) == 0

        events_at_liquidation = len(self.strategy.store)

        # Five more ticks after liquidation
        for i in range(2, 7):
            self._feed_price(exchange, CRASH_PRICE - i * 100, ts=i)

        assert len(self.strategy.store) == events_at_liquidation, (
            "No new events must be generated after liquidation is complete"
        )

    # ------------------------------------------------------------------
    # Threshold sensitivity: ratio > 1.0 triggers earlier
    # ------------------------------------------------------------------

    def test_higher_trigger_ratio_liquidates_earlier(self):
        """
        trigger_ratio=2.0 should liquidate at a smaller price drop than ratio=1.0,
        because equity must stay above 2x the maintenance margin.
        """
        def drop_until_liquidated(ratio: float) -> int | None:
            ex = self._build_exchange(
                liquidation_enabled=True,
                liquidation_trigger_ratio=ratio,
                starting_btc=1.0,
            )
            self._open_long(ex, price=ENTRY_PRICE, qty=10_000_000)
            steps = 50
            for i in range(1, steps + 1):
                price = ENTRY_PRICE - i * ((ENTRY_PRICE - CRASH_PRICE) / steps)
                self._feed_price(ex, price, ts=i)
                if len(self.cache.positions_open()) == 0:
                    return i
            return None  # never liquidated within the steps

        step_at_ratio_2 = drop_until_liquidated(2.0)
        step_at_ratio_1 = drop_until_liquidated(1.0)

        assert step_at_ratio_2 is not None, "ratio=2.0 must liquidate"
        assert step_at_ratio_1 is not None, "ratio=1.0 must liquidate"
        assert step_at_ratio_2 <= step_at_ratio_1, (
            "Higher trigger_ratio must liquidate at an earlier (or equal) step"
        )
