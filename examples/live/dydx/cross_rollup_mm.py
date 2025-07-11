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
DYdX Cross-Rollup Market Maker Strategy.

This strategy leverages dYdX v4's unique cross-rollup capabilities to provide liquidity
across multiple rollup chains and capture arbitrage opportunities between chains.

Key dYdX v4 Features:
- Cross-rollup state synchronization via Cosmos IBC
- Multi-chain asset bridging and settlement
- Atomic cross-chain transactions with oracle proof attachment
- Unified liquidity pools across rollup chains
- Chain-specific gas optimization and fee structures
- Intent-based cross-chain execution with retry logic

This implementation provides market making services across multiple rollup chains
while managing cross-chain position risk and optimizing for chain-specific conditions.

"""

import asyncio
from decimal import Decimal
from typing import Any

from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

ONE_BP = Decimal("0.0001")  # ⇢ basis point constant


class CrossRollupMMConfig:
    """
    Configuration for cross-rollup market maker strategy.
    """

    def __init__(
        self,
        target_chains: list[str],
        instruments_per_chain: dict[str, list[InstrumentId]],
        base_spread_bps: int = 20,
        cross_chain_spread_bps: int = 50,
        max_position_per_chain: Decimal = Decimal("10000.0"),
        bridge_threshold: Decimal = Decimal("1000.0"),
        gas_optimization: bool = True,
        min_compound_usd: Decimal = Decimal("20.0"),  # ⇢ minimum compound threshold
        max_gas_ratio: Decimal = Decimal("0.66"),  # ⇢ max gas cost as % of compound
        oracle_proof_required: bool = True,  # ⇢ attach oracle proofs to IBC transfers
        ibc_timeout_seconds: int = 45,  # ⇢ IBC transfer timeout
        ibc_retry_attempts: int = 3,  # ⇢ retry attempts for failed transfers
    ):
        self.target_chains = target_chains
        self.instruments_per_chain = instruments_per_chain
        self.base_spread_bps = base_spread_bps
        self.cross_chain_spread_bps = cross_chain_spread_bps
        self.max_position_per_chain = max_position_per_chain
        self.bridge_threshold = bridge_threshold
        self.gas_optimization = gas_optimization
        self.min_compound_usd = min_compound_usd  # ⇢ new attributes
        self.max_gas_ratio = max_gas_ratio
        self.oracle_proof_required = oracle_proof_required
        self.ibc_timeout_seconds = ibc_timeout_seconds
        self.ibc_retry_attempts = ibc_retry_attempts


class CrossRollupMarketMaker(Strategy):
    """
    Cross-rollup market maker strategy for dYdX v4 multi-chain ecosystem.

    This strategy provides liquidity across multiple rollup chains connected
    to dYdX v4's main chain via Cosmos IBC, enabling:

    - Cross-chain arbitrage opportunities
    - Chain-specific liquidity provision
    - Atomic cross-chain position management
    - Gas optimization across different rollup chains
    - Unified risk management across all chains

    dYdX v4's cross-rollup architecture enables seamless asset movement
    and position synchronization across multiple chains.

    """

    def __init__(self, config: CrossRollupMMConfig):
        super().__init__(config)
        self.config = config
        self.chain_connections: dict[str, dict[str, Any]] = {}
        self.cross_chain_positions: dict[str, dict[str, Any]] = {}
        self.bridge_queues: dict[str, list[dict[str, Any]]] = {}
        self.gas_prices: dict[str, Decimal] = {}
        self.ibc_channels: dict[str, dict[str, Any]] = {}  # ⇢ IBC channel mapping
        self.pending_transfers: dict[str, dict[str, Any]] = {}  # ⇢ track pending IBC transfers
        self.oracle_proofs: dict[str, dict[str, Any]] = {}  # ⇢ cached oracle proofs

        # Track background tasks
        self._tasks: list[asyncio.Task] = []

    def on_start(self) -> None:
        """
        Initialize cross-rollup connections and market making.
        """
        self.log.info("Starting cross-rollup market maker strategy")

        # Initialize connections to all target chains
        for chain in self.config.target_chains:
            task = asyncio.create_task(self._initialize_chain_connection(chain))
            self._tasks.append(task)

        # Subscribe to instruments across all chains
        for chain, instruments in self.config.instruments_per_chain.items():
            for instrument_id in instruments:
                self.subscribe_order_book_deltas(instrument_id)

        # Start cross-chain arbitrage monitoring
        self.add_timer(5.0, self._monitor_cross_chain_arbitrage)

        # Start position synchronization
        self.add_timer(30.0, self._sync_cross_chain_positions)

        # Start gas price monitoring
        self.add_timer(60.0, self._monitor_gas_prices)

        # ⇢ Start IBC transfer monitoring
        self.add_timer(10.0, self._monitor_ibc_transfers)

        # ⇢ Start oracle proof refresh
        self.add_timer(120.0, self._refresh_oracle_proofs)

        self.log.info(
            f"Cross-rollup market maker initialized for {len(self.config.target_chains)} chains",
        )

    def on_stop(self) -> None:
        """
        Clean up cross-chain positions and connections.
        """
        self.log.info("Stopping cross-rollup market maker strategy")

        # Close positions across all chains
        for chain in self.config.target_chains:
            task = asyncio.create_task(self._close_chain_positions(chain))
            self._tasks.append(task)

        # ⇢ Schedule pending IBC transfers cleanup
        task = asyncio.create_task(self._wait_for_pending_transfers())
        self._tasks.append(task)

    async def _initialize_chain_connection(self, chain: str) -> None:
        """
        Initialize connection to a specific rollup chain.

        dYdX v4's cross-rollup architecture uses Cosmos IBC for chain-to-chain
        communication and asset transfers.

        """
        try:
            # In a real implementation, this would establish gRPC connections
            # to the specific rollup chain's validator nodes

            connection_config = {
                "chain_id": chain,
                "grpc_endpoint": f"{chain}.dydx.trade:9090",
                "rest_endpoint": f"https://{chain}.dydx.trade",
                "ibc_channel": f"channel-{chain}",
            }

            self.chain_connections[chain] = connection_config

            # Initialize bridge queue for this chain
            self.bridge_queues[chain] = []

            self.log.info(f"Initialized connection to chain: {chain}")

        except Exception as e:
            self.log.error(f"Error initializing chain connection {chain}: {e}")

    async def _monitor_cross_chain_arbitrage(self) -> None:
        """
        Monitor for cross-chain arbitrage opportunities.

        This method compares prices across different rollup chains to identify
        profitable arbitrage opportunities.

        """
        try:
            # Get prices across all chains for each instrument
            arbitrage_opportunities = []

            for chain, instruments in self.config.instruments_per_chain.items():
                for instrument_id in instruments:
                    # Compare prices across chains
                    price_data = await self._get_cross_chain_prices(instrument_id)

                    if price_data:
                        arb_opportunity = self._analyze_arbitrage_opportunity(
                            instrument_id,
                            price_data,
                        )

                        if arb_opportunity:
                            arbitrage_opportunities.append(arb_opportunity)

            # Execute profitable arbitrage opportunities
            for opportunity in arbitrage_opportunities:
                await self._execute_cross_chain_arbitrage(opportunity)

        except Exception as e:
            self.log.error(f"Error monitoring cross-chain arbitrage: {e}")

    async def _get_cross_chain_prices(self, instrument_id: InstrumentId) -> dict | None:
        """
        Get prices for an instrument across all chains.

        This method queries order books across multiple rollup chains to identify price
        discrepancies.

        """
        try:
            price_data = {}

            for chain in self.config.target_chains:
                # Get order book for this chain
                # In a real implementation, this would query the specific chain's order book
                book = self.cache.order_book(instrument_id)

                if book and book.best_bid_price() and book.best_ask_price():
                    price_data[chain] = {
                        "bid": book.best_bid_price(),
                        "ask": book.best_ask_price(),
                        "mid": (book.best_bid_price() + book.best_ask_price()) / 2,
                        "spread": book.best_ask_price() - book.best_bid_price(),
                    }

            return price_data if len(price_data) > 1 else None

        except Exception as e:
            self.log.error(f"Error getting cross-chain prices for {instrument_id}: {e}")
            return None

    def _analyze_arbitrage_opportunity(
        self,
        instrument_id: InstrumentId,
        price_data: dict,
    ) -> dict | None:
        """
        Analyze cross-chain arbitrage opportunity.

        This method identifies profitable price discrepancies between chains and
        calculates potential arbitrage profits.

        """
        if len(price_data) < 2:
            return None

        # Find best bid and ask across chains
        best_bid_chain = None
        best_bid_price = Decimal(0)
        best_ask_chain = None
        best_ask_price = Decimal("999999")

        for chain, prices in price_data.items():
            if prices["bid"] > best_bid_price:
                best_bid_price = prices["bid"]
                best_bid_chain = chain

            if prices["ask"] < best_ask_price:
                best_ask_price = prices["ask"]
                best_ask_chain = chain

        # Calculate potential profit
        if best_bid_chain and best_ask_chain and best_bid_chain != best_ask_chain:
            profit = best_bid_price - best_ask_price
            profit_bps = (profit / best_ask_price) * 10000

            # Check if profit exceeds minimum threshold
            if profit_bps >= self.config.cross_chain_spread_bps:
                return {
                    "instrument_id": instrument_id,
                    "buy_chain": best_ask_chain,
                    "sell_chain": best_bid_chain,
                    "buy_price": best_ask_price,
                    "sell_price": best_bid_price,
                    "profit": profit,
                    "profit_bps": profit_bps,
                }

        return None

    async def _execute_cross_chain_arbitrage(self, opportunity: dict) -> None:
        """
        Execute cross-chain arbitrage opportunity.

        This method executes simultaneous trades across different rollup chains to
        capture arbitrage profits.

        """
        instrument_id = opportunity["instrument_id"]
        buy_chain = opportunity["buy_chain"]
        sell_chain = opportunity["sell_chain"]
        buy_price = opportunity["buy_price"]
        sell_price = opportunity["sell_price"]

        # Calculate position size based on available capital and risk limits
        position_size = self._calculate_arbitrage_size(opportunity)

        if position_size <= 0:
            return

        # Execute buy order on cheaper chain
        buy_order = self.order_factory.limit(
            instrument_id=instrument_id,
            order_side="BUY",
            quantity=position_size,
            price=buy_price,
            time_in_force="IOC",
            client_order_id=f"ARB_BUY_{buy_chain}_{int(asyncio.get_event_loop().time())}",
        )

        # Execute sell order on more expensive chain
        sell_order = self.order_factory.limit(
            instrument_id=instrument_id,
            order_side="SELL",
            quantity=position_size,
            price=sell_price,
            time_in_force="IOC",
            client_order_id=f"ARB_SELL_{sell_chain}_{int(asyncio.get_event_loop().time())}",
        )

        # Submit orders (in a real implementation, these would be sent to specific chains)
        self.submit_order(buy_order)
        self.submit_order(sell_order)

        self.log.info(
            f"Executed cross-chain arbitrage: {instrument_id} "
            f"Buy {buy_chain} @ {buy_price} / Sell {sell_chain} @ {sell_price} "
            f"Size: {position_size} Profit: {opportunity['profit']:.4f}",
        )

    def _calculate_arbitrage_size(self, opportunity: dict) -> Decimal:
        """
        Calculate optimal position size for cross-chain arbitrage.

        This method considers position limits, available capital, and chain-specific
        constraints.

        """
        instrument_id = opportunity["instrument_id"]
        buy_chain = opportunity["buy_chain"]
        sell_chain = opportunity["sell_chain"]

        # Get current positions on both chains
        buy_chain_position = self.cross_chain_positions.get(
            f"{buy_chain}_{instrument_id}",
            Decimal(0),
        )
        sell_chain_position = self.cross_chain_positions.get(
            f"{sell_chain}_{instrument_id}",
            Decimal(0),
        )

        # Calculate available capacity
        buy_capacity = self.config.max_position_per_chain - abs(buy_chain_position)
        sell_capacity = self.config.max_position_per_chain - abs(sell_chain_position)

        # Use smaller of the two capacities
        max_size = min(buy_capacity, sell_capacity)

        # Base size on profit potential
        profit_bps = opportunity["profit_bps"]
        base_size = self.config.max_position_per_chain * Decimal(str(profit_bps)) / 1000

        return min(max_size, base_size, Decimal("1000.0"))  # Cap at $1000

    async def _sync_cross_chain_positions(self) -> None:
        """
        Synchronize positions across all rollup chains.

        This method ensures position data is consistent across chains and identifies any
        position imbalances that need rebalancing.

        """
        try:
            # Get positions from all chains
            for chain in self.config.target_chains:
                chain_positions = await self._get_chain_positions(chain)

                for instrument_id, position in chain_positions.items():
                    key = f"{chain}_{instrument_id}"
                    self.cross_chain_positions[key] = position

            # Check for position imbalances
            await self._rebalance_cross_chain_positions()

        except Exception as e:
            self.log.error(f"Error syncing cross-chain positions: {e}")

    async def _get_chain_positions(self, chain: str) -> dict[InstrumentId, Decimal]:
        """
        Get current positions on a specific chain.
        """
        try:
            # In a real implementation, this would query the specific chain's position data
            # via the chain's gRPC or REST API

            positions = {}

            if chain in self.config.instruments_per_chain:
                for instrument_id in self.config.instruments_per_chain[chain]:
                    position = self.cache.position(instrument_id)
                    positions[instrument_id] = position.quantity if position else Decimal(0)

            return positions

        except Exception as e:
            self.log.error(f"Error getting positions for chain {chain}: {e}")
            return {}

    async def _rebalance_cross_chain_positions(self) -> None:
        """
        Rebalance positions across chains to maintain target allocation.

        This method uses IBC transfers to move assets between chains and maintain
        optimal position distribution.

        """
        # Calculate total position per instrument across all chains
        total_positions = {}

        for key, position in self.cross_chain_positions.items():
            _, instrument_id_str = key.split("_", 1)
            instrument_id = InstrumentId.from_str(instrument_id_str)

            if instrument_id not in total_positions:
                total_positions[instrument_id] = Decimal(0)
            total_positions[instrument_id] += position

        # Check if any rebalancing is needed
        for instrument_id, total_position in total_positions.items():
            if abs(total_position) > self.config.bridge_threshold:
                await self._initiate_cross_chain_transfer(instrument_id, total_position)

    async def _initiate_cross_chain_transfer(
        self,
        instrument_id: InstrumentId,
        amount: Decimal,
    ) -> None:
        """
        Initiate cross-chain asset transfer using IBC.

        This method uses dYdX v4's IBC integration to transfer assets between rollup
        chains for position rebalancing.

        """
        # Find source and destination chains
        source_chain = None
        dest_chain = None

        for chain in self.config.target_chains:
            chain_position = self.cross_chain_positions.get(f"{chain}_{instrument_id}", Decimal(0))

            if amount > 0 and chain_position > 0:
                source_chain = chain
            elif amount < 0 and chain_position < 0:
                dest_chain = chain

        if source_chain and dest_chain:
            transfer_amount = min(abs(amount), self.config.bridge_threshold)

            # Queue IBC transfer
            transfer_request = {
                "source_chain": source_chain,
                "dest_chain": dest_chain,
                "instrument_id": instrument_id,
                "amount": transfer_amount,
                "timestamp": asyncio.get_event_loop().time(),
            }

            self.bridge_queues[source_chain].append(transfer_request)

            self.log.info(
                f"Queued cross-chain transfer: {instrument_id} "
                f"{transfer_amount} from {source_chain} to {dest_chain}",
            )

    async def _monitor_gas_prices(self) -> None:
        """
        Monitor gas prices across all rollup chains.

        This method tracks gas prices to optimize transaction timing and chain selection
        for cost efficiency.

        """
        try:
            for chain in self.config.target_chains:
                gas_price = await self._get_chain_gas_price(chain)

                if gas_price:
                    self.gas_prices[chain] = gas_price

            # Log gas price summary
            if self.gas_prices:
                cheapest_chain = min(self.gas_prices, key=self.gas_prices.get)
                self.log.debug(
                    f"Gas prices: {dict(self.gas_prices)}, " f"Cheapest: {cheapest_chain}",
                )

        except Exception as e:
            self.log.error(f"Error monitoring gas prices: {e}")

    async def _get_chain_gas_price(self, chain: str) -> Decimal | None:
        """
        Get current gas price for a specific chain.
        """
        try:
            # In a real implementation, this would query the chain's gas price
            # via the chain's gRPC or REST API

            # Placeholder implementation
            base_gas_prices = {
                "arbitrum": Decimal("0.1"),
                "optimism": Decimal("0.05"),
                "polygon": Decimal("0.02"),
                "base": Decimal("0.03"),
            }

            return base_gas_prices.get(chain, Decimal("0.1"))

        except Exception as e:
            self.log.error(f"Error getting gas price for chain {chain}: {e}")
            return None

    async def _close_chain_positions(self, chain: str) -> None:
        """
        Close all positions on a specific chain.
        """
        if chain in self.config.instruments_per_chain:
            for instrument_id in self.config.instruments_per_chain[chain]:
                position = self.cache.position(instrument_id)
                if position and position.quantity != 0:
                    close_order = self.order_factory.market(
                        instrument_id=instrument_id,
                        order_side="SELL" if position.quantity > 0 else "BUY",
                        quantity=abs(position.quantity),
                        reduce_only=True,
                    )
                    self.submit_order(close_order)

    async def _wait_for_pending_transfers(self) -> None:
        """
        Wait for all pending IBC transfers to complete.
        """
        try:
            # Check all bridge queues for pending transfers
            pending_transfers = []
            for chain, queue in self.bridge_queues.items():
                for transfer in queue:
                    if transfer.get("status") == "pending":
                        pending_transfers.append(transfer)

            if pending_transfers:
                self.log.info(f"Waiting for {len(pending_transfers)} pending IBC transfers")

                # Wait up to 5 minutes for transfers to complete
                timeout = 300  # 5 minutes
                start_time = asyncio.get_event_loop().time()

                while (
                    pending_transfers and (asyncio.get_event_loop().time() - start_time) < timeout
                ):
                    await asyncio.sleep(5)
                    # Check transfer status (placeholder - would check actual IBC transfer status)
                    pending_transfers = [
                        t for t in pending_transfers if t.get("status") == "pending"
                    ]

                if pending_transfers:
                    self.log.warning(f"Timeout waiting for {len(pending_transfers)} IBC transfers")
                else:
                    self.log.info("All pending IBC transfers completed")

        except Exception as e:
            self.log.error(f"Error waiting for pending transfers: {e}")

    async def _refresh_oracle_proofs(self) -> None:
        """
        Refresh oracle price proofs for all chains.

        dYdX v4 uses oracle price proofs to ensure consistent pricing across all rollup
        chains. This method refreshes those proofs.

        """
        try:
            for chain in self.config.target_chains:
                # In a real implementation, this would fetch fresh oracle proofs
                # from the dYdX v4 oracle network

                proof_request = {
                    "chain": chain,
                    "timestamp": asyncio.get_event_loop().time(),
                    "instruments": self.config.instruments_per_chain.get(chain, []),
                }

                # Store proof for later use
                self.oracle_proofs[chain] = proof_request

            self.log.debug(f"Refreshed oracle proofs for {len(self.config.target_chains)} chains")

        except Exception as e:
            self.log.error(f"Error refreshing oracle proofs: {e}")

    async def _validate_cross_chain_opportunity(self, opportunity: dict) -> bool:
        """
        Validate cross-chain arbitrage opportunity.

        This method performs comprehensive validation including:
        - Oracle price consistency
        - Position limits
        - Gas cost analysis
        - IBC transfer capacity

        """
        try:
            instrument_id = opportunity["instrument_id"]
            buy_chain = opportunity["buy_chain"]
            sell_chain = opportunity["sell_chain"]

            # Check oracle price consistency
            if not await self._validate_oracle_prices(buy_chain, sell_chain, instrument_id):
                return False

            # Check position limits
            if not self._check_position_limits(buy_chain, sell_chain, instrument_id):
                return False

            # Check gas costs
            if not await self._validate_gas_costs(buy_chain, sell_chain, opportunity):
                return False

            # Check IBC transfer capacity
            if not self._check_ibc_capacity(buy_chain, sell_chain):
                return False

            return True

        except Exception as e:
            self.log.error(f"Error validating cross-chain opportunity: {e}")
            return False

    async def _validate_oracle_prices(
        self,
        buy_chain: str,
        sell_chain: str,
        instrument_id: InstrumentId,
    ) -> bool:
        """
        Validate oracle price consistency between chains.
        """
        try:
            # Get oracle proofs for both chains
            buy_proof = self.oracle_proofs.get(buy_chain)
            sell_proof = self.oracle_proofs.get(sell_chain)

            if not buy_proof or not sell_proof:
                self.log.warning(f"Missing oracle proofs for {buy_chain} or {sell_chain}")
                return False

            # Check proof freshness (should be less than 60 seconds old)
            current_time = asyncio.get_event_loop().time()
            if (current_time - buy_proof["timestamp"]) > 60 or (
                current_time - sell_proof["timestamp"]
            ) > 60:
                self.log.warning("Oracle proofs are stale, refreshing")
                await self._refresh_oracle_proofs()

            return True

        except Exception as e:
            self.log.error(f"Error validating oracle prices: {e}")
            return False

    def _check_position_limits(
        self,
        buy_chain: str,
        sell_chain: str,
        instrument_id: InstrumentId,
    ) -> bool:
        """
        Check if position limits allow for the arbitrage trade.
        """
        try:
            buy_position = self.cross_chain_positions.get(
                f"{buy_chain}_{instrument_id}",
                Decimal(0),
            )
            sell_position = self.cross_chain_positions.get(
                f"{sell_chain}_{instrument_id}",
                Decimal(0),
            )

            # Check if we're within position limits
            if abs(buy_position) >= self.config.max_position_per_chain:
                self.log.warning(f"Position limit reached for {buy_chain}")
                return False

            if abs(sell_position) >= self.config.max_position_per_chain:
                self.log.warning(f"Position limit reached for {sell_chain}")
                return False

            return True

        except Exception as e:
            self.log.error(f"Error checking position limits: {e}")
            return False

    async def _validate_gas_costs(self, buy_chain: str, sell_chain: str, opportunity: dict) -> bool:
        """
        Validate that gas costs don't exceed profit potential.
        """
        try:
            buy_gas = self.gas_prices.get(buy_chain, Decimal("0.1"))
            sell_gas = self.gas_prices.get(sell_chain, Decimal("0.1"))

            total_gas_cost = buy_gas + sell_gas
            expected_profit = opportunity["profit"]

            # Gas costs should be less than 20% of expected profit
            if total_gas_cost > (expected_profit * Decimal("0.2")):
                self.log.warning(
                    f"Gas costs too high: {total_gas_cost} vs profit {expected_profit}",
                )
                return False

            return True

        except Exception as e:
            self.log.error(f"Error validating gas costs: {e}")
            return False

    def _check_ibc_capacity(self, buy_chain: str, sell_chain: str) -> bool:
        """
        Check if IBC transfer capacity is available.
        """
        try:
            # Check pending transfers in bridge queues
            buy_queue_size = len(self.bridge_queues.get(buy_chain, []))
            sell_queue_size = len(self.bridge_queues.get(sell_chain, []))

            # Don't allow arbitrage if too many pending transfers
            if buy_queue_size > 5 or sell_queue_size > 5:
                self.log.warning(
                    f"Too many pending IBC transfers: {buy_chain}={buy_queue_size}, {sell_chain}={sell_queue_size}",
                )
                return False

            return True

        except Exception as e:
            self.log.error(f"Error checking IBC capacity: {e}")
            return False

    def _calculate_position_risk(self) -> dict[str, float]:
        """
        Calculate position risk metrics across all chains.

        This method provides comprehensive risk metrics for cross-chain position
        management.

        """
        try:
            risk_metrics = {}

            # Calculate per-chain risk
            for chain in self.config.target_chains:
                chain_positions = {
                    k: v for k, v in self.cross_chain_positions.items() if k.startswith(f"{chain}_")
                }

                total_exposure = sum(abs(pos) for pos in chain_positions.values())
                risk_metrics[f"{chain}_exposure"] = float(total_exposure)
                risk_metrics[f"{chain}_utilization"] = float(
                    total_exposure / self.config.max_position_per_chain,
                )

            # Calculate cross-chain concentration risk
            instrument_concentrations = {}
            for key, position in self.cross_chain_positions.items():
                _, instrument_str = key.split("_", 1)
                if instrument_str not in instrument_concentrations:
                    instrument_concentrations[instrument_str] = Decimal(0)
                instrument_concentrations[instrument_str] += abs(position)

            max_concentration = (
                max(instrument_concentrations.values()) if instrument_concentrations else Decimal(0)
            )
            risk_metrics["max_instrument_concentration"] = float(max_concentration)

            return risk_metrics

        except Exception as e:
            self.log.error(f"Error calculating position risk: {e}")
            return {}

    async def _emergency_position_close(self) -> None:
        """
        Emergency position closure across all chains.

        This method is called when circuit breakers are triggered or when emergency
        shutdown is required.

        """
        try:
            self.log.warning("EMERGENCY: Initiating cross-chain position closure")

            # Close all positions across all chains
            for chain in self.config.target_chains:
                await self._close_chain_positions(chain)

            # Cancel all pending orders
            for order in self.cache.orders_open():
                cancel_order = self.order_factory.cancel(order.client_order_id)
                self.submit_order(cancel_order)

            # Clear bridge queues
            for chain in self.config.target_chains:
                self.bridge_queues[chain].clear()

            self.log.warning("Emergency position closure completed")

        except Exception as e:
            self.log.error(f"Error during emergency position closure: {e}")

    # Add this method before the __main__ section
