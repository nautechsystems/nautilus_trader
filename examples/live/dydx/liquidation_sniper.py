#!/usr/bin/env python3
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
"""
DYdX Liquidation Sniper Strategy.

This strategy leverages dYdX v4's unique liquidation mechanism and transparent on-chain
liquidation data to identify and profit from liquidation events.

Key dYdX v4 Features:
- Transparent on-chain liquidation data via CometBFT
- Liquidation auction mechanism with Dutch auction pricing
- Real-time liquidation event notifications
- Cross-margin liquidation thresholds and safety factors
- MEV protection through fair liquidation pricing

This implementation monitors liquidation events and positions to profit from
liquidation auction opportunities while managing risk exposure.

"""

import asyncio
from decimal import Decimal
from typing import Any

from nautilus_trader.adapters.dydx import DYDXDataClientConfig
from nautilus_trader.adapters.dydx import DYDXExecClientConfig
from nautilus_trader.adapters.dydx import DYDXLiveDataClientFactory
from nautilus_trader.adapters.dydx import DYDXLiveExecClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class LiquidationSniperConfig:
    """
    Configuration for liquidation sniper strategy.
    """

    def __init__(
        self,
        target_instruments: list[InstrumentId],
        min_liquidation_size: Decimal = Decimal("1000.0"),  # $1k minimum
        max_position_size: Decimal = Decimal("50000.0"),  # $50k maximum
        liquidation_discount: Decimal = Decimal("0.02"),  # 2% discount target
        risk_buffer: Decimal = Decimal("0.01"),  # 1% safety buffer
        max_auction_duration: int = 30,  # 30 seconds max auction time
    ):
        self.target_instruments = target_instruments
        self.min_liquidation_size = min_liquidation_size
        self.max_position_size = max_position_size
        self.liquidation_discount = liquidation_discount
        self.risk_buffer = risk_buffer
        self.max_auction_duration = max_auction_duration


class LiquidationSniper(Strategy):
    """
    Liquidation sniper strategy for dYdX v4 liquidation auctions.

    This strategy monitors dYdX v4's transparent liquidation system to identify
    profitable liquidation auction opportunities. Key features:

    - Real-time liquidation event monitoring via CometBFT
    - Dutch auction participation with optimal bidding strategy
    - Risk management through position sizing and safety buffers
    - MEV protection through fair liquidation pricing mechanism

    dYdX v4's liquidation system provides transparent on-chain data,
    enabling sophisticated liquidation strategies with reduced information asymmetry.

    """

    def __init__(self, config: LiquidationSniperConfig):
        super().__init__(config)
        self.config = config
        self.active_liquidations: dict[str, dict[str, Any]] = {}
        self.liquidation_history: list[dict[str, Any]] = []
        self.pending_auctions: dict[str, dict[str, Any]] = {}
        self.profit_tracker = Decimal(0)

        # ⇢ dYdX v4 liquidation-specific enhancements
        self.liquidation_stats = {
            "total_events": 0,
            "participated_auctions": 0,
            "successful_bids": 0,
            "total_profit": Decimal("0"),
            "largest_liquidation": Decimal("0"),
            "average_discount": Decimal("0"),
        }

        # ⇢ Penalty calculation tracking
        self.penalty_ratios: dict[str, Decimal] = {}
        self.maintenance_margins: dict[str, Decimal] = {}

        # ⇢ Risk monitoring
        self.current_exposure = Decimal("0")
        self.max_concurrent_liquidations = 5

        # ⇢ Auction timing optimization
        self.auction_timing_stats: dict[str, dict[str, Any]] = {}
        self.optimal_bid_times: dict[str, float] = {}

        # ⇢ MEV protection tracking
        self.mev_protection_events = 0
        self.blocked_liquidations: list[str] = []

        # Track background tasks
        self._tasks: list[asyncio.Task] = []

    def on_start(self) -> None:
        """
        Initialize liquidation monitoring and auction participation.
        """
        self.log.info("Starting liquidation sniper strategy")

        # Subscribe to order book updates for target instruments
        for instrument_id in self.config.target_instruments:
            self.subscribe_order_book_deltas(instrument_id)
            # ⇢ Subscribe to trade ticks for profit tracking
            self.subscribe_trade_ticks(instrument_id)

        # ⇢ Start liquidation monitoring (every 1 second for real-time detection)
        self.add_timer(1.0, self._monitor_liquidations)

        # ⇢ Start auction participation timer (every 0.5 seconds)
        self.add_timer(0.5, self._participate_in_auctions)

        # ⇢ Start risk monitoring (every 10 seconds)
        self.add_timer(10.0, self._monitor_risk_exposure)

        # ⇢ Start penalty calculation updates (every 5 seconds)
        self.add_timer(5.0, self._update_penalty_calculations)

        # ⇢ Start auction timing optimization (every 30 seconds)
        self.add_timer(30.0, self._optimize_auction_timing)

        # ⇢ Start liquidation statistics reporting (every 5 minutes)
        self.add_timer(300.0, self._report_liquidation_stats)

        # ⇢ Start MEV protection monitoring (every 2 seconds)
        self.add_timer(2.0, self._monitor_mev_protection)

        self.log.info(
            f"Liquidation sniper monitoring {len(self.config.target_instruments)} instruments",
        )

    def on_stop(self) -> None:
        """
        Clean up liquidation positions and log final performance.
        """
        self.log.info("Stopping liquidation sniper strategy")

        # ⇢ Close any remaining liquidation positions gracefully
        for instrument_id in self.config.target_instruments:
            task = asyncio.create_task(self._close_liquidation_positions_advanced(instrument_id))
            self._tasks.append(task)

        # ⇢ Generate comprehensive final report
        self._generate_liquidation_report()

        self.log.info(f"Final liquidation sniper profit: ${self.profit_tracker:.2f}")

    async def _monitor_liquidations(self) -> None:
        """
        Monitor for new liquidation events on dYdX v4.

        This method uses dYdX v4's transparent on-chain liquidation data to identify new
        liquidation opportunities in real-time.

        """
        try:
            # ⇢ Get current liquidation events from dYdX v4 with enhanced parsing
            liquidation_events = await self._get_liquidation_events_advanced()

            if not liquidation_events:
                return

            # ⇢ Update total events counter
            self.liquidation_stats["total_events"] += len(liquidation_events)

            # ⇢ Process new liquidation events
            for event in liquidation_events:
                if event["id"] not in self.active_liquidations:
                    await self._process_liquidation_event_advanced(event)

        except Exception as e:
            self.log.error(f"Error monitoring liquidations: {e}")

    async def _process_liquidation_event_advanced(self, event: dict) -> None:
        """
        Process liquidation event with comprehensive analysis.

        This method performs detailed analysis of liquidation events including
        profitability assessment and risk evaluation.

        """
        try:
            liquidation_id = event["id"]
            instrument_id = event["instrument_id"]
            position_size = event["position_size"]

            # ⇢ Check if liquidation meets minimum size requirements
            if position_size < self.config.min_liquidation_size:
                self.log.debug(f"Liquidation {liquidation_id} below minimum size: {position_size}")
                return

            # ⇢ Check if we're not at max concurrent liquidations
            if len(self.active_liquidations) >= self.max_concurrent_liquidations:
                self.log.warning(
                    f"Maximum concurrent liquidations reached: {len(self.active_liquidations)}",
                )
                return

            # ⇢ Calculate initial penalty ratio
            penalty_ratio = await self._calculate_penalty_ratio_advanced(event)
            if penalty_ratio:
                event["current_penalty"] = penalty_ratio

            # ⇢ Add to active liquidations
            self.active_liquidations[liquidation_id] = event

            # ⇢ Track largest liquidation
            if position_size > self.liquidation_stats["largest_liquidation"]:
                self.liquidation_stats["largest_liquidation"] = position_size

            self.log.info(
                (
                    f"New liquidation detected: {liquidation_id} "
                    f"Instrument: {instrument_id} "
                    f"Size: ${position_size:.2f} "
                    f"Penalty: {penalty_ratio:.4f}"
                    if penalty_ratio
                    else ""
                ),
            )

        except Exception as e:
            self.log.error(f"Error processing liquidation event: {e}")

    async def _participate_in_auctions(self) -> None:
        """
        Participate in active liquidation auctions.

        This method evaluates active auctions and submits bids when profitable
        opportunities are identified.

        """
        try:
            current_time = asyncio.get_event_loop().time()

            # ⇢ Check each active liquidation
            for liquidation_id, liquidation_data in list(self.active_liquidations.items()):
                auction_end = liquidation_data.get("auction_end", 0)

                # ⇢ Skip if auction has ended
                if current_time >= auction_end:
                    self.log.debug(f"Auction ended for liquidation {liquidation_id}")
                    del self.active_liquidations[liquidation_id]
                    continue

                # ⇢ Check if we should bid now based on optimal timing
                if await self._should_bid_now(liquidation_data):
                    await self._evaluate_and_bid(liquidation_data)

        except Exception as e:
            self.log.error(f"Error participating in auctions: {e}")

    async def _should_bid_now(self, liquidation_data: dict) -> bool:
        """
        Determine if we should bid now based on optimal timing strategy.

        This method uses historical data to determine the optimal time to submit bids
        for maximum success probability.

        """
        try:
            instrument_id = liquidation_data.get("instrument_id")
            auction_start = liquidation_data.get("auction_start", 0)
            auction_end = liquidation_data.get("auction_end", 0)

            current_time = asyncio.get_event_loop().time()

            # Calculate auction progress
            auction_duration = auction_end - auction_start
            time_elapsed = current_time - auction_start
            auction_progress = time_elapsed / auction_duration if auction_duration > 0 else 0

            # ⇢ Get optimal bid timing for this instrument
            timing_data = self.optimal_bid_times.get(instrument_id, {})
            optimal_progress = timing_data.get("optimal_bid_ratio", 0.7)  # Default 70%

            # ⇢ Check if we're in the optimal bidding window
            if auction_progress >= optimal_progress:
                return True

            # ⇢ Also bid if auction is about to end (emergency bidding)
            if auction_progress >= 0.9:  # 90% through auction
                return True

            return False

        except Exception as e:
            self.log.error(f"Error determining bid timing: {e}")
            return False

    async def _evaluate_and_bid(self, liquidation_data: dict) -> None:
        """
        Evaluate liquidation opportunity and submit bid if profitable.

        This method performs comprehensive evaluation and submits bids for profitable
        liquidation opportunities.

        """
        try:
            liquidation_id = liquidation_data["id"]

            # ⇢ Skip if already bid on this liquidation
            if liquidation_data.get("bid_submitted", False):
                return

            # ⇢ Calculate optimal bid price
            bid_price = await self._calculate_optimal_bid_price(liquidation_data)

            if bid_price <= 0:
                self.log.debug(f"Invalid bid price for liquidation {liquidation_id}")
                return

            # ⇢ Validate profitability
            if not await self._validate_liquidation_profitability(liquidation_data, bid_price):
                self.log.debug(f"Liquidation {liquidation_id} not profitable")
                return

            # ⇢ Check risk limits
            if not await self._check_risk_limits(liquidation_data):
                self.log.warning(f"Risk limits exceeded for liquidation {liquidation_id}")
                return

            # ⇢ Submit bid
            if await self._execute_liquidation_bid(liquidation_data, bid_price):
                liquidation_data["bid_submitted"] = True

        except Exception as e:
            self.log.error(f"Error evaluating and bidding: {e}")

    async def _check_risk_limits(self, liquidation_data: dict) -> bool:
        """
        Check if liquidation meets risk management criteria.
        """
        try:
            position_size = liquidation_data.get("position_size", Decimal("0"))
            index_price = liquidation_data.get("index_price", Decimal("0"))

            # Check position size limits
            if position_size > self.config.max_position_size:
                return False

            # Check total exposure
            liquidation_value = position_size * index_price
            new_exposure = self.current_exposure + liquidation_value

            max_total_exposure = self.config.max_position_size * len(self.config.target_instruments)

            if new_exposure > max_total_exposure:
                return False

            # Check instrument concentration
            instrument_id = liquidation_data.get("instrument_id")
            instrument_exposure = sum(
                data.get("position_size", Decimal("0")) * data.get("index_price", Decimal("0"))
                for data in self.active_liquidations.values()
                if data.get("instrument_id") == instrument_id
            )

            max_instrument_exposure = self.config.max_position_size

            if instrument_exposure > max_instrument_exposure:
                return False

            return True

        except Exception as e:
            self.log.error(f"Error checking risk limits: {e}")
            return False

    async def _monitor_risk_exposure(self) -> None:
        """
        Monitor overall risk exposure from liquidation positions.

        This method tracks total exposure and ensures risk limits are maintained across
        all liquidation activities.

        """
        try:
            total_exposure = Decimal("0")

            # ⇢ Calculate total exposure from active liquidations
            for liquidation_data in self.active_liquidations.values():
                position_size = liquidation_data.get("position_size", Decimal("0"))
                index_price = liquidation_data.get("index_price", Decimal("0"))

                if liquidation_data.get("bid_submitted", False):
                    total_exposure += position_size * index_price

            # ⇢ Add exposure from actual positions
            for instrument_id in self.config.target_instruments:
                position = self.cache.position(instrument_id)
                if position and position.quantity != 0:
                    book = self.cache.order_book(instrument_id)
                    if book and book.best_bid_price() and book.best_ask_price():
                        current_price = (book.best_bid_price() + book.best_ask_price()) / 2
                        total_exposure += abs(position.quantity) * current_price

            # ⇢ Update current exposure
            self.current_exposure = total_exposure

            # ⇢ Check if exposure exceeds limits
            max_exposure = self.config.max_position_size * len(self.config.target_instruments)

            if total_exposure > max_exposure:
                self.log.warning(
                    f"Total exposure exceeds limit: ${total_exposure:.2f} > ${max_exposure:.2f}",
                )

                # Emergency position reduction
                await self._reduce_liquidation_exposure()

        except Exception as e:
            self.log.error(f"Error monitoring risk exposure: {e}")

    async def _reduce_liquidation_exposure(self) -> None:
        """
        Emergency procedure to reduce liquidation exposure.
        """
        try:
            self.log.warning("Initiating emergency exposure reduction")

            # Close positions starting with least profitable
            for instrument_id in self.config.target_instruments:
                position = self.cache.position(instrument_id)
                if position and position.quantity != 0:
                    # Close part of the position
                    close_size = abs(position.quantity) * Decimal("0.5")  # Close 50%

                    close_order = self.order_factory.market(
                        instrument_id=instrument_id,
                        order_side="SELL" if position.quantity > 0 else "BUY",
                        quantity=close_size,
                        reduce_only=True,
                    )

                    self.submit_order(close_order)

                    self.log.info(
                        f"Emergency position reduction: {instrument_id} size: {close_size}",
                    )

        except Exception as e:
            self.log.error(f"Error reducing liquidation exposure: {e}")

        except Exception as e:
            self.log.error(f"Error monitoring liquidations: {e}")

    async def _get_liquidation_events(self) -> list[dict]:
        """
        Get current liquidation events from dYdX v4.

        dYdX v4 provides transparent liquidation data through on-chain events, enabling
        real-time liquidation monitoring and auction participation.

        """
        try:
            # In a real implementation, this would query dYdX v4's liquidation API
            # Example: /v4/liquidations/active or CometBFT event subscription

            # Placeholder implementation
            return [
                {
                    "id": "liq_001",
                    "subaccount": "0x1234567890abcdef",
                    "instrument_id": "BTC-USD-PERP.DYDX",
                    "liquidation_size": Decimal("5000.0"),
                    "liquidation_price": Decimal("45000.0"),
                    "auction_start_time": 1234567890,
                    "auction_end_time": 1234567920,
                    "current_discount": Decimal("0.01"),
                    "status": "active",
                },
            ]

        except Exception as e:
            self.log.error(f"Error fetching liquidation events: {e}")
            return []

    async def _process_liquidation_event(self, event: dict) -> None:
        """
        Process a new liquidation event and evaluate participation opportunity.

        This method analyzes liquidation events to determine if they meet profitability
        and risk criteria for auction participation.

        """
        liquidation_id = event["id"]
        instrument_id = InstrumentId.from_str(event["instrument_id"])
        liquidation_size = event["liquidation_size"]
        # liquidation_price = event["liquidation_price"]  # Not used in this implementation

        # Check if liquidation meets minimum size requirement
        if liquidation_size < self.config.min_liquidation_size:
            return

        # Check if instrument is in target list
        if instrument_id not in self.config.target_instruments:
            return

        # Analyze liquidation profitability
        profitability = await self._analyze_liquidation_profitability(event)

        if profitability >= self.config.liquidation_discount:
            # Add to active liquidations for auction participation
            self.active_liquidations[liquidation_id] = event

            self.log.info(
                f"New liquidation opportunity: {liquidation_id} "
                f"{instrument_id} ${liquidation_size:.2f} "
                f"Expected profit: {profitability:.2%}",
            )

    async def _analyze_liquidation_profitability(self, event: dict) -> Decimal:
        """
        Analyze potential profitability of a liquidation event.

        This method calculates the expected profit from participating in a liquidation
        auction based on current market conditions.

        """
        instrument_id = InstrumentId.from_str(event["instrument_id"])
        liquidation_price = event["liquidation_price"]
        current_discount = event["current_discount"]

        # Get current market price
        book = self.cache.order_book(instrument_id)
        if not book or not book.best_bid_price() or not book.best_ask_price():
            return Decimal(0)

        mid_price = (book.best_bid_price() + book.best_ask_price()) / 2

        # Calculate effective liquidation price with discount
        effective_price = liquidation_price * (1 - current_discount)

        # Calculate potential profit
        if event.get("side") == "SELL":
            # Buying liquidated position, profit = mid_price - effective_price
            profit_per_unit = mid_price - effective_price
        else:
            # Selling liquidated position, profit = effective_price - mid_price
            profit_per_unit = effective_price - mid_price

        # Calculate profit margin
        profit_margin = profit_per_unit / mid_price if mid_price > 0 else Decimal(0)

        # Apply risk buffer
        risk_adjusted_profit = profit_margin - self.config.risk_buffer

        return max(risk_adjusted_profit, Decimal(0))

    async def _participate_in_auctions(self) -> None:
        """
        Participate in active liquidation auctions.

        This method submits bids for profitable liquidation auctions using dYdX v4's
        Dutch auction mechanism.

        """
        try:
            current_time = asyncio.get_event_loop().time()

            for liquidation_id, event in list(self.active_liquidations.items()):
                # Check if auction is still active
                if current_time > event["auction_end_time"]:
                    # Auction ended, remove from active
                    del self.active_liquidations[liquidation_id]
                    continue

                # Check if we should participate in this auction
                if await self._should_participate_in_auction(event):
                    await self._submit_liquidation_bid(event)

        except Exception as e:
            self.log.error(f"Error participating in auctions: {e}")

    async def _should_participate_in_auction(self, event: dict) -> bool:
        """
        Determine if we should participate in a liquidation auction.

        This method evaluates current auction conditions and position limits to decide
        on auction participation.

        """
        instrument_id = InstrumentId.from_str(event["instrument_id"])
        liquidation_size = event["liquidation_size"]

        # Check position limits
        current_position = self.cache.position(instrument_id)
        current_size = current_position.quantity if current_position else Decimal(0)

        if abs(current_size) + liquidation_size > self.config.max_position_size:
            return False

        # Check current profitability
        profitability = await self._analyze_liquidation_profitability(event)

        return profitability >= self.config.liquidation_discount

    async def _submit_liquidation_bid(self, event: dict) -> None:
        """
        Submit a bid for a liquidation auction.

        This method submits orders to participate in dYdX v4's liquidation auction using
        the Dutch auction mechanism.

        """
        liquidation_id = event["id"]
        instrument_id = InstrumentId.from_str(event["instrument_id"])
        liquidation_size = event["liquidation_size"]
        liquidation_price = event["liquidation_price"]
        current_discount = event["current_discount"]

        # Calculate bid price
        bid_price = liquidation_price * (1 - current_discount)

        # Determine order side
        order_side = "BUY" if event.get("side") == "SELL" else "SELL"

        # Submit liquidation order
        liquidation_order = self.order_factory.limit(
            instrument_id=instrument_id,
            order_side=order_side,
            quantity=liquidation_size,
            price=bid_price,
            time_in_force="IOC",  # Immediate or Cancel for auction
            client_order_id=f"LIQ_{liquidation_id}",
        )

        self.submit_order(liquidation_order)

        # Track auction participation
        self.pending_auctions[liquidation_id] = {
            "order": liquidation_order,
            "expected_profit": await self._analyze_liquidation_profitability(event),
            "timestamp": asyncio.get_event_loop().time(),
        }

        self.log.info(
            f"Submitted liquidation bid: {liquidation_id} "
            f"{order_side} {liquidation_size} @ ${bid_price:.2f}",
        )

    async def _monitor_risk_exposure(self) -> None:
        """
        Monitor current risk exposure from liquidation positions.

        This method tracks total position sizes and profit/loss from liquidation
        activities to manage risk exposure.

        """
        try:
            total_exposure = Decimal(0)
            total_pnl = Decimal(0)

            for instrument_id in self.config.target_instruments:
                position = self.cache.position(instrument_id)
                if position:
                    total_exposure += abs(position.quantity * position.avg_px_open)
                    total_pnl += position.unrealized_pnl

            # Log risk metrics
            self.log.debug(
                f"Liquidation risk exposure: "
                f"Total exposure: ${total_exposure:.2f}, "
                f"Unrealized PnL: ${total_pnl:.2f}",
            )

            # Check if exposure exceeds limits
            if total_exposure > self.config.max_position_size * len(self.config.target_instruments):
                self.log.warning(f"High liquidation exposure: ${total_exposure:.2f}")
                await self._reduce_exposure()

        except Exception as e:
            self.log.error(f"Error monitoring risk exposure: {e}")

    async def _reduce_exposure(self) -> None:
        """
        Reduce liquidation exposure by closing positions.
        """
        for instrument_id in self.config.target_instruments:
            position = self.cache.position(instrument_id)
            if position and position.quantity != 0:
                # Close half of the position
                close_size = abs(position.quantity) / 2

                close_order = self.order_factory.market(
                    instrument_id=instrument_id,
                    order_side="SELL" if position.quantity > 0 else "BUY",
                    quantity=close_size,
                    reduce_only=True,
                )

                self.submit_order(close_order)

                self.log.info(f"Reducing liquidation exposure: {instrument_id} {close_size}")

    def _close_liquidation_positions(self, instrument_id: InstrumentId) -> None:
        """
        Close all liquidation positions for a specific instrument.
        """
        position = self.cache.position(instrument_id)
        if position and position.quantity != 0:
            close_order = self.order_factory.market(
                instrument_id=instrument_id,
                order_side="SELL" if position.quantity > 0 else "BUY",
                quantity=abs(position.quantity),
                reduce_only=True,
            )
            self.submit_order(close_order)

    async def _update_penalty_calculations(self) -> None:
        """
        Update penalty ratio calculations for all active liquidations.

        dYdX v4 uses specific penalty calculations for liquidation auctions. This method
        tracks and updates these calculations in real-time.

        """
        try:
            for liquidation_id, liquidation_data in self.active_liquidations.items():
                # ⇢ Calculate exact penalty ratio per dYdX v4 specification
                penalty_ratio = await self._calculate_penalty_ratio_advanced(liquidation_data)

                if penalty_ratio:
                    self.penalty_ratios[liquidation_id] = penalty_ratio

                    # Update liquidation data with new penalty
                    liquidation_data["current_penalty"] = penalty_ratio
                    liquidation_data["last_penalty_update"] = asyncio.get_event_loop().time()

        except Exception as e:
            self.log.error(f"Error updating penalty calculations: {e}")

    async def _calculate_penalty_ratio_advanced(self, liquidation_data: dict) -> Decimal | None:
        """
        Calculate exact penalty ratio based on dYdX v4 liquidation mechanics.

        Formula: penalty_ratio = min(position_size, (deficit * index_price) / 1.015)
        where deficit = maintenance_margin - equity

        """
        try:
            # Extract liquidation details
            deficit = Decimal(liquidation_data.get("deficit", "0"))
            index_price = Decimal(liquidation_data.get("index_price", "0"))
            position_size = Decimal(liquidation_data.get("position_size", "0"))

            if deficit <= 0 or index_price <= 0 or position_size <= 0:
                return None

            # ⇢ Calculate penalty cap based on dYdX v4 formula
            penalty_cap = min(
                position_size,
                (deficit * index_price) / Decimal("1.015"),
            )

            # Calculate penalty ratio
            penalty_ratio = penalty_cap / (position_size * index_price)

            return penalty_ratio

        except Exception as e:
            self.log.error(f"Error calculating penalty ratio: {e}")
            return None

    async def _optimize_auction_timing(self) -> None:
        """
        Optimize auction participation timing based on historical performance.

        This method analyzes past auction results to determine optimal bidding times for
        maximum profit capture.

        """
        try:
            # Analyze historical auction data
            if len(self.liquidation_history) < 10:
                return  # Need more data for optimization

            # Calculate average auction duration and optimal bid timing
            total_duration = sum(event.get("duration", 0) for event in self.liquidation_history)

            if total_duration > 0:
                avg_duration = total_duration / len(self.liquidation_history)

                # Calculate optimal bid timing (typically 60-80% through auction)
                optimal_bid_ratio = Decimal("0.7")  # 70% through auction

                for instrument_id in self.config.target_instruments:
                    self.optimal_bid_times[str(instrument_id)] = {
                        "avg_duration": avg_duration,
                        "optimal_bid_time": avg_duration * float(optimal_bid_ratio),
                        "success_rate": self._calculate_success_rate(instrument_id),
                    }

        except Exception as e:
            self.log.error(f"Error optimizing auction timing: {e}")

    def _calculate_success_rate(self, instrument_id: InstrumentId) -> Decimal:
        """
        Calculate auction success rate for a specific instrument.
        """
        try:
            relevant_events = [
                event
                for event in self.liquidation_history
                if event.get("instrument_id") == str(instrument_id)
            ]

            if not relevant_events:
                return Decimal("0")

            successful_bids = sum(1 for event in relevant_events if event.get("won_auction", False))

            return Decimal(str(successful_bids)) / Decimal(str(len(relevant_events)))

        except Exception as e:
            self.log.error(f"Error calculating success rate: {e}")
            return Decimal("0")

    async def _monitor_mev_protection(self) -> None:
        """
        Monitor MEV protection mechanisms in dYdX v4 liquidations.

        dYdX v4 implements MEV protection through fair liquidation pricing. This method
        tracks when MEV protection is triggered.

        """
        try:
            # Check for MEV protection events
            for liquidation_id, liquidation_data in self.active_liquidations.items():
                # Check if liquidation has MEV protection enabled
                if liquidation_data.get("mev_protection", False):
                    self.mev_protection_events += 1

                    # Adjust bidding strategy for MEV-protected liquidations
                    await self._adjust_bid_for_mev_protection(liquidation_id, liquidation_data)

        except Exception as e:
            self.log.error(f"Error monitoring MEV protection: {e}")

    async def _adjust_bid_for_mev_protection(
        self,
        liquidation_id: str,
        liquidation_data: dict,
    ) -> None:
        """
        Adjust bidding strategy for MEV-protected liquidations.
        """
        try:
            # MEV protection typically requires more conservative bidding
            original_discount = liquidation_data.get(
                "target_discount",
                self.config.liquidation_discount,
            )

            # Reduce discount target for MEV-protected liquidations
            mev_adjusted_discount = original_discount * Decimal("0.8")  # 20% reduction

            liquidation_data["mev_adjusted_discount"] = mev_adjusted_discount
            liquidation_data["mev_protection_active"] = True

            self.log.debug(
                f"Adjusted bid for MEV protection: {liquidation_id}, "
                f"discount: {original_discount:.4f} -> {mev_adjusted_discount:.4f}",
            )

        except Exception as e:
            self.log.error(f"Error adjusting bid for MEV protection: {e}")

    async def _report_liquidation_stats(self) -> None:
        """
        Report comprehensive liquidation statistics.
        """
        try:
            stats = self.liquidation_stats.copy()

            # Calculate success rates
            participation_rate = Decimal("0")
            success_rate = Decimal("0")

            if stats["total_events"] > 0:
                participation_rate = (
                    Decimal(str(stats["participated_auctions"]))
                    / Decimal(str(stats["total_events"]))
                    * 100
                )

            if stats["participated_auctions"] > 0:
                success_rate = (
                    Decimal(str(stats["successful_bids"]))
                    / Decimal(str(stats["participated_auctions"]))
                    * 100
                )

            # Calculate average profit per liquidation
            avg_profit = Decimal("0")
            if stats["successful_bids"] > 0:
                avg_profit = stats["total_profit"] / Decimal(str(stats["successful_bids"]))

            self.log.info(
                f"Liquidation Stats: "
                f"Events: {stats['total_events']}, "
                f"Participation: {participation_rate:.1f}%, "
                f"Success: {success_rate:.1f}%, "
                f"Total Profit: ${stats['total_profit']:.2f}, "
                f"Avg Profit: ${avg_profit:.2f}, "
                f"Largest: ${stats['largest_liquidation']:.2f}, "
                f"MEV Events: {self.mev_protection_events}",
            )

        except Exception as e:
            self.log.error(f"Error reporting liquidation stats: {e}")

    async def _get_liquidation_events_advanced(self) -> list[dict]:
        """
        Get liquidation events with enhanced data parsing.

        This method queries dYdX v4's on-chain liquidation data with comprehensive event
        parsing and validation.

        """
        try:
            # ⇢ In a real implementation, this would query:
            # - CometBFT event log for liquidation events
            # - dYdX v4 indexer for liquidation details
            # - On-chain state for current auction status

            # Simulate realistic liquidation events
            events = []

            # Generate 0-3 liquidation events per call
            import secrets

            num_events = secrets.randbelow(4)  # 0-3 events

            for i in range(num_events):
                # Use cryptographically secure random for demo
                instrument_idx = secrets.randbelow(len(self.config.target_instruments))
                selected_instrument = self.config.target_instruments[instrument_idx]

                position_size = Decimal(str(1000 + secrets.randbelow(49000)))  # 1000-50000
                deficit = Decimal(str(100 + secrets.randbelow(4900)))  # 100-5000
                index_price = Decimal(str(1000 + secrets.randbelow(2000)))  # 1000-3000
                maintenance_margin = Decimal(str(500 + secrets.randbelow(2000)))  # 500-2500
                equity = Decimal(str(secrets.randbelow(2000)))  # 0-2000
                mev_protection = secrets.randbelow(2) == 1  # True/False

                event = {
                    "id": f"liquidation_{int(asyncio.get_event_loop().time())}_{i}",
                    "instrument_id": str(selected_instrument),
                    "position_size": position_size,
                    "deficit": deficit,
                    "index_price": index_price,
                    "maintenance_margin": maintenance_margin,
                    "equity": equity,
                    "timestamp": asyncio.get_event_loop().time(),
                    "auction_start": asyncio.get_event_loop().time(),
                    "auction_end": asyncio.get_event_loop().time() + 30,  # 30 second auction
                    "mev_protection": mev_protection,
                    "liquidator": None,  # Will be filled when auction is won
                    "final_price": None,  # Will be filled when auction completes
                }

                events.append(event)

            return events

        except Exception as e:
            self.log.error(f"Error getting liquidation events: {e}")
            return []

    async def _calculate_optimal_bid_price(self, liquidation_data: dict) -> Decimal:
        """
        Calculate optimal bid price for liquidation auction.

        This method uses dYdX v4's Dutch auction mechanics to determine the optimal bid
        price for maximum profit while ensuring execution.

        """
        try:
            # Extract liquidation parameters
            index_price = liquidation_data.get("index_price", Decimal("0"))
            # penalty_ratio = liquidation_data.get("current_penalty", Decimal("0"))  # Not used in this implementation
            auction_start = liquidation_data.get("auction_start", 0)
            auction_end = liquidation_data.get("auction_end", 0)

            current_time = asyncio.get_event_loop().time()

            # Calculate auction progress (0 to 1)
            auction_duration = auction_end - auction_start
            time_elapsed = current_time - auction_start
            auction_progress = (
                min(time_elapsed / auction_duration, 1.0) if auction_duration > 0 else 0
            )

            # ⇢ Dutch auction price calculation
            # Price starts at index_price and decreases over time
            max_discount = self.config.liquidation_discount

            # Adjust for MEV protection
            if liquidation_data.get("mev_protection_active", False):
                max_discount = liquidation_data.get("mev_adjusted_discount", max_discount)

            # Calculate current auction price
            current_discount = max_discount * Decimal(str(auction_progress))
            auction_price = index_price * (Decimal("1") - current_discount)

            # Apply risk buffer
            risk_adjusted_price = auction_price * (Decimal("1") - self.config.risk_buffer)

            # Ensure minimum profit margin
            min_profit_margin = Decimal("0.005")  # 0.5%
            min_price = index_price * (Decimal("1") - min_profit_margin)

            optimal_price = max(risk_adjusted_price, min_price)

            return optimal_price

        except Exception as e:
            self.log.error(f"Error calculating optimal bid price: {e}")
            return Decimal("0")

    async def _validate_liquidation_profitability(
        self,
        liquidation_data: dict,
        bid_price: Decimal,
    ) -> bool:
        """
        Validate that liquidation opportunity is profitable.

        This method performs comprehensive profitability analysis including fees,
        slippage, and execution risk.

        """
        try:
            index_price = liquidation_data.get("index_price", Decimal("0"))
            position_size = liquidation_data.get("position_size", Decimal("0"))

            if index_price <= 0 or position_size <= 0 or bid_price <= 0:
                return False

            # Calculate gross profit
            gross_profit = (index_price - bid_price) * position_size

            # Estimate execution costs
            trading_fees = position_size * index_price * Decimal("0.0005")  # 5 bps
            slippage_cost = position_size * index_price * Decimal("0.0002")  # 2 bps
            gas_costs = Decimal("5.0")  # $5 gas cost

            total_costs = trading_fees + slippage_cost + gas_costs

            # Calculate net profit
            net_profit = gross_profit - total_costs

            # Check profitability threshold
            min_profit_threshold = Decimal("50.0")  # $50 minimum profit

            if net_profit < min_profit_threshold:
                return False

            # Check profit margin
            profit_margin = net_profit / (position_size * index_price)
            min_profit_margin = Decimal("0.01")  # 1% minimum margin

            if profit_margin < min_profit_margin:
                return False

            # Store profitability metrics
            liquidation_data["estimated_profit"] = net_profit
            liquidation_data["profit_margin"] = profit_margin
            liquidation_data["total_costs"] = total_costs

            return True

        except Exception as e:
            self.log.error(f"Error validating liquidation profitability: {e}")
            return False

    async def _execute_liquidation_bid(self, liquidation_data: dict, bid_price: Decimal) -> bool:
        """
        Execute liquidation auction bid.

        This method submits a bid to the dYdX v4 liquidation auction with proper error
        handling and execution tracking.

        """
        try:
            instrument_id = InstrumentId.from_str(liquidation_data["instrument_id"])
            position_size = liquidation_data["position_size"]
            liquidation_id = liquidation_data["id"]

            # ⇢ Create liquidation bid order
            bid_order = self.order_factory.limit(
                instrument_id=instrument_id,
                order_side="BUY",  # Always buying in liquidation
                quantity=position_size,
                price=bid_price,
                time_in_force="IOC",  # Immediate or cancel
                exec_algorithm_id="LIQUIDATION_BID",
                exec_algorithm_params={"liquidation_id": liquidation_id},
            )

            # Submit bid
            self.submit_order(bid_order)

            # Track auction participation
            self.liquidation_stats["participated_auctions"] += 1

            # Store bid details
            liquidation_data["bid_order"] = bid_order
            liquidation_data["bid_price"] = bid_price
            liquidation_data["bid_timestamp"] = asyncio.get_event_loop().time()

            self.log.info(
                f"Submitted liquidation bid: {liquidation_id} "
                f"Instrument: {instrument_id} "
                f"Size: {position_size} "
                f"Price: {bid_price} "
                f"Est. Profit: ${liquidation_data.get('estimated_profit', 0):.2f}",
            )

            return True

        except Exception as e:
            self.log.error(f"Error executing liquidation bid: {e}")
            return False

    async def _close_liquidation_positions_advanced(self, instrument_id: InstrumentId) -> None:
        """
        Advanced liquidation position closure with P&L tracking.
        """
        try:
            position = self.cache.position(instrument_id)
            if not position or position.quantity == 0:
                return

            # Calculate P&L before closing
            book = self.cache.order_book(instrument_id)
            if book and book.best_bid_price() and book.best_ask_price():
                current_price = (book.best_bid_price() + book.best_ask_price()) / 2
                unrealized_pnl = position.unrealized_pnl(current_price)

                # Close position
                close_order = self.order_factory.market(
                    instrument_id=instrument_id,
                    order_side="SELL" if position.quantity > 0 else "BUY",
                    quantity=abs(position.quantity),
                    reduce_only=True,
                )

                self.submit_order(close_order)

                # Update profit tracking
                self.profit_tracker += unrealized_pnl
                self.liquidation_stats["total_profit"] += unrealized_pnl

                self.log.info(
                    f"Closed liquidation position: {instrument_id} "
                    f"Size: {position.quantity} "
                    f"P&L: ${unrealized_pnl:.2f}",
                )

        except Exception as e:
            self.log.error(f"Error closing liquidation positions: {e}")

    def _generate_liquidation_report(self) -> None:
        """
        Generate comprehensive liquidation performance report.
        """
        try:
            stats = self.liquidation_stats.copy()

            self.log.info("=== LIQUIDATION SNIPER FINAL REPORT ===")
            self.log.info(f"Total Events Detected: {stats['total_events']}")
            self.log.info(f"Auctions Participated: {stats['participated_auctions']}")
            self.log.info(f"Successful Bids: {stats['successful_bids']}")
            self.log.info(
                f"Success Rate: {(stats['successful_bids'] / max(stats['participated_auctions'], 1) * 100):.1f}%",
            )
            self.log.info(f"Total Profit: ${stats['total_profit']:.2f}")
            self.log.info(f"Largest Liquidation: ${stats['largest_liquidation']:.2f}")
            self.log.info(f"Average Discount: {stats['average_discount']:.4f}")
            self.log.info(f"MEV Protection Events: {self.mev_protection_events}")
            self.log.info(f"Blocked Liquidations: {len(self.blocked_liquidations)}")
            self.log.info("======================================")

        except Exception as e:
            self.log.error(f"Error generating liquidation report: {e}")

    # Add these methods before the existing methods


# Example configuration and node setup
if __name__ == "__main__":
    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("LIQUIDATION-SNIPER-001"),
        logging=LoggingConfig(log_level="INFO", use_pyo3=True),
        exec_engine=LiveExecEngineConfig(
            reconciliation=True,
            reconciliation_lookback_mins=1440,
        ),
        cache=CacheConfig(
            timestamps_as_iso8601=True,
            buffer_interval_ms=100,
        ),
        data_clients={
            "DYDX": DYDXDataClientConfig(
                wallet_address=None,  # 'DYDX_WALLET_ADDRESS' env var
                instrument_provider=InstrumentProviderConfig(load_all=True),
                is_testnet=True,
            ),
        },
        exec_clients={
            "DYDX": DYDXExecClientConfig(
                wallet_address=None,  # 'DYDX_WALLET_ADDRESS' env var
                mnemonic=None,  # 'DYDX_MNEMONIC' env var
                instrument_provider=InstrumentProviderConfig(load_all=True),
                is_testnet=True,
            ),
        },
        timeout_connection=20.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )

    # Instantiate the node
    node = TradingNode(config=config_node)

    # Configure liquidation sniper strategy
    strategy_config = LiquidationSniperConfig(
        target_instruments=[
            InstrumentId.from_str("BTC-USD-PERP.DYDX"),
            InstrumentId.from_str("ETH-USD-PERP.DYDX"),
            InstrumentId.from_str("SOL-USD-PERP.DYDX"),
        ],
        min_liquidation_size=Decimal("1000.0"),
        max_position_size=Decimal("50000.0"),
        liquidation_discount=Decimal("0.02"),  # 2% minimum discount
        risk_buffer=Decimal("0.01"),  # 1% safety buffer
        max_auction_duration=30,  # 30 seconds
    )

    # Instantiate the strategy
    strategy = LiquidationSniper(config=strategy_config)

    # Add strategy to node
    node.trader.add_strategy(strategy)

    # Register client factories
    node.add_data_client_factory("DYDX", DYDXLiveDataClientFactory)
    node.add_exec_client_factory("DYDX", DYDXLiveExecClientFactory)
    node.build()

    # Run the strategy
    if __name__ == "__main__":
        try:
            node.run()
        finally:
            node.dispose()
