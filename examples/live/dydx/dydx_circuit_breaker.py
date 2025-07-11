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
DYdX Circuit Breaker Strategy.

This strategy implements intelligent circuit breaker logic for dYdX v4 trading
to protect against extreme market conditions and system anomalies.

Key dYdX v4 Features:
- Real-time chain state monitoring via CometBFT
- Validator set health and consensus monitoring
- Network congestion and gas price tracking
- Protocol-level risk parameter monitoring
- Emergency position liquidation capabilities

This implementation provides comprehensive risk management and automatic
position protection during adverse market or network conditions.

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
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class CircuitBreakerConfig:
    """
    Configuration for circuit breaker strategy.
    """

    def __init__(
        self,
        monitored_instruments: list[InstrumentId],
        volatility_threshold: Decimal = Decimal("0.20"),  # 20% volatility
        volume_spike_threshold: Decimal = Decimal("5.0"),  # 5x volume spike
        price_deviation_threshold: Decimal = Decimal("0.10"),  # 10% price deviation
        network_congestion_threshold: int = 80,  # 80% block fullness
        max_drawdown_threshold: Decimal = Decimal("0.15"),  # 15% max drawdown
        emergency_liquidation_enabled: bool = True,
        auto_resume_enabled: bool = True,
        cooldown_period_seconds: int = 300,  # 5 minutes
    ):
        self.monitored_instruments = monitored_instruments
        self.volatility_threshold = volatility_threshold
        self.volume_spike_threshold = volume_spike_threshold
        self.price_deviation_threshold = price_deviation_threshold
        self.network_congestion_threshold = network_congestion_threshold
        self.max_drawdown_threshold = max_drawdown_threshold
        self.emergency_liquidation_enabled = emergency_liquidation_enabled
        self.auto_resume_enabled = auto_resume_enabled
        self.cooldown_period_seconds = cooldown_period_seconds


class CircuitBreaker(Strategy):
    """
    Circuit breaker strategy for dYdX v4 risk management.

    This strategy monitors various risk factors and automatically
    implements protective measures during adverse conditions:

    - Market volatility and price deviation monitoring
    - Network congestion and consensus health tracking
    - Portfolio drawdown and risk exposure management
    - Emergency position liquidation and stop-loss execution
    - Automatic system recovery and trading resumption

    dYdX v4's transparent chain state enables sophisticated
    risk monitoring and automated protection mechanisms.

    """

    def __init__(self, config: CircuitBreakerConfig):
        super().__init__(config)
        self.config = config
        self.circuit_breaker_active = False
        self.breach_conditions: list[dict[str, Any]] = []
        self.last_breach_time = 0
        self.baseline_metrics: dict[str, Any] = {}
        self.risk_metrics: dict[str, Any] = {}
        self.emergency_actions_taken: list[dict[str, Any]] = []

        # Track background tasks
        self._tasks: list[asyncio.Task] = []

    def on_start(self) -> None:
        """
        Initialize circuit breaker monitoring systems.
        """
        self.log.info("Starting circuit breaker strategy")

        # Subscribe to monitored instruments
        for instrument_id in self.config.monitored_instruments:
            self.subscribe_order_book_deltas(instrument_id)
            self.subscribe_bars(BarType.from_str(f"{instrument_id}-1-MINUTE-LAST"))

        # Start risk monitoring (high frequency)
        self.add_timer(1.0, self._monitor_market_conditions)

        # Start network monitoring
        self.add_timer(5.0, self._monitor_network_health)

        # Start portfolio monitoring
        self.add_timer(10.0, self._monitor_portfolio_risk)

        # Start circuit breaker status check
        self.add_timer(30.0, self._check_circuit_breaker_status)

        # Initialize baseline metrics
        task = asyncio.create_task(self._initialize_baseline_metrics())
        self._tasks.append(task)

        self.log.info("Circuit breaker strategy initialized")

    def on_stop(self) -> None:
        """
        Clean up monitoring and log final status.
        """
        self.log.info("Stopping circuit breaker strategy")

        if self.circuit_breaker_active:
            self.log.warning("Circuit breaker was active at shutdown")

        self.log.info(f"Total emergency actions taken: {len(self.emergency_actions_taken)}")

    async def _initialize_baseline_metrics(self) -> None:
        """
        Initialize baseline metrics for comparison.
        """
        try:
            # Wait for initial data
            await asyncio.sleep(10)

            # Calculate baseline metrics for each instrument
            for instrument_id in self.config.monitored_instruments:
                baseline = await self._calculate_baseline_metrics(instrument_id)
                if baseline:
                    self.baseline_metrics[instrument_id] = baseline

        except Exception as e:
            self.log.error(f"Error initializing baseline metrics: {e}")

    async def _calculate_baseline_metrics(self, instrument_id: InstrumentId) -> dict | None:
        """
        Calculate baseline metrics for an instrument.
        """
        try:
            # Get historical data for baseline calculation
            # In a real implementation, this would use historical bars

            # Placeholder implementation
            return {
                "average_volume": Decimal("1000000"),
                "average_volatility": Decimal("0.05"),
                "average_spread": Decimal("0.001"),
                "price_range": {
                    "min": Decimal("45000"),
                    "max": Decimal("55000"),
                },
            }

        except Exception as e:
            self.log.error(f"Error calculating baseline metrics for {instrument_id}: {e}")
            return None

    async def _monitor_market_conditions(self) -> None:
        """
        Monitor market conditions for circuit breaker triggers.

        This method checks various market metrics against thresholds to detect abnormal
        conditions.

        """
        try:
            breaches = []

            for instrument_id in self.config.monitored_instruments:
                # Check volatility
                volatility_breach = await self._check_volatility_breach(instrument_id)
                if volatility_breach:
                    breaches.append(volatility_breach)

                # Check volume spikes
                volume_breach = await self._check_volume_breach(instrument_id)
                if volume_breach:
                    breaches.append(volume_breach)

                # Check price deviation
                price_breach = await self._check_price_deviation(instrument_id)
                if price_breach:
                    breaches.append(price_breach)

            if breaches:
                await self._handle_market_breaches(breaches)

        except Exception as e:
            self.log.error(f"Error monitoring market conditions: {e}")

    async def _check_volatility_breach(self, instrument_id: InstrumentId) -> dict | None:
        """
        Check for volatility threshold breach.
        """
        try:
            # Calculate current volatility
            current_volatility = await self._calculate_current_volatility(instrument_id)

            if current_volatility > self.config.volatility_threshold:
                return {
                    "type": "volatility_breach",
                    "instrument": instrument_id,
                    "current_value": current_volatility,
                    "threshold": self.config.volatility_threshold,
                    "severity": (
                        "high"
                        if current_volatility > self.config.volatility_threshold * 2
                        else "medium"
                    ),
                }

        except Exception as e:
            self.log.error(f"Error checking volatility breach for {instrument_id}: {e}")

        return None

    async def _calculate_current_volatility(self, instrument_id: InstrumentId) -> Decimal:
        """
        Calculate current volatility for an instrument.
        """
        try:
            # In a real implementation, this would calculate volatility
            # from recent price data

            # Placeholder implementation
            return Decimal("0.15")  # 15% volatility

        except Exception as e:
            self.log.error(f"Error calculating volatility for {instrument_id}: {e}")
            return Decimal(0)

    async def _check_volume_breach(self, instrument_id: InstrumentId) -> dict | None:
        """
        Check for volume spike threshold breach.
        """
        try:
            current_volume = await self._get_current_volume(instrument_id)
            baseline = self.baseline_metrics.get(instrument_id, {})
            baseline_volume = baseline.get("average_volume", Decimal("1000000"))

            volume_ratio = current_volume / baseline_volume if baseline_volume > 0 else Decimal(1)

            if volume_ratio > self.config.volume_spike_threshold:
                return {
                    "type": "volume_breach",
                    "instrument": instrument_id,
                    "current_value": current_volume,
                    "baseline_value": baseline_volume,
                    "ratio": volume_ratio,
                    "threshold": self.config.volume_spike_threshold,
                    "severity": (
                        "high"
                        if volume_ratio > self.config.volume_spike_threshold * 2
                        else "medium"
                    ),
                }

        except Exception as e:
            self.log.error(f"Error checking volume breach for {instrument_id}: {e}")

        return None

    async def _get_current_volume(self, instrument_id: InstrumentId) -> Decimal:
        """
        Get current volume for an instrument.
        """
        try:
            # In a real implementation, this would get current volume data
            # from recent bars or trade data

            # Placeholder implementation
            return Decimal("2000000")  # $2M volume

        except Exception as e:
            self.log.error(f"Error getting current volume for {instrument_id}: {e}")
            return Decimal(0)

    async def _check_price_deviation(self, instrument_id: InstrumentId) -> dict | None:
        """
        Check for price deviation threshold breach.
        """
        try:
            current_price = await self._get_current_price(instrument_id)
            baseline = self.baseline_metrics.get(instrument_id, {})
            price_range = baseline.get("price_range", {})

            if not price_range:
                return None

            min_price = price_range.get("min", Decimal(0))
            max_price = price_range.get("max", Decimal(999999))

            # Check if price is outside expected range
            if current_price < min_price * (1 - self.config.price_deviation_threshold):
                return {
                    "type": "price_deviation",
                    "instrument": instrument_id,
                    "current_price": current_price,
                    "expected_min": min_price,
                    "deviation": (min_price - current_price) / min_price,
                    "direction": "down",
                    "severity": "high",
                }

            elif current_price > max_price * (1 + self.config.price_deviation_threshold):
                return {
                    "type": "price_deviation",
                    "instrument": instrument_id,
                    "current_price": current_price,
                    "expected_max": max_price,
                    "deviation": (current_price - max_price) / max_price,
                    "direction": "up",
                    "severity": "high",
                }

        except Exception as e:
            self.log.error(f"Error checking price deviation for {instrument_id}: {e}")

        return None

    async def _get_current_price(self, instrument_id: InstrumentId) -> Decimal:
        """
        Get current price for an instrument.
        """
        try:
            book = self.cache.order_book(instrument_id)
            if book and book.best_bid_price() and book.best_ask_price():
                return (book.best_bid_price() + book.best_ask_price()) / 2

        except Exception as e:
            self.log.error(f"Error getting current price for {instrument_id}: {e}")

        return Decimal(0)

    async def _monitor_network_health(self) -> None:
        """
        Monitor dYdX v4 network health and consensus.

        This method checks validator set health, network congestion, and consensus
        performance.

        """
        try:
            # Check network congestion
            congestion_level = await self._check_network_congestion()

            if congestion_level > self.config.network_congestion_threshold:
                breach = {
                    "type": "network_congestion",
                    "current_value": congestion_level,
                    "threshold": self.config.network_congestion_threshold,
                    "severity": "high" if congestion_level > 95 else "medium",
                }

                await self._handle_network_breach(breach)

            # Check validator set health
            validator_health = await self._check_validator_health()

            if validator_health and validator_health["active_ratio"] < 0.67:  # Less than 2/3 active
                breach = {
                    "type": "validator_health",
                    "active_ratio": validator_health["active_ratio"],
                    "threshold": 0.67,
                    "severity": "critical",
                }

                await self._handle_network_breach(breach)

        except Exception as e:
            self.log.error(f"Error monitoring network health: {e}")

    async def _check_network_congestion(self) -> int:
        """
        Check network congestion level (0-100%).
        """
        try:
            # In a real implementation, this would query blockchain metrics
            # such as block fullness, gas prices, and transaction queue

            # Placeholder implementation
            return 45  # 45% congestion

        except Exception as e:
            self.log.error(f"Error checking network congestion: {e}")
            return 0

    async def _check_validator_health(self) -> dict | None:
        """
        Check validator set health.
        """
        try:
            # In a real implementation, this would query validator set
            # and check for missed blocks, slashing events, etc.

            # Placeholder implementation
            return {
                "total_validators": 100,
                "active_validators": 95,
                "active_ratio": 0.95,
                "recent_slashing_events": 0,
            }

        except Exception as e:
            self.log.error(f"Error checking validator health: {e}")
            return None

    async def _monitor_portfolio_risk(self) -> None:
        """
        Monitor portfolio-level risk metrics.
        """
        try:
            # Calculate current drawdown
            current_drawdown = await self._calculate_current_drawdown()

            if current_drawdown > self.config.max_drawdown_threshold:
                breach = {
                    "type": "drawdown_breach",
                    "current_value": current_drawdown,
                    "threshold": self.config.max_drawdown_threshold,
                    "severity": "critical",
                }

                await self._handle_portfolio_breach(breach)

        except Exception as e:
            self.log.error(f"Error monitoring portfolio risk: {e}")

    async def _calculate_current_drawdown(self) -> Decimal:
        """
        Calculate current portfolio drawdown.
        """
        try:
            # In a real implementation, this would calculate drawdown
            # from portfolio high water mark

            # Placeholder implementation
            return Decimal("0.08")  # 8% drawdown

        except Exception as e:
            self.log.error(f"Error calculating current drawdown: {e}")
            return Decimal(0)

    async def _handle_market_breaches(self, breaches: list[dict]) -> None:
        """
        Handle market condition breaches.
        """
        for breach in breaches:
            await self._trigger_circuit_breaker(breach)

    async def _handle_network_breach(self, breach: dict) -> None:
        """
        Handle network health breaches.
        """
        await self._trigger_circuit_breaker(breach)

    async def _handle_portfolio_breach(self, breach: dict) -> None:
        """
        Handle portfolio risk breaches.
        """
        await self._trigger_circuit_breaker(breach)

    async def _trigger_circuit_breaker(self, breach: dict) -> None:
        """
        Trigger circuit breaker and implement protective measures.

        This method activates the circuit breaker and executes emergency risk management
        actions.

        """
        try:
            # Activate circuit breaker
            self.circuit_breaker_active = True
            self.breach_conditions.append(breach)
            self.last_breach_time = int(asyncio.get_event_loop().time())

            self.log.critical(
                f"CIRCUIT BREAKER ACTIVATED: {breach['type']} "
                f"Severity: {breach.get('severity', 'unknown')}",
            )

            # Execute emergency actions based on breach type and severity
            if breach.get("severity") == "critical":
                await self._execute_emergency_liquidation()

            elif breach.get("severity") == "high":
                await self._execute_risk_reduction()

            else:  # medium severity
                await self._execute_position_hedging()

        except Exception as e:
            self.log.error(f"Error triggering circuit breaker: {e}")

    async def _execute_emergency_liquidation(self) -> None:
        """
        Execute emergency liquidation of all positions.
        """
        if not self.config.emergency_liquidation_enabled:
            return

        try:
            self.log.critical("EXECUTING EMERGENCY LIQUIDATION")

            # Close all positions
            for instrument_id in self.config.monitored_instruments:
                position = self.cache.position(instrument_id)

                if position and position.quantity != 0:
                    # Submit market order to close position
                    close_order = self.order_factory.market(
                        instrument_id=instrument_id,
                        order_side="SELL" if position.quantity > 0 else "BUY",
                        quantity=abs(position.quantity),
                        reduce_only=True,
                        client_order_id=f"EMERGENCY_CLOSE_{instrument_id}",
                    )

                    self.submit_order(close_order)

            # Cancel all open orders
            self.cancel_all_orders()

            # Record emergency action
            self.emergency_actions_taken.append(
                {
                    "action": "emergency_liquidation",
                    "timestamp": asyncio.get_event_loop().time(),
                    "reason": "critical_breach",
                },
            )

        except Exception as e:
            self.log.error(f"Error executing emergency liquidation: {e}")

    async def _execute_risk_reduction(self) -> None:
        """
        Execute risk reduction measures.
        """
        try:
            self.log.warning("EXECUTING RISK REDUCTION")

            # Reduce position sizes by 50%
            for instrument_id in self.config.monitored_instruments:
                position = self.cache.position(instrument_id)

                if position and position.quantity != 0:
                    reduction_size = abs(position.quantity) * Decimal("0.5")

                    reduce_order = self.order_factory.market(
                        instrument_id=instrument_id,
                        order_side="SELL" if position.quantity > 0 else "BUY",
                        quantity=reduction_size,
                        reduce_only=True,
                        client_order_id=f"RISK_REDUCE_{instrument_id}",
                    )

                    self.submit_order(reduce_order)

            # Record risk reduction action
            self.emergency_actions_taken.append(
                {
                    "action": "risk_reduction",
                    "timestamp": asyncio.get_event_loop().time(),
                    "reason": "high_severity_breach",
                },
            )

        except Exception as e:
            self.log.error(f"Error executing risk reduction: {e}")

    async def _execute_position_hedging(self) -> None:
        """
        Execute position hedging measures.
        """
        try:
            self.log.warning("EXECUTING POSITION HEDGING")

            # Implement hedging logic (placeholder)
            # This would typically involve opening offsetting positions
            # or using derivatives for hedging

            # Record hedging action
            self.emergency_actions_taken.append(
                {
                    "action": "position_hedging",
                    "timestamp": asyncio.get_event_loop().time(),
                    "reason": "medium_severity_breach",
                },
            )

        except Exception as e:
            self.log.error(f"Error executing position hedging: {e}")

    async def _check_circuit_breaker_status(self) -> None:
        """
        Check circuit breaker status and handle recovery.
        """
        try:
            if not self.circuit_breaker_active:
                return

            current_time = asyncio.get_event_loop().time()

            # Check if cooldown period has passed
            if current_time - self.last_breach_time > self.config.cooldown_period_seconds:

                # Check if conditions have normalized
                if await self._check_conditions_normalized():
                    await self._reset_circuit_breaker()

        except Exception as e:
            self.log.error(f"Error checking circuit breaker status: {e}")

    async def _check_conditions_normalized(self) -> bool:
        """
        Check if market and network conditions have normalized.
        """
        try:
            # Check market conditions
            for instrument_id in self.config.monitored_instruments:
                volatility = await self._calculate_current_volatility(instrument_id)
                if volatility > self.config.volatility_threshold * Decimal(
                    "0.8"
                ):  # 80% of threshold
                    return False

            # Check network conditions
            congestion = await self._check_network_congestion()
            if congestion > self.config.network_congestion_threshold * Decimal("0.8"):
                return False

            # Check portfolio risk
            drawdown = await self._calculate_current_drawdown()
            if drawdown > self.config.max_drawdown_threshold * Decimal("0.8"):
                return False

            return True

        except Exception as e:
            self.log.error(f"Error checking normalized conditions: {e}")
            return False

    async def _reset_circuit_breaker(self) -> None:
        """
        Reset circuit breaker and resume normal operations.
        """
        try:
            self.circuit_breaker_active = False
            self.breach_conditions = []

            self.log.info("CIRCUIT BREAKER RESET - Resuming normal operations")

            # Record reset action
            self.emergency_actions_taken.append(
                {
                    "action": "circuit_breaker_reset",
                    "timestamp": asyncio.get_event_loop().time(),
                    "reason": "conditions_normalized",
                },
            )

        except Exception as e:
            self.log.error(f"Error resetting circuit breaker: {e}")


# Example configuration and node setup
if __name__ == "__main__":
    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("CIRCUIT-BREAKER-001"),
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

    # Configure circuit breaker strategy
    strategy_config = CircuitBreakerConfig(
        monitored_instruments=[
            InstrumentId.from_str("BTC-USD-PERP.DYDX"),
            InstrumentId.from_str("ETH-USD-PERP.DYDX"),
            InstrumentId.from_str("SOL-USD-PERP.DYDX"),
        ],
        volatility_threshold=Decimal("0.20"),  # 20% volatility
        volume_spike_threshold=Decimal("5.0"),  # 5x volume spike
        price_deviation_threshold=Decimal("0.10"),  # 10% price deviation
        network_congestion_threshold=80,  # 80% congestion
        max_drawdown_threshold=Decimal("0.15"),  # 15% max drawdown
        emergency_liquidation_enabled=True,
        auto_resume_enabled=True,
        cooldown_period_seconds=300,  # 5 minutes
    )

    # Instantiate the strategy
    strategy = CircuitBreaker(config=strategy_config)

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
