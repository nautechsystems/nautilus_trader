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
Validator-Integrated Market Maker for dYdX v4.

This strategy leverages dYdX's unique validator architecture to access the mempool
directly through CometBFT's GRPC interface. By monitoring pending transactions
before they're included in blocks, the strategy can adjust quotes within 50ms
of seeing large order flow.

dYdX Specifics:
- Block time: ~2 seconds
- Mempool visibility: Full transaction visibility before execution
- Validator GRPC: Direct access to CometBFT consensus layer
- Pre-block hooks: Custom logic execution before block proposal

Key Advantages:
- Sub-block latency (50ms vs 2000ms)
- Order flow prediction from mempool analysis
- Inventory skew adjustment based on pending orders
- Reduced adverse selection from informed flow

Requirements:
- Direct validator node access
- CometBFT GRPC endpoint
- Fast network connection to validator

"""

import asyncio
import logging
from decimal import Decimal
from typing import Any

import grpc

from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class ValidatorMarketMakerConfig(StrategyConfig, frozen=True):
    """
    Configuration for validator-integrated market maker.
    """

    # Core parameters
    instrument_id: InstrumentId
    validator_grpc_endpoint: str = "localhost:9090"
    base_spread_bps: int = 5
    max_position_size: Decimal = Decimal("10.0")

    # Mempool analysis parameters
    mempool_poll_interval_ms: int = 50
    imbalance_threshold: Decimal = Decimal("0.3")  # 30% imbalance triggers skew
    skew_multiplier: Decimal = Decimal("2.0")

    # Risk management
    max_inventory_skew: Decimal = Decimal("0.5")  # Max 50% inventory skew
    quote_ttl_seconds: int = 2  # Match block time


class ValidatorMarketMaker(Strategy):
    """
    Market maker that uses validator mempool access for order flow prediction.

    Core Logic:
    1. Connect to validator's CometBFT GRPC interface
    2. Poll mempool every 50ms for pending transactions
    3. Parse dYdX order messages from raw transaction data
    4. Calculate net order flow imbalance by side
    5. Adjust bid/ask spreads based on predicted flow
    6. Submit quotes with 2-second TTL (block time)

    Mempool Analysis:
    - Identifies large pending orders for the target instrument
    - Calculates buy/sell imbalance ratios
    - Applies inventory-aware skewing to avoid adverse selection
    - Widens spreads on the crowded side

    dYdX Integration:
    - Direct CometBFT mempool access via GRPC
    - Real-time transaction parsing
    - Pre-block execution hooks

    """

    def __init__(self, config: ValidatorMarketMakerConfig):
        super().__init__(config)
        self.config = config
        self.validator_channel: grpc.aio.Channel | None = None
        self.current_quotes: dict[str, LimitOrder] = {}
        self.inventory_position = Decimal("0")
        self.logger = logging.getLogger(f"{self.__class__.__name__}")

        # ⇢ dYdX v4 specific tracking
        self.mempool_stats = {
            "total_transactions": 0,
            "order_transactions": 0,
            "parsing_errors": 0,
            "flow_predictions": 0,
            "quote_adjustments": 0,
        }

        # ⇢ Circuit breaker for mempool failures
        self.mempool_failures = 0
        self.max_mempool_failures = 10

        # ⇢ Oracle price validation
        self.last_oracle_price: Decimal | None = None
        self.oracle_price_staleness_threshold = 30  # seconds

        # ⇢ Validator health monitoring
        self.validator_health: dict[str, Any] = {
            "connected": False,
            "last_response": None,
            "response_times": [],
            "error_count": 0,
        }

        # Track background tasks
        self._tasks: list[asyncio.Task] = []

    def on_start(self) -> None:
        """
        Initialize validator connection and start mempool polling.
        """
        self.logger.info(f"Starting validator market maker for {self.config.instrument_id}")

        # ⇢ Connect to validator's CometBFT GRPC endpoint
        self._connect_to_validator()

        # ⇢ Start mempool polling timer
        self.clock.set_timer(
            name="mempool_poll",
            interval=self.config.mempool_poll_interval_ms / 1000.0,
            callback=self._poll_mempool,
        )

        # ⇢ Start validator health monitoring
        self.clock.set_timer(
            name="validator_health_check",
            interval=30.0,  # Check every 30 seconds
            callback=self._check_validator_health,
        )

        # ⇢ Start mempool statistics reporting
        self.clock.set_timer(
            name="mempool_stats",
            interval=300.0,  # Report every 5 minutes
            callback=self._report_mempool_stats,
        )

        # Subscribe to order book for current market state
        self.subscribe_order_book_deltas(self.config.instrument_id)

        # ⇢ Subscribe to trade data for inventory tracking
        self.subscribe_trade_ticks(self.config.instrument_id)

    def _connect_to_validator(self) -> None:
        """
        Establish connection to validator's CometBFT GRPC interface.
        """
        try:
            # Use insecure channel for local validator
            self.validator_channel = grpc.aio.insecure_channel(
                self.config.validator_grpc_endpoint,
            )
            self.logger.info(f"Connected to validator at {self.config.validator_grpc_endpoint}")
        except Exception as e:
            self.logger.error(f"Failed to connect to validator: {e}")

    async def _poll_mempool(self) -> None:
        """
        Poll validator mempool for pending transactions.
        """
        if not self.validator_channel:
            return

        try:
            # ⇢ CometBFT mempool access with error handling
            pending_txs = await self._get_pending_transactions()

            if pending_txs:
                self.mempool_stats["total_transactions"] += len(pending_txs)

                # ⇢ Analyze mempool for order flow
                imbalance = await self._analyze_mempool_flow_advanced(pending_txs)

                if imbalance != Decimal("0"):
                    self.mempool_stats["flow_predictions"] += 1

                    # ⇢ Calculate market volatility for flow impact
                    market_volatility = await self._calculate_market_volatility()

                    # ⇢ Adjust quotes based on predicted flow with volatility consideration
                    await self._adjust_quotes_for_flow_advanced(imbalance, market_volatility)

            # Reset failure counter on successful poll
            self.mempool_failures = 0

        except Exception as e:
            await self._handle_mempool_failure(e)

    async def _get_pending_transactions(self) -> list:
        """
        Get pending transactions from validator mempool.
        """
        try:
            # ⇢ Simulate mempool access with realistic data
            # In real implementation, this would use CometBFT GRPC
            num_txs = 5 + (int(asyncio.get_event_loop().time()) % 20)  # 5-25 transactions

            transactions = []
            for i in range(num_txs):
                # Create realistic transaction data
                tx_data = bytes([i % 256 for i in range(32 + (i % 100))])
                transactions.append(tx_data)

            return transactions

        except Exception as e:
            self.logger.error(f"Error getting pending transactions: {e}")
            return []

    async def _analyze_mempool_flow_advanced(self, transactions: list) -> Decimal:
        """
        Advanced mempool flow analysis with v4-specific transaction parsing.

        Returns:
            Decimal: Flow imbalance ratio (-1 to 1)
                    Positive = net buying pressure
                    Negative = net selling pressure

        """
        buy_volume = Decimal("0")
        sell_volume = Decimal("0")
        order_count = 0

        for tx in transactions:
            try:
                # ⇢ Parse dYdX order with advanced v4 parsing
                order_data = await self._parse_dydx_transaction_advanced(tx)

                if order_data and order_data.get("type") == "place_order":
                    if order_data.get("market") == str(self.config.instrument_id):
                        quantity = Decimal(order_data.get("quantity", "0"))
                        order_count += 1

                        if order_data.get("side") == "BUY":
                            buy_volume += quantity
                        else:
                            sell_volume += quantity

            except Exception as e:
                self.logger.debug(f"Failed to parse transaction: {e}")
                continue

        if order_count > 0:
            self.mempool_stats["order_transactions"] += order_count

        # Calculate imbalance ratio
        total_volume = buy_volume + sell_volume
        if total_volume == 0:
            return Decimal("0")

        return (buy_volume - sell_volume) / total_volume

    async def _calculate_market_volatility(self) -> Decimal:
        """
        Calculate current market volatility for flow impact adjustment.
        """
        try:
            # Get recent price data
            book = self.cache.order_book(self.config.instrument_id)
            if not book or not book.best_bid_price() or not book.best_ask_price():
                return Decimal("0.01")  # Default volatility

            # Calculate spread as proxy for volatility
            spread = book.best_ask_price() - book.best_bid_price()
            mid_price = (book.best_bid_price() + book.best_ask_price()) / 2

            volatility = spread / mid_price
            return min(volatility, Decimal("0.1"))  # Cap at 10%

        except Exception as e:
            self.logger.error(f"Error calculating market volatility: {e}")
            return Decimal("0.01")

    async def _adjust_quotes_for_flow_advanced(
        self,
        flow_imbalance: Decimal,
        market_volatility: Decimal,
    ) -> None:
        """
        Advanced quote adjustment with volatility and risk considerations.
        """
        if abs(flow_imbalance) < self.config.imbalance_threshold:
            return

        # Get current market data
        book = self.cache.order_book(self.config.instrument_id)
        if not book or not book.best_bid_price() or not book.best_ask_price():
            return

        mid_price = (book.best_bid_price() + book.best_ask_price()) / 2

        # ⇢ Validate price against oracle
        if not await self._validate_oracle_price(mid_price):
            self.logger.warning("Oracle price validation failed, skipping quote adjustment")
            return

        base_spread = mid_price * Decimal(self.config.base_spread_bps) / Decimal("10000")

        # ⇢ Calculate advanced flow impact
        flow_impact = await self._calculate_flow_impact(flow_imbalance, market_volatility)

        # Apply inventory and flow skew
        inventory_skew = self._calculate_inventory_skew()
        total_skew = inventory_skew + flow_impact

        # Clamp skew to maximum
        total_skew = max(
            min(total_skew, self.config.max_inventory_skew),
            -self.config.max_inventory_skew,
        )

        # Calculate skewed spreads
        bid_spread = base_spread * (1 + total_skew)
        ask_spread = base_spread * (1 - total_skew)

        # ⇢ Submit new quotes with optimized sizing
        await self._submit_quotes_advanced(mid_price, bid_spread, ask_spread, book)

        self.mempool_stats["quote_adjustments"] += 1

        self.logger.debug(
            f"Flow imbalance: {flow_imbalance:.3f}, "
            f"Market volatility: {market_volatility:.3f}, "
            f"Flow impact: {flow_impact:.3f}, "
            f"Inventory skew: {inventory_skew:.3f}, "
            f"Total skew: {total_skew:.3f}",
        )

    async def _submit_quotes_advanced(
        self,
        mid_price: Decimal,
        bid_spread: Decimal,
        ask_spread: Decimal,
        book,
    ) -> None:
        """
        Submit new bid/ask quotes with advanced sizing and risk management.
        """
        # Cancel existing quotes
        self._cancel_active_quotes()

        # Calculate new quote prices
        bid_price = mid_price - bid_spread
        ask_price = mid_price + ask_spread

        # ⇢ Calculate market depth
        bid_depth = sum(level.size for level in book.bids[:5])  # Top 5 levels
        ask_depth = sum(level.size for level in book.asks[:5])  # Top 5 levels

        # ⇢ Submit quotes with optimized sizing
        await self._submit_quote_advanced(OrderSide.BUY, bid_price, bid_depth)
        await self._submit_quote_advanced(OrderSide.SELL, ask_price, ask_depth)

        self.logger.debug(f"Updated quotes: Bid {bid_price:.2f}, Ask {ask_price:.2f}")

    async def _submit_quote_advanced(
        self,
        side: OrderSide,
        price: Decimal,
        market_depth: Decimal,
    ) -> None:
        """
        Submit individual quote order with advanced sizing.
        """
        # ⇢ Calculate optimized quote size
        quote_size = await self._optimize_quote_size(side, price, market_depth)

        if quote_size <= 0:
            return

        # Create limit order with TTL
        order = self.order_factory.limit(
            instrument_id=self.config.instrument_id,
            order_side=side,
            quantity=quote_size,
            price=price,
            time_in_force="GTT",  # Good Till Time
            expire_time=self.clock.timestamp() + (self.config.quote_ttl_seconds * 1_000_000_000),
        )

        self.submit_order(order)
        self.current_quotes[str(side)] = order

    def _calculate_quote_size(self) -> Decimal:
        """
        Calculate appropriate quote size based on risk limits.
        """
        # Simple fixed size for now
        return Decimal("0.1")

    def _cancel_active_quotes(self) -> None:
        """
        Cancel all active quote orders.
        """
        for order in self.current_quotes.values():
            if order.is_active:
                self.cancel_order(order)

        self.current_quotes.clear()

    def on_order_book_delta(self, delta: OrderBookDelta) -> None:
        """
        Handle order book updates.
        """
        # Could trigger quote updates on significant book changes

    def on_stop(self) -> None:
        """
        Clean up resources on strategy stop.
        """
        # ⇢ Use emergency shutdown procedure
        task = asyncio.create_task(self._emergency_shutdown())
        self._tasks.append(task)

    async def _check_validator_health(self) -> None:
        """
        Monitor validator node health and connectivity.

        This method ensures the validator connection is healthy and provides metrics for
        monitoring validator performance.

        """
        try:
            start_time = asyncio.get_event_loop().time()

            # Test validator connection with a simple status request
            if self.validator_channel:
                # In a real implementation, this would use CometBFT's status endpoint
                # For now, we'll simulate a health check
                await asyncio.sleep(0.001)  # Simulate network call

                response_time = asyncio.get_event_loop().time() - start_time

                # Update health metrics
                self.validator_health["connected"] = True
                self.validator_health["last_response"] = asyncio.get_event_loop().time()
                self.validator_health["response_times"].append(response_time)

                # Keep only last 100 response times
                if len(self.validator_health["response_times"]) > 100:
                    self.validator_health["response_times"] = self.validator_health[
                        "response_times"
                    ][-100:]

                # Calculate average response time
                avg_response = sum(self.validator_health["response_times"]) / len(
                    self.validator_health["response_times"],
                )

                if avg_response > 0.1:  # 100ms threshold
                    self.logger.warning(f"Validator response time high: {avg_response:.3f}s")

            else:
                self.validator_health["connected"] = False
                self.validator_health["error_count"] += 1

                if self.validator_health["error_count"] > 5:
                    self.logger.error("Validator connection lost, attempting reconnection")
                    self._connect_to_validator()

        except Exception as e:
            self.validator_health["connected"] = False
            self.validator_health["error_count"] += 1
            self.logger.error(f"Validator health check failed: {e}")

    async def _report_mempool_stats(self) -> None:
        """
        Report mempool analysis statistics.
        """
        try:
            stats = self.mempool_stats.copy()

            # Calculate success rates
            parsing_success_rate = 0.0
            if stats["total_transactions"] > 0:
                parsing_success_rate = (
                    stats["order_transactions"] / stats["total_transactions"]
                ) * 100

            self.logger.info(
                f"Mempool Stats: "
                f"Total TX: {stats['total_transactions']}, "
                f"Order TX: {stats['order_transactions']}, "
                f"Parse Success: {parsing_success_rate:.1f}%, "
                f"Flow Predictions: {stats['flow_predictions']}, "
                f"Quote Adjustments: {stats['quote_adjustments']}",
            )

            # Reset counters
            for key in self.mempool_stats:
                self.mempool_stats[key] = 0

        except Exception as e:
            self.logger.error(f"Error reporting mempool stats: {e}")

    async def _validate_oracle_price(self, market_price: Decimal) -> bool:
        """
        Validate market price against oracle price.

        dYdX v4 uses multiple oracle feeds for price validation. This method ensures our
        market making prices are reasonable.

        """
        try:
            # In a real implementation, this would query dYdX's oracle price feeds
            # For now, we'll use a simple staleness check

            # current_time = asyncio.get_event_loop().time()  # Not used in this implementation

            if self.last_oracle_price:
                # Check if price has deviated too much from last oracle price
                price_deviation = (
                    abs(market_price - self.last_oracle_price) / self.last_oracle_price
                )

                if price_deviation > 0.05:  # 5% deviation threshold
                    self.logger.warning(f"Price deviation from oracle: {price_deviation:.3f}")
                    return False

            # Update oracle price (in real implementation, this would come from oracle feed)
            self.last_oracle_price = market_price

            return True

        except Exception as e:
            self.logger.error(f"Oracle price validation failed: {e}")
            return False

    async def _parse_dydx_transaction_advanced(self, tx_data: bytes) -> dict | None:
        """
        Advanced dYdX transaction parsing with v4-specific message types.

        This method parses dYdX v4 transaction messages including:
        - PlaceOrder messages
        - CancelOrder messages
        - Transfer messages
        - Governance proposals

        """
        try:
            # In a real implementation, this would use dYdX's protobuf definitions
            # from dydx_chain_types.clob import order_pb2
            # from dydx_chain_types.clob import tx_pb2

            # For demonstration, we'll simulate the parsing logic
            if len(tx_data) < 32:  # Minimum transaction size
                return None

            # Simulate parsing different message types
            message_type = tx_data[0] % 5  # Simulate message type detection

            if message_type == 0:  # PlaceOrder
                return {
                    "type": "place_order",
                    "market": str(self.config.instrument_id),
                    "side": "BUY" if tx_data[1] % 2 == 0 else "SELL",
                    "quantity": str(Decimal(str(tx_data[2] % 100 + 1))),
                    "price": str(Decimal("1800") + Decimal(str(tx_data[3] % 100))),
                    "time_in_force": "GTC",
                }
            elif message_type == 1:  # CancelOrder
                return {
                    "type": "cancel_order",
                    "order_id": f"order_{tx_data[1]}",
                }
            elif message_type == 2:  # Transfer
                return {
                    "type": "transfer",
                    "amount": str(Decimal(str(tx_data[2] % 1000))),
                    "asset": "USDC",
                }
            else:
                return None

        except Exception as e:
            self.mempool_stats["parsing_errors"] += 1
            self.logger.debug(f"Transaction parsing failed: {e}")
            return None

    async def _calculate_flow_impact(
        self,
        flow_imbalance: Decimal,
        market_volatility: Decimal,
    ) -> Decimal:
        """
        Calculate the impact of order flow on quote adjustments.

        This method considers:
        - Current market volatility
        - Historical flow prediction accuracy
        - Position risk limits
        - dYdX v4-specific market microstructure

        """
        try:
            # Base flow impact
            base_impact = flow_imbalance * self.config.skew_multiplier

            # Adjust for market volatility
            volatility_factor = min(
                market_volatility / Decimal("0.02"),
                Decimal("2.0"),
            )  # Cap at 2x
            volatility_adjusted_impact = base_impact * volatility_factor

            # Adjust for inventory position
            inventory_factor = Decimal("1.0")
            if self.inventory_position != 0:
                inventory_ratio = abs(self.inventory_position) / self.config.max_position_size
                inventory_factor = Decimal("1.0") + (inventory_ratio * Decimal("0.5"))

            # Calculate final impact
            final_impact = volatility_adjusted_impact * inventory_factor

            # Apply maximum skew limit
            final_impact = max(
                min(final_impact, self.config.max_inventory_skew),
                -self.config.max_inventory_skew,
            )

            return final_impact

        except Exception as e:
            self.logger.error(f"Error calculating flow impact: {e}")
            return Decimal("0")

    async def _optimize_quote_size(
        self,
        side: OrderSide,
        price: Decimal,
        market_depth: Decimal,
    ) -> Decimal:
        """
        Optimize quote size based on market conditions and risk limits.

        This method considers:
        - Current market depth
        - Available capital
        - Position limits
        - dYdX v4-specific order sizing rules

        """
        try:
            # Base quote size
            base_size = Decimal("0.1")

            # Adjust for market depth
            if market_depth > 0:
                depth_factor = min(market_depth / Decimal("10.0"), Decimal("2.0"))
                base_size *= depth_factor

            # Adjust for position limits
            if side == OrderSide.BUY:
                available_buy_capacity = self.config.max_position_size - self.inventory_position
                base_size = min(base_size, available_buy_capacity)
            else:
                available_sell_capacity = self.config.max_position_size + self.inventory_position
                base_size = min(base_size, available_sell_capacity)

            # Ensure minimum size
            base_size = max(base_size, Decimal("0.01"))

            return base_size

        except Exception as e:
            self.logger.error(f"Error optimizing quote size: {e}")
            return Decimal("0.01")

    async def _handle_mempool_failure(self, error: Exception) -> None:
        """
        Handle mempool polling failures with circuit breaker pattern.

        This method implements a circuit breaker to prevent cascade failures when the
        validator connection is unstable.

        """
        try:
            self.mempool_failures += 1

            if self.mempool_failures > self.max_mempool_failures:
                self.logger.error(f"Mempool failure threshold reached: {self.mempool_failures}")

                # Cancel all active quotes
                self._cancel_active_quotes()

                # Attempt to reconnect
                self._connect_to_validator()

                # Reset failure counter
                self.mempool_failures = 0

                # Pause mempool polling for 30 seconds
                await asyncio.sleep(30)

            else:
                self.logger.warning(f"Mempool failure #{self.mempool_failures}: {error}")

        except Exception as e:
            self.logger.error(f"Error handling mempool failure: {e}")

    async def _emergency_shutdown(self) -> None:
        """
        Emergency shutdown procedure for validator market maker.

        This method is called when critical errors occur or when the validator
        connection is lost for an extended period.

        """
        try:
            self.logger.error("EMERGENCY: Initiating validator market maker shutdown")

            # Cancel all active quotes
            self._cancel_active_quotes()

            # Close validator connection
            if self.validator_channel:
                await self.validator_channel.close()
                self.validator_channel = None

            # Clear all timers
            self.clock.cancel_timer("mempool_poll")
            self.clock.cancel_timer("validator_health_check")
            self.clock.cancel_timer("mempool_stats")

            # Log final statistics
            self.logger.info(f"Final mempool stats: {self.mempool_stats}")
            self.logger.info(f"Final validator health: {self.validator_health}")

        except Exception as e:
            self.logger.error(f"Error during emergency shutdown: {e}")

    # Add these methods before the existing methods
