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
Hyperliquid Outcome Market Paper Trading Strategy

This strategy automatically:
1. Discovers all available outcome (prediction) markets
2. Subscribes to quotes, trades, and order book data for each
3. Places simulated orders based on market conditions
4. Manages positions and periodically flattens them
5. Reports status and PnL

Designed for long-running paper trading with robust error handling.
"""

from __future__ import annotations

import asyncio
import os
from datetime import timedelta
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.paper import is_outcome_instrument_id
from nautilus_trader.adapters.hyperliquid.paper import validate_outcome_price
from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox.factory import SandboxLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import HyperliquidEnvironment
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A PAPER TRADING STRATEGY FOR TESTING PURPOSES ONLY ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY ***


class OutcomePaperStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for `OutcomePaperStrategy` instances.
    """

    instrument_ids: list[InstrumentId] | None = None  # None = auto-discover
    client_id: ClientId = ClientId("SANDBOX")
    order_qty: Decimal = Decimal("100")  # Base order quantity in USDH
    max_positions: int = 5  # Max simultaneous positions
    min_spread_bps: int = 50  # Minimum spread to trade (50 = 0.5%)
    position_hold_seconds: int = 300  # Flatten position after this time
    status_report_interval_seconds: int = 60  # Status log interval
    price_guardrail: bool = True  # Enforce [0.001, 0.999] price range


class OutcomePaperStrategy(Strategy):
    """
    A paper trading strategy for Hyperliquid outcome markets.

    This strategy demonstrates:
    - Auto-discovery and subscription to outcome markets
    - Quote-driven order placement
    - Position management with time-based flattening
    - Robust error handling and status reporting
    """

    def __init__(self, config: OutcomePaperStrategyConfig):
        PyCondition.type(config, OutcomePaperStrategyConfig, "config")
        super().__init__(config)

        self._outcome_instruments: list[InstrumentId] = []
        self._positions_opened: int = 0
        self._positions_closed: int = 0
        self._orders_submitted: int = 0
        self._orders_filled: int = 0
        self._errors: int = 0
        self._last_prices: dict[InstrumentId, Decimal] = {}
        self._position_open_times: dict[InstrumentId, Any] = {}
        self._test_flow_done: set[InstrumentId] = set()

    def on_start(self) -> None:
        """
        Called when the strategy starts.
        """
        self.log.info("=" * 60)
        self.log.info("Outcome Paper Trading Strategy Starting")
        self.log.info("=" * 60)

        # Discover or use configured instruments
        if self.config.instrument_ids:
            self._outcome_instruments = [
                inst for inst in self.config.instrument_ids
                if is_outcome_instrument_id(inst)
            ]
            self.log.info(f"Using {len(self._outcome_instruments)} configured outcome instruments")
        else:
            # Auto-discover from cache
            self._discover_outcome_instruments()

        if not self._outcome_instruments:
            self.log.error("No outcome instruments found! Cannot continue.")
            self.stop()
            return

        # Subscribe to data for all outcome instruments
        self._subscribe_to_market_data()

        # Start periodic tasks
        self._start_periodic_tasks()

        self.log.info(f"Strategy initialized with {len(self._outcome_instruments)} markets")

    def _discover_outcome_instruments(self) -> None:
        """Discover outcome instruments from the cache."""
        all_instruments = self.cache.instruments()
        self._outcome_instruments = [
            inst.id for inst in all_instruments
            if is_outcome_instrument_id(inst.id)
        ]
        self._outcome_instruments.sort(key=lambda x: x.value)

    def _subscribe_to_market_data(self) -> None:
        """Subscribe to quotes, trades, and order book for all outcome instruments."""
        for instrument_id in self._outcome_instruments:
            try:
                # Subscribe to quote ticks (BBO)
                self.subscribe_quote_ticks(instrument_id)
                self.log.info(f"Subscribed to quotes: {instrument_id}")

                # Subscribe to trade ticks
                self.subscribe_trade_ticks(instrument_id)
                self.log.info(f"Subscribed to trades: {instrument_id}")

                # Subscribe to order book (depth 10)
                self.subscribe_order_book_depth(
                    instrument_id=instrument_id,
                    depth=10,
                )
                self.log.info(f"Subscribed to order book: {instrument_id}")

            except Exception as e:
                self.log.error(f"Failed to subscribe to {instrument_id}: {e}")
                self._errors += 1

    def _start_periodic_tasks(self) -> None:
        """Start periodic maintenance tasks."""
        # Position flattening timer
        self.clock.set_timer(
            name="FLATTEN_POSITIONS",
            interval=timedelta(seconds=10),
            callback=self._check_position_hold_times,
        )

        # Status report timer
        self.clock.set_timer(
            name="STATUS_REPORT",
            interval=timedelta(seconds=self.config.status_report_interval_seconds),
            callback=self._report_status,
        )

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Called when a quote tick is received.
        """
        instrument_id = tick.instrument_id
        self._last_prices[instrument_id] = Decimal(str(tick.bid_price))

        # Test flow: buy on first tick, then schedule sell after 5 seconds
        if instrument_id not in self._test_flow_done:
            self._test_flow_done.add(instrument_id)
            self._test_buy_then_sell(instrument_id)

    def _test_buy_then_sell(self, instrument_id: InstrumentId) -> None:
        """Test flow: market buy, wait 5s, market sell."""
        try:
            instrument = self.cache.instrument(instrument_id)
            if instrument is None:
                self.log.error(f"Instrument not found: {instrument_id}")
                return

            # Step 1: Market buy
            buy_order = self.order_factory.market(
                instrument_id=instrument_id,
                order_side=OrderSide.BUY,
                quantity=Quantity(self.config.order_qty, instrument.size_precision),
            )
            self.submit_order(buy_order, client_id=self.config.client_id)
            self._orders_submitted += 1
            self.log.info(f"[TEST] Market BUY submitted: {instrument_id}", color=LogColor.GREEN)

            # Step 2: Schedule market sell after 5 seconds
            self.clock.set_timer(
                name=f"SELL_{instrument_id.value}",
                interval=timedelta(seconds=5),
                callback=lambda event, inst=instrument_id, instr=instrument: self._test_sell(inst, instr),
            )
        except Exception as e:
            self.log.error(f"[TEST] Buy failed: {e}")
            self._errors += 1

    def _test_sell(self, instrument_id: InstrumentId, instrument) -> None:
        """Sell position for test flow."""
        try:
            sell_order = self.order_factory.market(
                instrument_id=instrument_id,
                order_side=OrderSide.SELL,
                quantity=Quantity(self.config.order_qty, instrument.size_precision),
            )
            self.submit_order(sell_order, client_id=self.config.client_id)
            self._orders_submitted += 1
            self.log.info(f"[TEST] Market SELL submitted: {instrument_id}", color=LogColor.RED)
        except Exception as e:
            self.log.error(f"[TEST] Sell failed: {e}")
            self._errors += 1

    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Called when a trade tick is received.
        """
        # Track last trade price
        self._last_prices[tick.instrument_id] = Decimal(str(tick.price))

    def _should_trade(self, instrument_id: InstrumentId) -> bool:
        """Check if we should place a trade for this instrument."""
        # Don't trade if at max positions
        open_positions = len(self.cache.positions_open())
        if open_positions >= self.config.max_positions:
            return False

        # Don't trade if already have a position in this instrument
        if self._has_any_position(instrument_id):
            return False

        return True

    def _has_position(self, instrument_id: InstrumentId, side: OrderSide) -> bool:
        """Check if we have a position for instrument with given side."""
        positions = self.cache.positions_open(instrument_id=instrument_id)
        for pos in positions:
            if pos.side == side:
                return True
        return False

    def _has_any_position(self, instrument_id: InstrumentId) -> bool:
        """Check if we have any position for instrument."""
        positions = self.cache.positions_open(instrument_id=instrument_id)
        return len(positions) > 0

    def _submit_limit_order(
        self,
        instrument_id: InstrumentId,
        side: OrderSide,
        price: float,
    ) -> None:
        """Submit a limit order."""
        try:
            # Validate price
            if self.config.price_guardrail:
                validate_outcome_price(Decimal(str(price)))

            instrument = self.cache.instrument(instrument_id)
            if instrument is None:
                self.log.error(f"Instrument not found: {instrument_id}")
                return

            order = self.order_factory.limit(
                instrument_id=instrument_id,
                side=side,
                quantity=Quantity(self.config.order_qty, instrument.size_precision),
                price=Price(price, instrument.price_precision),
                time_in_force=TimeInForce.GTC,
                post_only=True,
            )

            self.submit_order(order, client_id=self.config.client_id)
            self._orders_submitted += 1

            self.log.info(
                f"Submitted {side.name} order: {instrument_id} @ {price}",
                color=LogColor.GREEN if side == OrderSide.BUY else LogColor.RED,
            )

        except Exception as e:
            self.log.error(f"Failed to submit order: {e}")
            self._errors += 1

    def on_order_filled(self, event) -> None:
        """
        Called when an order is filled.
        """
        self._orders_filled += 1

        if event.position_id:
            position = self.cache.position(event.position_id)
            if position:
                instrument_id = position.instrument_id

                if position.is_open:
                    # Track position open time
                    if instrument_id not in self._position_open_times:
                        self._position_open_times[instrument_id] = self.clock.utc_now()
                        self._positions_opened += 1
                        self.log.info(
                            f"Position opened: {instrument_id} "
                            f"{position.side.name} {position.quantity} "
                            f"@ avg {position.avg_px_open}",
                            color=LogColor.CYAN,
                        )

    def _check_position_hold_times(self, time_event) -> None:
        """
        Check and flatten positions that have been held too long.
        """
        now = self.clock.utc_now()
        hold_time = timedelta(seconds=self.config.position_hold_seconds)

        for instrument_id, open_time in list(self._position_open_times.items()):
            if now - open_time > hold_time:
                positions = self.cache.positions_open(instrument_id=instrument_id)
                for position in positions:
                    self._flatten_position(position)

                # Remove from tracking
                self._position_open_times.pop(instrument_id, None)

    def _flatten_position(self, position: Position) -> None:
        """Flatten a position using market order."""
        try:
            instrument = self.cache.instrument(position.instrument_id)
            if instrument is None:
                return

            order = self.order_factory.market(
                instrument_id=position.instrument_id,
                side=position.side.opposite(),
                quantity=position.quantity,
                reduce_only=True,
            )

            self.submit_order(order, client_id=self.config.client_id)
            self._positions_closed += 1

            pnl = position.unrealized_pnl(instrument)
            self.log.info(
                f"Flattened position: {position.instrument_id} "
                f"P&L: {pnl:.4f}",
                color=LogColor.YELLOW,
            )

        except Exception as e:
            self.log.error(f"Failed to flatten position: {e}")
            self._errors += 1

    def _report_status(self, time_event) -> None:
        """
        Report current strategy status.
        """
        open_positions = self.cache.positions_open()
        pnl_total = sum(
            p.unrealized_pnl(self.cache.instrument(p.instrument_id)) or Decimal(0)
            for p in open_positions
        )

        self.log.info("=" * 60)
        self.log.info("STATUS REPORT")
        self.log.info("=" * 60)
        self.log.info(f"Outcome Markets Tracked: {len(self._outcome_instruments)}")
        self.log.info(f"Open Positions: {len(open_positions)}")
        self.log.info(f"Positions Opened (total): {self._positions_opened}")
        self.log.info(f"Positions Closed (total): {self._positions_closed}")
        self.log.info(f"Orders Submitted: {self._orders_submitted}")
        self.log.info(f"Orders Filled: {self._orders_filled}")
        self.log.info(f"Total Unrealized P&L: {pnl_total:.4f} USDH")
        self.log.info(f"Errors: {self._errors}")

        # Log current prices
        for instrument_id, price in list(self._last_prices.items())[:5]:
            self.log.info(f"  {instrument_id}: {price:.4f}")

        self.log.info("=" * 60)

        # Health check
        if self._errors > 100:
            self.log.warning("High error count detected, consider restarting")

    def on_stop(self) -> None:
        """
        Called when the strategy stops.
        """
        self.log.info("=" * 60)
        self.log.info("Outcome Paper Trading Strategy Stopping")
        self.log.info("=" * 60)

        # Cancel all orders
        self.cancel_all_orders()

        # Close all positions
        for position in self.cache.positions_open():
            self._flatten_position(position)

        # Final report
        self._report_status(None)

        self.log.info("Strategy stopped gracefully")


async def main() -> None:
    """
    Main entry point for the paper trading node.
    """
    testnet = os.getenv("HYPERLIQUID_OUTCOME_TESTNET", "1").strip() == "1"
    environment = (
        HyperliquidEnvironment.TESTNET if testnet else HyperliquidEnvironment.MAINNET
    )

    # Configure node
    config_node = TradingNodeConfig(
        trader_id=TraderId("OUTCOME-PAPER-001"),
        logging=LoggingConfig(
            log_level="INFO",
            log_colors=True,
            use_pyo3=True,
        ),
        data_clients={
            HYPERLIQUID: HyperliquidDataClientConfig(
                environment=environment,
                testnet=testnet,
                instrument_provider=InstrumentProviderConfig(load_all=True),
                product_types=(
                    HyperliquidProductType.OUTCOME,
                    # Uncomment to include other product types
                    # HyperliquidProductType.SPOT,
                    # HyperliquidProductType.PERP,
                ),
            ),
        },
        exec_clients={
            "SANDBOX": SandboxExecutionClientConfig(
                venue=HYPERLIQUID,
                base_currency="USDH",
                starting_balances=["10_000 USDH"],
                account_type="MARGIN",
                oms_type="NETTING",
            ),
        },
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )

    node = TradingNode(config=config_node)

    # Create and configure strategy
    strategy = OutcomePaperStrategy(
        config=OutcomePaperStrategyConfig(
            strategy_id="OUTCOME-PAPER-STRATEGY",
            client_id=ClientId("SANDBOX"),
            order_qty=Decimal("100"),
            max_positions=5,
            min_spread_bps=50,
            position_hold_seconds=300,
            status_report_interval_seconds=60,
            price_guardrail=True,
        ),
    )

    node.trader.add_strategy(strategy)

    # Add factories
    node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
    node.add_exec_client_factory("SANDBOX", SandboxLiveExecClientFactory)

    # Build and run
    node.build()

    try:
        await node.run_async()
    except KeyboardInterrupt:
        print("Keyboard interrupt received, shutting down...")
    finally:
        await node.stop_async()
        await asyncio.sleep(1)
        node.dispose()


if __name__ == "__main__":
    asyncio.run(main())
