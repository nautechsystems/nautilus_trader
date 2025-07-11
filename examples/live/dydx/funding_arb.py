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
DYdX Funding Rate Arbitrage Strategy.

This strategy leverages dYdX v4's unique funding rate mechanism by capitalizing on
funding rate differentials between dYdX and other perpetual exchanges.

Key dYdX v4 Features:
- Hourly funding rate updates (vs 8-hour on most exchanges)
- Funding rate transparency through on-chain oracle data
- Multi-collateral support for hedging positions
- Cross-margin efficiency for capital optimization

This implementation monitors funding rate spreads and executes arbitrage positions
when profitable opportunities arise, using dYdX's capital efficiency advantages.

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


class FundingArbConfig:
    """
    Configuration for funding rate arbitrage strategy.
    """

    def __init__(
        self,
        instruments: list[InstrumentId],
        min_funding_diff: Decimal = Decimal("0.0001"),  # 1 bps minimum spread
        max_position_size: Decimal = Decimal("1000.0"),
        hedge_exchanges: list[str] | None = None,
        rebalance_threshold: Decimal = Decimal("0.02"),  # 2% position drift
    ):
        self.instruments = instruments
        self.min_funding_diff = min_funding_diff
        self.max_position_size = max_position_size
        self.hedge_exchanges = hedge_exchanges or ["BINANCE", "BYBIT"]
        self.rebalance_threshold = rebalance_threshold


class FundingArbitrage(Strategy):
    """
    Funding rate arbitrage strategy leveraging dYdX v4's hourly funding updates.

    This strategy monitors funding rate differentials between dYdX and other perpetual
    exchanges, executing arbitrage positions when profitable spreads are identified.

    dYdX v4 Advantages:
    - Hourly funding updates provide more frequent opportunities
    - On-chain oracle data ensures funding rate transparency
    - Multi-collateral support enables efficient hedging
    - Cross-margin system optimizes capital usage

    """

    def __init__(self, config: FundingArbConfig):
        super().__init__(config)
        self.config = config
        self.funding_rates: dict[str, Decimal] = {}
        self.hedge_positions: dict[str, dict[str, Any]] = {}
        self.last_funding_check = 0

        # ⇢ dYdX v4 specific enhancements
        self.funding_history: dict[str, list[dict[str, Any]]] = {}  # Track funding rate history
        self.arbitrage_pnl: dict[str, Decimal] = {}  # Track P&L by instrument
        self.hedge_connections: dict[str, dict[str, Any]] = {}  # External exchange connections
        self.oracle_prices: dict[str, Decimal] = {}  # Oracle price validation
        self.risk_metrics = {
            "total_exposure": Decimal("0"),
            "hedge_ratio": Decimal("0"),
            "funding_earned": Decimal("0"),
            "position_count": 0,
        }

        # ⇢ Circuit breaker for funding rate anomalies
        self.funding_rate_alerts: dict[str, dict[str, Any]] = {}
        self.max_funding_rate_change = Decimal("0.01")  # 1% max hourly change

        # ⇢ Multi-collateral tracking
        self.collateral_usage: dict[str, Decimal] = {}
        self.collateral_weights = {
            "USDC": Decimal("1.0"),
            "USDT": Decimal("0.95"),
            "BTC": Decimal("0.8"),
            "ETH": Decimal("0.85"),
        }

        # Track background tasks
        self._tasks: list[asyncio.Task] = []

    def on_start(self) -> None:
        """
        Initialize funding rate monitoring and hedge connections.
        """
        self.log.info("Starting funding rate arbitrage strategy")

        # Subscribe to funding rate updates for all instruments
        for instrument_id in self.config.instruments:
            self.subscribe_order_book_deltas(instrument_id)
            # ⇢ Subscribe to trade data for P&L tracking
            self.subscribe_trade_ticks(instrument_id)

        # ⇢ Start funding rate monitoring timer (every 30 seconds)
        self.add_timer(30.0, self._check_funding_opportunities)

        # ⇢ Start hourly funding rate update timer
        self.add_timer(3600.0, self._update_funding_rates)

        # ⇢ Start risk monitoring timer (every 5 minutes)
        self.add_timer(300.0, self._monitor_risk_metrics)

        # ⇢ Start hedge position reconciliation timer (every 10 minutes)
        self.add_timer(600.0, self._reconcile_hedge_positions)

        # ⇢ Start oracle price validation timer (every 60 seconds)
        self.add_timer(60.0, self._validate_oracle_prices)

        # ⇢ Start P&L reporting timer (every 30 minutes)
        self.add_timer(1800.0, self._report_arbitrage_pnl)

        # ⇢ Initialize hedge connections
        task = asyncio.create_task(self._initialize_hedge_connections())
        self._tasks.append(task)

        self.log.info(
            f"Monitoring {len(self.config.instruments)} instruments for funding arbitrage",
        )

    def on_stop(self) -> None:
        """
        Clean up positions and connections.
        """
        self.log.info("Stopping funding arbitrage strategy")

        # ⇢ Close all arbitrage positions gracefully
        for instrument_id in self.config.instruments:
            task = asyncio.create_task(self._close_arbitrage_position_advanced(instrument_id))
            self._tasks.append(task)

        # ⇢ Generate final P&L report
        self._generate_final_report()

    async def _check_funding_opportunities(self) -> None:
        """
        Check for profitable funding rate arbitrage opportunities.

        This method compares dYdX funding rates with other exchanges and executes
        arbitrage positions when profitable spreads are found.

        """
        try:
            for instrument_id in self.config.instruments:
                await self._analyze_funding_spread(instrument_id)
        except Exception as e:
            self.log.error(f"Error checking funding opportunities: {e}")

    async def _analyze_funding_spread(self, instrument_id: InstrumentId) -> None:
        """
        Analyze funding rate spread for a specific instrument.

        dYdX v4 provides hourly funding updates, creating more frequent arbitrage
        opportunities compared to traditional 8-hour cycles.

        """
        # Get current dYdX funding rate
        dydx_funding = await self._get_dydx_funding_rate(instrument_id)
        if dydx_funding is None:
            return

        # Compare with other exchanges
        best_hedge_rate = None
        best_hedge_exchange = None

        for exchange in self.config.hedge_exchanges:
            hedge_rate = await self._get_external_funding_rate(exchange, instrument_id)
            if hedge_rate is not None:
                if best_hedge_rate is None or abs(hedge_rate - dydx_funding) > abs(
                    best_hedge_rate - dydx_funding,
                ):
                    best_hedge_rate = hedge_rate
                    best_hedge_exchange = exchange

        if best_hedge_rate is None:
            return

        # Calculate funding spread
        funding_spread = dydx_funding - best_hedge_rate

        # Check if spread exceeds minimum threshold
        if abs(funding_spread) >= self.config.min_funding_diff:
            await self._execute_funding_arbitrage(
                instrument_id,
                funding_spread,
                best_hedge_exchange,
                dydx_funding,
                best_hedge_rate,
            )

    async def _get_dydx_funding_rate(self, instrument_id: InstrumentId) -> Decimal | None:
        """
        Get current funding rate from dYdX v4 with enhanced validation.

        dYdX v4 provides transparent on-chain funding rate data through oracles,
        enabling precise arbitrage calculations with additional validation.

        """
        try:
            # ⇢ In a real implementation, this would query the dYdX v4 indexer API
            # GET /v4/perpetualMarkets/{market}/historicalFundingRates?limit=1

            # Simulate API call with realistic funding rate
            if "BTC" in str(instrument_id):
                base_rate = Decimal("0.0001")
            elif "ETH" in str(instrument_id):
                base_rate = Decimal("0.0002")
            elif "SOL" in str(instrument_id):
                base_rate = Decimal("0.0003")
            else:
                base_rate = Decimal("0.0001")

            # Add some realistic variation
            import secrets

            # Use cryptographically secure random for demo
            variation_pct = (secrets.randbelow(100) - 50) / 100.0  # -50% to +50%
            variation = Decimal(str(variation_pct)) * base_rate
            funding_rate = base_rate + variation

            # ⇢ Validate funding rate change
            if not await self._validate_funding_rate_change(instrument_id, funding_rate):
                self.log.warning(f"Funding rate change validation failed for {instrument_id}")
                return None

            # ⇢ Store in funding history
            if str(instrument_id) not in self.funding_history:
                self.funding_history[str(instrument_id)] = []

            self.funding_history[str(instrument_id)].append(
                {
                    "rate": funding_rate,
                    "timestamp": asyncio.get_event_loop().time(),
                },
            )

            # Keep only last 24 hours of data
            cutoff_time = asyncio.get_event_loop().time() - 86400
            self.funding_history[str(instrument_id)] = [
                entry
                for entry in self.funding_history[str(instrument_id)]
                if entry["timestamp"] > cutoff_time
            ]

            return funding_rate

        except Exception as e:
            self.log.error(f"Error getting dYdX funding rate for {instrument_id}: {e}")
            return None

    async def _get_external_funding_rate(
        self,
        exchange: str,
        instrument_id: InstrumentId,
    ) -> Decimal | None:
        """
        Get funding rate from external exchange with connection validation.
        """
        try:
            # ⇢ Check connection health
            if exchange not in self.hedge_connections:
                self.log.warning(f"No connection established for {exchange}")
                return None

            connection = self.hedge_connections[exchange]
            if not connection.get("connected", False):
                self.log.warning(f"Connection to {exchange} is not active")
                return None

            # ⇢ In a real implementation, this would query the external exchange API
            # for current funding rates with proper error handling and rate limiting

            # Simulate external exchange funding rates
            if exchange == "BINANCE":
                if "BTC" in str(instrument_id):
                    rate = Decimal("0.0002")
                elif "ETH" in str(instrument_id):
                    rate = Decimal("0.0003")
                else:
                    rate = Decimal("0.0002")
            elif exchange == "BYBIT":
                if "BTC" in str(instrument_id):
                    rate = Decimal("0.0003")
                elif "ETH" in str(instrument_id):
                    rate = Decimal("0.0004")
                else:
                    rate = Decimal("0.0003")
            else:
                rate = Decimal("0.0002")

            # ⇢ Add some realistic variation
            import secrets

            # Use cryptographically secure random for demo
            variation_pct = (secrets.randbelow(60) - 30) / 100.0  # -30% to +30%
            variation = Decimal(str(variation_pct)) * rate
            final_rate = rate + variation

            # ⇢ Update connection heartbeat
            connection["last_heartbeat"] = asyncio.get_event_loop().time()

            return final_rate

        except Exception as e:
            self.log.error(f"Error getting {exchange} funding rate for {instrument_id}: {e}")

            # ⇢ Mark connection as failed
            if exchange in self.hedge_connections:
                self.hedge_connections[exchange]["connected"] = False

            return None

    async def _execute_funding_arbitrage(
        self,
        instrument_id: InstrumentId,
        funding_spread: Decimal,
        hedge_exchange: str,
        dydx_rate: Decimal,
        hedge_rate: Decimal,
    ) -> None:
        """
        Execute funding rate arbitrage position with enhanced risk management.

        This method opens positions to profit from funding rate differentials,
        leveraging dYdX v4's multi-collateral and cross-margin advantages.

        """
        try:
            # ⇢ Validate oracle price consistency
            if not await self._validate_oracle_prices():
                self.log.warning("Oracle price validation failed, skipping arbitrage")
                return

            # ⇢ Check collateral availability
            await self._optimize_collateral_usage()

            # ⇢ Calculate position size based on funding spread and risk limits
            position_size = await self._calculate_position_size_advanced(
                funding_spread,
                instrument_id,
                hedge_exchange,
            )

            if position_size <= 0:
                self.log.debug(f"Position size too small for {instrument_id}: {position_size}")
                return

            # ⇢ Determine position direction
            # If dYdX funding > hedge funding, short dYdX and long hedge
            # If dYdX funding < hedge funding, long dYdX and short hedge
            dydx_side = "SELL" if funding_spread > 0 else "BUY"
            hedge_side = "BUY" if funding_spread > 0 else "SELL"

            # ⇢ Execute dYdX position with enhanced order management
            dydx_order = self.order_factory.limit(
                instrument_id=instrument_id,
                order_side=dydx_side,
                quantity=position_size,
                price=await self._get_optimal_execution_price(instrument_id, dydx_side),
                time_in_force="IOC",  # Immediate or cancel
                reduce_only=False,
            )

            self.submit_order(dydx_order)

            # ⇢ Execute hedge position with retry logic
            hedge_success = await self._execute_hedge_position_with_retry(
                instrument_id,
                hedge_exchange,
                hedge_side,
                position_size,
                max_retries=3,
            )

            if not hedge_success:
                self.log.error(f"Failed to execute hedge position for {instrument_id}")
                # Cancel dYdX order if hedge failed
                if dydx_order.is_active:
                    self.cancel_order(dydx_order)
                return

            # ⇢ Log the arbitrage execution with detailed metrics
            self.log.info(
                f"Executed funding arbitrage: {instrument_id} "
                f"dYdX={dydx_rate:.4f} {hedge_exchange}={hedge_rate:.4f} "
                f"spread={funding_spread:.4f} size={position_size} "
                f"expected_profit={self._calculate_expected_profit(funding_spread, position_size):.2f}",
            )

            # ⇢ Update position tracking
            self._update_position_tracking(instrument_id, dydx_side, position_size, funding_spread)

        except Exception as e:
            self.log.error(f"Error executing funding arbitrage: {e}")

    async def _calculate_position_size_advanced(
        self,
        funding_spread: Decimal,
        instrument_id: InstrumentId,
        hedge_exchange: str,
    ) -> Decimal:
        """
        Advanced position size calculation with enhanced risk management.

        Uses dYdX v4's cross-margin efficiency and multi-collateral support to optimize
        capital allocation and risk-adjusted returns.

        """
        try:
            # ⇢ Get current position
            position = self.cache.position(instrument_id)
            current_size = position.quantity if position else Decimal(0)

            # ⇢ Calculate base position size proportional to funding spread
            spread_factor = min(abs(funding_spread) / self.config.min_funding_diff, 5.0)
            base_size = self.config.max_position_size * Decimal(str(spread_factor)) / 10

            # ⇢ Adjust for current market volatility
            market_volatility = await self._calculate_market_volatility(instrument_id)
            volatility_adjustment = max(Decimal("0.5"), 1 - market_volatility)
            adjusted_size = base_size * volatility_adjustment

            # ⇢ Check hedge exchange capacity
            hedge_connection = self.hedge_connections.get(hedge_exchange, {})
            max_hedge_size = hedge_connection.get("api_limits", {}).get(
                "max_order_size",
                Decimal("100"),
            )
            adjusted_size = min(adjusted_size, max_hedge_size)

            # ⇢ Adjust for current position
            if current_size * funding_spread > 0:
                # Already positioned in profitable direction
                return Decimal(0)

            # ⇢ Calculate net position size needed
            target_size = adjusted_size if funding_spread > 0 else -adjusted_size
            position_size = abs(target_size - current_size)

            # ⇢ Apply risk limits
            max_allowed = self.config.max_position_size - abs(current_size)
            final_size = min(position_size, max_allowed)

            # ⇢ Minimum size check
            if final_size < Decimal("0.01"):
                return Decimal(0)

            return final_size

        except Exception as e:
            self.log.error(f"Error calculating advanced position size: {e}")
            return Decimal(0)

    async def _calculate_market_volatility(self, instrument_id: InstrumentId) -> Decimal:
        """
        Calculate current market volatility for position sizing.
        """
        try:
            # Get order book
            book = self.cache.order_book(instrument_id)
            if not book or not book.best_bid_price() or not book.best_ask_price():
                return Decimal("0.02")  # Default volatility

            # Calculate spread as proxy for volatility
            spread = book.best_ask_price() - book.best_bid_price()
            mid_price = (book.best_bid_price() + book.best_ask_price()) / 2

            volatility = spread / mid_price
            return min(volatility, Decimal("0.1"))  # Cap at 10%

        except Exception as e:
            self.log.error(f"Error calculating market volatility: {e}")
            return Decimal("0.02")

    async def _get_optimal_execution_price(self, instrument_id: InstrumentId, side: str) -> Decimal:
        """
        Get optimal execution price for improved fill rates.
        """
        try:
            book = self.cache.order_book(instrument_id)
            if not book or not book.best_bid_price() or not book.best_ask_price():
                return Decimal("0")

            if side == "BUY":
                # Use aggressive pricing for better fill probability
                return book.best_ask_price() * Decimal("1.0001")  # 1 bps above ask
            else:
                return book.best_bid_price() * Decimal("0.9999")  # 1 bps below bid

        except Exception as e:
            self.log.error(f"Error getting optimal execution price: {e}")
            return Decimal("0")

    async def _execute_hedge_position_with_retry(
        self,
        instrument_id: InstrumentId,
        exchange: str,
        side: str,
        size: Decimal,
        max_retries: int = 3,
    ) -> bool:
        """
        Execute hedge position with retry logic.
        """
        try:
            for attempt in range(max_retries):
                success = await self._execute_hedge_position(instrument_id, exchange, side, size)

                if success:
                    return True

                if attempt < max_retries - 1:
                    self.log.warning(
                        f"Hedge execution failed, retrying ({attempt + 1}/{max_retries})",
                    )
                    await asyncio.sleep(1)  # Wait before retry

            return False

        except Exception as e:
            self.log.error(f"Error executing hedge position with retry: {e}")
            return False

    def _calculate_expected_profit(
        self,
        funding_spread: Decimal,
        position_size: Decimal,
    ) -> Decimal:
        """
        Calculate expected profit from funding arbitrage.
        """
        try:
            # Assume position is held for 1 funding period (1 hour on dYdX)
            position_value = position_size * Decimal("1800")  # Placeholder price
            expected_profit = position_value * abs(funding_spread)
            return expected_profit

        except Exception as e:
            self.log.error(f"Error calculating expected profit: {e}")
            return Decimal("0")

    def _update_position_tracking(
        self,
        instrument_id: InstrumentId,
        side: str,
        size: Decimal,
        funding_spread: Decimal,
    ) -> None:
        """
        Update position tracking with trade details.
        """
        try:
            key = str(instrument_id)

            if key not in self.arbitrage_pnl:
                self.arbitrage_pnl[key] = {
                    "trades": [],
                    "total_size": Decimal("0"),
                    "weighted_spread": Decimal("0"),
                }

            trade_info = {
                "timestamp": asyncio.get_event_loop().time(),
                "side": side,
                "size": size,
                "funding_spread": funding_spread,
                "expected_profit": self._calculate_expected_profit(funding_spread, size),
            }

            self.arbitrage_pnl[key]["trades"].append(trade_info)
            self.arbitrage_pnl[key]["total_size"] += size

            # Calculate weighted average spread
            total_weighted_spread = (
                self.arbitrage_pnl[key]["weighted_spread"]
                * (self.arbitrage_pnl[key]["total_size"] - size)
                + funding_spread * size
            )

            self.arbitrage_pnl[key]["weighted_spread"] = (
                total_weighted_spread / self.arbitrage_pnl[key]["total_size"]
            )

        except Exception as e:
            self.log.error(f"Error updating position tracking: {e}")

    async def _execute_hedge_position(
        self,
        instrument_id: InstrumentId,
        exchange: str,
        side: str,
        size: Decimal,
    ) -> None:
        """
        Execute hedge position on external exchange.

        In a real implementation, this would integrate with external exchange APIs.

        """
        # Placeholder for hedge execution
        self.log.info(f"Hedge position: {exchange} {side} {size} {instrument_id}")

        # Store hedge position for tracking
        key = f"{exchange}_{instrument_id}"
        if key not in self.hedge_positions:
            self.hedge_positions[key] = Decimal(0)

        adjustment = size if side == "BUY" else -size
        self.hedge_positions[key] += adjustment

    async def _update_funding_rates(self) -> None:
        """
        Update funding rates for all monitored instruments.

        dYdX v4's hourly funding updates provide more frequent data points for arbitrage
        analysis compared to traditional 8-hour cycles.

        """
        for instrument_id in self.config.instruments:
            dydx_rate = await self._get_dydx_funding_rate(instrument_id)
            if dydx_rate is not None:
                self.funding_rates[f"DYDX_{instrument_id}"] = dydx_rate

            # Update external rates
            for exchange in self.config.hedge_exchanges:
                external_rate = await self._get_external_funding_rate(exchange, instrument_id)
                if external_rate is not None:
                    self.funding_rates[f"{exchange}_{instrument_id}"] = external_rate

    def _close_arbitrage_position(self, instrument_id: InstrumentId) -> None:
        """
        Close arbitrage position for a specific instrument.
        """
        position = self.cache.position(instrument_id)
        if position and position.quantity != 0:
            # Close dYdX position
            close_order = self.order_factory.market(
                instrument_id=instrument_id,
                order_side="SELL" if position.quantity > 0 else "BUY",
                quantity=abs(position.quantity),
                reduce_only=True,
            )
            self.submit_order(close_order)

            # Close hedge positions (placeholder)
            for exchange in self.config.hedge_exchanges:
                key = f"{exchange}_{instrument_id}"
                if key in self.hedge_positions and self.hedge_positions[key] != 0:
                    self.log.info(f"Closing hedge position: {key}")
                    self.hedge_positions[key] = Decimal(0)

    async def _initialize_hedge_connections(self) -> None:
        """
        Initialize connections to external exchanges for hedging.

        This method establishes connections to external exchanges used for hedging
        funding rate arbitrage positions.

        """
        try:
            for exchange in self.config.hedge_exchanges:
                # In a real implementation, this would establish connections
                # to external exchange APIs (Binance, Bybit, etc.)

                self.hedge_connections[exchange] = {
                    "connected": False,
                    "last_ping": 0,
                    "api_key": None,
                    "secret": None,
                    "websocket": None,
                }

                # Initialize connection
                await self._connect_to_exchange(exchange)

            self.log.info(
                f"Initialized hedge connections to {len(self.config.hedge_exchanges)} exchanges",
            )

        except Exception as e:
            self.log.error(f"Error initializing hedge connections: {e}")

    async def _connect_to_exchange(self, exchange: str) -> None:
        """
        Connect to a specific external exchange.
        """
        try:
            # Placeholder for actual exchange connection logic
            # In real implementation, this would use exchange-specific clients

            self.hedge_connections[exchange]["connected"] = True
            self.hedge_connections[exchange]["last_ping"] = asyncio.get_event_loop().time()

            self.log.info(f"Connected to {exchange} for hedging")

        except Exception as e:
            self.log.error(f"Error connecting to {exchange}: {e}")

    async def _monitor_risk_metrics(self) -> None:
        """
        Monitor comprehensive risk metrics for the funding arbitrage strategy.

        This method tracks exposure, hedge ratios, and P&L across all positions.

        """
        try:
            # Calculate total exposure across all instruments
            total_exposure = Decimal("0")
            position_count = 0

            for instrument_id in self.config.instruments:
                position = self.cache.position(instrument_id)
                if position:
                    exposure = abs(position.quantity * position.avg_px_open)
                    total_exposure += exposure
                    position_count += 1

            # Calculate hedge ratio
            hedge_exposure = sum(abs(pos) for pos in self.hedge_positions.values())
            hedge_ratio = hedge_exposure / total_exposure if total_exposure > 0 else Decimal("0")

            # Update risk metrics
            self.risk_metrics.update(
                {
                    "total_exposure": total_exposure,
                    "hedge_ratio": hedge_ratio,
                    "position_count": position_count,
                },
            )

            # Check risk limits
            if total_exposure > self.config.max_position_size * len(self.config.instruments):
                self.log.warning(f"Total exposure exceeds limits: {total_exposure}")

            if hedge_ratio < Decimal("0.8") or hedge_ratio > Decimal("1.2"):
                self.log.warning(f"Hedge ratio out of bounds: {hedge_ratio}")

            self.log.debug(f"Risk metrics: {self.risk_metrics}")

        except Exception as e:
            self.log.error(f"Error monitoring risk metrics: {e}")

    async def _reconcile_hedge_positions(self) -> None:
        """
        Reconcile hedge positions across all external exchanges.

        This method ensures hedge positions are accurate and identifies any
        discrepancies that need correction.

        """
        try:
            for exchange in self.config.hedge_exchanges:
                if not self.hedge_connections[exchange]["connected"]:
                    continue

                # Get actual positions from exchange
                actual_positions = await self._get_actual_hedge_positions(exchange)

                # Compare with tracked positions
                for instrument_id in self.config.instruments:
                    key = f"{exchange}_{instrument_id}"
                    tracked_position = self.hedge_positions.get(key, Decimal("0"))
                    actual_position = actual_positions.get(str(instrument_id), Decimal("0"))

                    discrepancy = abs(tracked_position - actual_position)

                    if discrepancy > Decimal("0.01"):  # 0.01 size tolerance
                        self.log.warning(
                            f"Position discrepancy on {exchange}: {instrument_id} "
                            f"tracked={tracked_position} actual={actual_position}",
                        )

                        # Update tracked position
                        self.hedge_positions[key] = actual_position

        except Exception as e:
            self.log.error(f"Error reconciling hedge positions: {e}")

    async def _get_actual_hedge_positions(self, exchange: str) -> dict[str, Decimal]:
        """
        Get actual positions from external exchange.
        """
        try:
            # In a real implementation, this would query the exchange API
            # for current positions

            # Placeholder implementation
            positions = {}
            for instrument_id in self.config.instruments:
                # Simulate position data
                positions[str(instrument_id)] = Decimal("0")

            return positions

        except Exception as e:
            self.log.error(f"Error getting positions from {exchange}: {e}")
            return {}

    async def _validate_oracle_prices(self) -> None:
        """
        Validate oracle prices for all monitored instruments.

        dYdX v4 uses multiple oracle feeds for price validation. This method ensures our
        funding calculations are based on accurate prices.

        """
        try:
            for instrument_id in self.config.instruments:
                # Get oracle price from dYdX v4
                oracle_price = await self._get_oracle_price(instrument_id)

                if oracle_price:
                    # Get market price for comparison
                    book = self.cache.order_book(instrument_id)
                    if book and book.best_bid_price() and book.best_ask_price():
                        market_price = (book.best_bid_price() + book.best_ask_price()) / 2

                        # Check for significant deviation
                        price_deviation = abs(oracle_price - market_price) / oracle_price

                        if price_deviation > Decimal("0.01"):  # 1% threshold
                            self.log.warning(
                                f"Oracle price deviation for {instrument_id}: "
                                f"oracle={oracle_price} market={market_price} "
                                f"deviation={price_deviation:.3f}",
                            )

                        # Store oracle price for validation
                        self.oracle_prices[str(instrument_id)] = oracle_price

        except Exception as e:
            self.log.error(f"Error validating oracle prices: {e}")

    async def _get_oracle_price(self, instrument_id: InstrumentId) -> Decimal | None:
        """
        Get oracle price for a specific instrument.
        """
        try:
            # In a real implementation, this would query dYdX v4's oracle price feeds
            # via the indexer or chain state

            # Placeholder implementation
            return Decimal("1800.0")  # Example price

        except Exception as e:
            self.log.error(f"Error getting oracle price for {instrument_id}: {e}")
            return None

    async def _report_arbitrage_pnl(self) -> None:
        """
        Report arbitrage P&L across all instruments and exchanges.

        This method provides comprehensive P&L reporting for monitoring the performance
        of funding arbitrage strategies.

        """
        try:
            total_pnl = Decimal("0")

            for instrument_id in self.config.instruments:
                # Get dYdX position P&L
                position = self.cache.position(instrument_id)
                dydx_pnl = position.unrealized_pnl if position else Decimal("0")

                # Get hedge P&L (placeholder)
                hedge_pnl = Decimal("0")
                for exchange in self.config.hedge_exchanges:
                    key = f"{exchange}_{instrument_id}"
                    hedge_position = self.hedge_positions.get(key, Decimal("0"))
                    # Calculate hedge P&L (simplified)
                    hedge_pnl += hedge_position * Decimal("0.01")  # Placeholder calculation

                # Calculate net P&L
                net_pnl = dydx_pnl + hedge_pnl
                total_pnl += net_pnl

                # Store P&L for tracking
                self.arbitrage_pnl[str(instrument_id)] = {
                    "dydx_pnl": dydx_pnl,
                    "hedge_pnl": hedge_pnl,
                    "net_pnl": net_pnl,
                    "timestamp": asyncio.get_event_loop().time(),
                }

            # Update total funding earned
            self.risk_metrics["funding_earned"] = total_pnl

            self.log.info(f"Arbitrage P&L Report: Total={total_pnl:.2f} USDC")

            # Log individual instrument P&L
            for instrument_id, pnl_data in self.arbitrage_pnl.items():
                self.log.info(
                    f"  {instrument_id}: "
                    f"dYdX={pnl_data['dydx_pnl']:.2f} "
                    f"Hedge={pnl_data['hedge_pnl']:.2f} "
                    f"Net={pnl_data['net_pnl']:.2f}",
                )

        except Exception as e:
            self.log.error(f"Error reporting arbitrage P&L: {e}")

    async def _check_funding_rate_anomalies(
        self,
        instrument_id: InstrumentId,
        new_rate: Decimal,
    ) -> bool:
        """
        Check for funding rate anomalies that might indicate data issues.

        This method implements circuit breakers for unusual funding rate movements.

        """
        try:
            key = f"DYDX_{instrument_id}"

            if key in self.funding_rates:
                old_rate = self.funding_rates[key]
                rate_change = abs(new_rate - old_rate)

                # Check for excessive rate change
                if rate_change > self.max_funding_rate_change:
                    self.log.warning(
                        f"Funding rate anomaly detected for {instrument_id}: "
                        f"old={old_rate:.6f} new={new_rate:.6f} change={rate_change:.6f}",
                    )

                    # Store alert
                    self.funding_rate_alerts[key] = {
                        "timestamp": asyncio.get_event_loop().time(),
                        "old_rate": old_rate,
                        "new_rate": new_rate,
                        "change": rate_change,
                    }

                    return False  # Block trading on anomaly

            return True  # Rate change is normal

        except Exception as e:
            self.log.error(f"Error checking funding rate anomalies: {e}")
            return False

    async def _optimize_collateral_usage(self) -> None:
        """
        Optimize collateral usage across dYdX v4's multi-collateral system.

        This method leverages dYdX v4's multi-collateral support to optimize capital
        efficiency and reduce funding costs.

        """
        try:
            # Get account balances for all supported collateral types
            account = self.cache.account()

            for collateral_type, weight in self.collateral_weights.items():
                balance = account.balance(collateral_type) if account else Decimal("0")

                # Calculate weighted collateral value
                weighted_value = balance * weight

                # Track collateral usage
                self.collateral_usage[collateral_type] = {
                    "balance": balance,
                    "weight": weight,
                    "weighted_value": weighted_value,
                    "utilization": (
                        weighted_value / account.equity
                        if account and account.equity > 0
                        else Decimal("0")
                    ),
                }

            # Log collateral optimization opportunities
            total_weighted_value = sum(
                data["weighted_value"] for data in self.collateral_usage.values()
            )

            self.log.debug(
                f"Collateral optimization: Total weighted value={total_weighted_value:.2f}",
            )

        except Exception as e:
            self.log.error(f"Error optimizing collateral usage: {e}")

    def _generate_final_report(self) -> None:
        """
        Generate comprehensive final report for the funding arbitrage strategy.
        """
        try:
            self.log.info("=== FUNDING ARBITRAGE FINAL REPORT ===")

            # Risk metrics
            self.log.info(f"Final Risk Metrics: {self.risk_metrics}")

            # P&L summary
            total_pnl = sum(data["net_pnl"] for data in self.arbitrage_pnl.values())
            self.log.info(f"Total P&L: {total_pnl:.2f} USDC")

            # Funding rate alerts
            if self.funding_rate_alerts:
                self.log.info(f"Funding rate alerts: {len(self.funding_rate_alerts)}")
                for alert in self.funding_rate_alerts.values():
                    self.log.info(f"  Alert: {alert}")

            # Collateral usage
            self.log.info(f"Final Collateral Usage: {self.collateral_usage}")

            # Hedge positions
            self.log.info(f"Final Hedge Positions: {self.hedge_positions}")

        except Exception as e:
            self.log.error(f"Error generating final report: {e}")

    async def _close_arbitrage_position_advanced(self, instrument_id: InstrumentId) -> None:
        """
        Advanced position closure with hedge reconciliation.
        """
        try:
            # Close dYdX position
            position = self.cache.position(instrument_id)
            if position and position.quantity != 0:
                close_order = self.order_factory.market(
                    instrument_id=instrument_id,
                    order_side="SELL" if position.quantity > 0 else "BUY",
                    quantity=abs(position.quantity),
                    reduce_only=True,
                )
                self.submit_order(close_order)

            # Close hedge positions with proper reconciliation
            for exchange in self.config.hedge_exchanges:
                key = f"{exchange}_{instrument_id}"
                if key in self.hedge_positions and self.hedge_positions[key] != 0:
                    await self._close_hedge_position(exchange, instrument_id)

            self.log.info(f"Closed arbitrage position for {instrument_id}")

        except Exception as e:
            self.log.error(f"Error closing arbitrage position for {instrument_id}: {e}")

    async def _close_hedge_position(self, exchange: str, instrument_id: InstrumentId) -> None:
        """
        Close hedge position on specific exchange.
        """
        try:
            key = f"{exchange}_{instrument_id}"
            position_size = self.hedge_positions.get(key, Decimal("0"))

            if position_size == 0:
                return

            # In a real implementation, this would place a closing order
            # on the external exchange

            self.log.info(f"Closing hedge position: {exchange} {instrument_id} {position_size}")

            # Reset tracked position
            self.hedge_positions[key] = Decimal("0")

        except Exception as e:
            self.log.error(f"Error closing hedge position on {exchange}: {e}")

    # Add these methods before the existing methods


# Example configuration and node setup
if __name__ == "__main__":
    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("FUNDING-ARB-001"),
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

    # Configure funding arbitrage strategy
    strategy_config = FundingArbConfig(
        instruments=[
            InstrumentId.from_str("BTC-USD-PERP.DYDX"),
            InstrumentId.from_str("ETH-USD-PERP.DYDX"),
            InstrumentId.from_str("SOL-USD-PERP.DYDX"),
        ],
        min_funding_diff=Decimal("0.0001"),  # 1 bps minimum
        max_position_size=Decimal("1000.0"),
        hedge_exchanges=["BINANCE", "BYBIT"],
        rebalance_threshold=Decimal("0.02"),
    )

    # Instantiate the strategy
    strategy = FundingArbitrage(config=strategy_config)

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
