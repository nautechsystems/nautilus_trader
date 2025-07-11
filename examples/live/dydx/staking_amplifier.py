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
DYdX Staking Amplifier Strategy.

This strategy leverages dYdX v4's unique staking mechanism and validator economics
to maximize staking rewards while maintaining active trading positions.

Key dYdX v4 Features:
- Native DYDX token staking with validator delegation
- Liquid staking derivatives for capital efficiency
- Validator commission optimization and rewards
- Staking-based governance voting power
- Slashing protection and risk management

This implementation optimizes staking rewards while using liquid staking
derivatives to maintain trading capital availability.

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


class StakingAmplifierConfig:
    """
    Configuration for staking amplifier strategy.
    """

    def __init__(
        self,
        staking_tokens: list[InstrumentId],
        target_staking_ratio: Decimal = Decimal("0.70"),  # 70% of DYDX staked
        min_staking_apy: Decimal = Decimal("0.08"),  # 8% minimum APY
        validator_preferences: list[str] | None = None,
        liquid_staking_enabled: bool = True,
        auto_compound_frequency: int = 24,  # Hours
        slashing_protection_buffer: Decimal = Decimal("0.05"),  # 5% buffer
    ):
        self.staking_tokens = staking_tokens
        self.target_staking_ratio = target_staking_ratio
        self.min_staking_apy = min_staking_apy
        self.validator_preferences = validator_preferences or []
        self.liquid_staking_enabled = liquid_staking_enabled
        self.auto_compound_frequency = auto_compound_frequency
        self.slashing_protection_buffer = slashing_protection_buffer


class StakingAmplifier(Strategy):
    """
    Staking amplifier strategy for dYdX v4 staking optimization.

    This strategy maximizes staking rewards while maintaining capital
    efficiency through liquid staking derivatives and validator optimization.

    Key Features:
    - Automated staking ratio management
    - Validator performance monitoring and delegation optimization
    - Liquid staking derivative utilization for capital efficiency
    - Automatic reward compounding and reinvestment
    - Slashing risk monitoring and protection
    - Governance participation optimization

    dYdX v4's staking mechanism provides multiple revenue streams
    including block rewards, transaction fees, and governance incentives.

    """

    def __init__(self, config: StakingAmplifierConfig):
        super().__init__(config)
        self.config = config
        self.staking_positions: dict[str, dict[str, Any]] = {}
        self.validator_performance: dict[str, dict[str, Any]] = {}
        self.liquid_staking_positions: dict[str, dict[str, Any]] = {}
        self.pending_rewards = Decimal(0)
        self.last_compound_time = 0

        # Track background tasks
        self._tasks: list[asyncio.Task] = []

    def on_start(self) -> None:
        """
        Initialize staking monitoring and optimization.
        """
        self.log.info("Starting staking amplifier strategy")

        # Subscribe to staking token price data
        for token_id in self.config.staking_tokens:
            self.subscribe_order_book_deltas(token_id)

        # Start staking ratio monitoring
        self.add_timer(300.0, self._monitor_staking_ratio)

        # Start validator performance monitoring
        self.add_timer(600.0, self._monitor_validator_performance)

        # Start reward compounding
        compound_interval = self.config.auto_compound_frequency * 3600
        self.add_timer(compound_interval, self._compound_staking_rewards)

        # Start slashing risk monitoring
        self.add_timer(60.0, self._monitor_slashing_risks)

        self.log.info("Staking amplifier strategy initialized")

    def on_stop(self) -> None:
        """
        Clean up staking positions and claim final rewards.
        """
        self.log.info("Stopping staking amplifier strategy")

        # Claim any pending rewards
        task = asyncio.create_task(self._claim_all_rewards())
        self._tasks.append(task)

    async def _monitor_staking_ratio(self) -> None:
        """
        Monitor and maintain target staking ratio.

        This method ensures the optimal balance between staked tokens and liquid tokens
        for trading capital.

        """
        try:
            # Get current token holdings
            total_holdings = await self._get_total_token_holdings()

            if total_holdings == 0:
                return

            # Calculate current staking ratio
            staked_amount = await self._get_total_staked_amount()
            current_ratio = staked_amount / total_holdings

            # Check if adjustment is needed
            ratio_diff = abs(current_ratio - self.config.target_staking_ratio)

            if ratio_diff > Decimal("0.05"):  # 5% tolerance
                await self._adjust_staking_ratio(current_ratio, total_holdings)

            self.log.debug(
                f"Staking ratio: {current_ratio:.2%} "
                f"Target: {self.config.target_staking_ratio:.2%} "
                f"Total holdings: {total_holdings:.2f}",
            )

        except Exception as e:
            self.log.error(f"Error monitoring staking ratio: {e}")

    async def _get_total_token_holdings(self) -> Decimal:
        """
        Get total token holdings across all staking tokens.
        """
        total = Decimal(0)

        for token_id in self.config.staking_tokens:
            position = self.cache.position(token_id)
            if position:
                total += position.quantity

        return total

    async def _get_total_staked_amount(self) -> Decimal:
        """
        Get total amount currently staked across all validators.
        """
        try:
            # Query staking module for delegated amounts
            staked_amount = await self._query_staking_delegations()

            # Add liquid staking positions
            if self.config.liquid_staking_enabled:
                liquid_staked = await self._get_liquid_staking_amount()
                staked_amount += liquid_staked

            return staked_amount

        except Exception as e:
            self.log.error(f"Error getting staked amount: {e}")
            return Decimal(0)

    async def _query_staking_delegations(self) -> Decimal:
        """
        Query current staking delegations from the chain.
        """
        try:
            # In a real implementation, this would query the staking module
            # Example: /cosmos/staking/v1beta1/delegations/{delegator_addr}

            # Placeholder implementation
            return Decimal("5000.0")  # $5000 staked

        except Exception as e:
            self.log.error(f"Error querying staking delegations: {e}")
            return Decimal(0)

    async def _get_liquid_staking_amount(self) -> Decimal:
        """
        Get amount in liquid staking derivatives.
        """
        total = Decimal(0)

        for token_id, amount in self.liquid_staking_positions.items():
            total += amount

        return total

    async def _adjust_staking_ratio(self, current_ratio: Decimal, total_holdings: Decimal) -> None:
        """
        Adjust staking ratio to match target.

        This method stakes or unstakes tokens to maintain the target ratio.

        """
        target_staked = total_holdings * self.config.target_staking_ratio
        current_staked = await self._get_total_staked_amount()

        adjustment_needed = target_staked - current_staked

        if adjustment_needed > Decimal("100.0"):  # Need to stake more
            await self._stake_tokens(adjustment_needed)

        elif adjustment_needed < Decimal("-100.0"):  # Need to unstake
            await self._unstake_tokens(abs(adjustment_needed))

    async def _stake_tokens(self, amount: Decimal) -> None:
        """
        Stake tokens with optimal validator selection.

        This method selects the best validators based on performance and commission
        rates.

        """
        try:
            # Get best validators for staking
            best_validators = await self._select_optimal_validators()

            if not best_validators:
                self.log.warning("No validators available for staking")
                return

            # Distribute staking across multiple validators for safety
            remaining_amount = amount

            for validator in best_validators:
                if remaining_amount <= 0:
                    break

                # Calculate stake amount for this validator
                stake_amount = min(remaining_amount, amount / len(best_validators))

                # Submit staking transaction
                await self._delegate_to_validator(validator, stake_amount)

                remaining_amount -= stake_amount

            self.log.info(f"Staked {amount} tokens across {len(best_validators)} validators")

        except Exception as e:
            self.log.error(f"Error staking tokens: {e}")

    async def _select_optimal_validators(self) -> list[dict]:
        """
        Select optimal validators based on performance metrics.

        This method analyzes validator performance, commission rates, and slashing
        history to select the best validators.

        """
        try:
            # Get all active validators
            validators = await self._get_active_validators()

            if not validators:
                return []

            # Score validators based on multiple criteria
            scored_validators = []

            for validator in validators:
                score = self._calculate_validator_score(validator)
                scored_validators.append((validator, score))

            # Sort by score (highest first)
            scored_validators.sort(key=lambda x: x[1], reverse=True)

            # Return top validators (up to 5 for diversification)
            return [validator for validator, score in scored_validators[:5]]

        except Exception as e:
            self.log.error(f"Error selecting validators: {e}")
            return []

    async def _get_active_validators(self) -> list[dict]:
        """
        Get list of active validators.
        """
        try:
            # In a real implementation, this would query the staking module
            # Example: /cosmos/staking/v1beta1/validators

            # Placeholder implementation
            return [
                {
                    "operator_address": "validator_1",
                    "moniker": "Top Validator",
                    "commission": Decimal("0.05"),  # 5% commission
                    "uptime": Decimal("0.99"),  # 99% uptime
                    "total_stake": Decimal("1000000"),
                    "slashing_events": 0,
                },
                {
                    "operator_address": "validator_2",
                    "moniker": "Reliable Validator",
                    "commission": Decimal("0.03"),  # 3% commission
                    "uptime": Decimal("0.98"),  # 98% uptime
                    "total_stake": Decimal("800000"),
                    "slashing_events": 0,
                },
            ]

        except Exception as e:
            self.log.error(f"Error getting active validators: {e}")
            return []

    def _calculate_validator_score(self, validator: dict) -> float:
        """
        Calculate validator score based on performance metrics.
        """
        # Scoring criteria weights
        uptime_weight = 0.4
        commission_weight = 0.3
        stake_weight = 0.2
        slashing_weight = 0.1

        # Normalize metrics
        uptime_score = float(validator.get("uptime", 0))
        commission_score = 1 - float(
            validator.get("commission", 0.1),
        )  # Lower commission = higher score
        stake_score = min(float(validator.get("total_stake", 0)) / 1000000, 1.0)  # Normalize by 1M
        slashing_score = 1.0 if validator.get("slashing_events", 0) == 0 else 0.5

        # Calculate weighted score
        total_score = (
            uptime_score * uptime_weight
            + commission_score * commission_weight
            + stake_score * stake_weight
            + slashing_score * slashing_weight
        )

        return total_score

    async def _delegate_to_validator(self, validator: dict, amount: Decimal) -> None:
        """
        Delegate tokens to a specific validator.
        """
        try:
            # In a real implementation, this would create a delegation transaction
            # and submit it to the blockchain

            validator_address = validator["operator_address"]

            # Track staking position
            if validator_address not in self.staking_positions:
                self.staking_positions[validator_address] = Decimal(0)

            self.staking_positions[validator_address] += amount

            self.log.info(
                f"Delegated {amount} to {validator.get('moniker', validator_address)}",
            )

        except Exception as e:
            self.log.error(f"Error delegating to validator: {e}")

    async def _unstake_tokens(self, amount: Decimal) -> None:
        """
        Unstake tokens from validators.
        """
        try:
            # Unstake from validators with lowest performance first
            validators_to_unstake = await self._select_validators_for_unstaking()

            remaining_amount = amount

            for validator_addr, staked_amount in validators_to_unstake:
                if remaining_amount <= 0:
                    break

                unstake_amount = min(remaining_amount, staked_amount)

                # Submit unstaking transaction
                await self._undelegate_from_validator(validator_addr, unstake_amount)

                remaining_amount -= unstake_amount

            self.log.info(f"Unstaked {amount} tokens")

        except Exception as e:
            self.log.error(f"Error unstaking tokens: {e}")

    async def _select_validators_for_unstaking(self) -> list[tuple[str, Decimal]]:
        """
        Select validators to unstake from (lowest performance first).
        """
        # Get current delegations
        validators_with_stake = []

        for validator_addr, staked_amount in self.staking_positions.items():
            if staked_amount > 0:
                validators_with_stake.append((validator_addr, staked_amount))

        # Sort by staked amount (unstake from smaller delegations first)
        validators_with_stake.sort(key=lambda x: x[1])

        return validators_with_stake

    async def _undelegate_from_validator(self, validator_addr: str, amount: Decimal) -> None:
        """
        Undelegate tokens from a specific validator.
        """
        try:
            # In a real implementation, this would create an undelegation transaction

            # Update tracking
            if validator_addr in self.staking_positions:
                self.staking_positions[validator_addr] -= amount

                if self.staking_positions[validator_addr] <= 0:
                    del self.staking_positions[validator_addr]

            self.log.info(f"Undelegated {amount} from {validator_addr}")

        except Exception as e:
            self.log.error(f"Error undelegating from validator: {e}")

    async def _monitor_validator_performance(self) -> None:
        """
        Monitor performance of validators we're delegated to.
        """
        try:
            for validator_addr in self.staking_positions.keys():
                performance = await self._get_validator_performance(validator_addr)

                if performance:
                    self.validator_performance[validator_addr] = performance

                    # Check if validator performance is poor
                    if performance["uptime"] < 0.95:  # Less than 95% uptime
                        self.log.warning(
                            f"Poor validator performance: {validator_addr} "
                            f"Uptime: {performance['uptime']:.2%}",
                        )

        except Exception as e:
            self.log.error(f"Error monitoring validator performance: {e}")

    async def _get_validator_performance(self, validator_addr: str) -> dict | None:
        """
        Get performance metrics for a specific validator.
        """
        try:
            # In a real implementation, this would query validator metrics

            # Placeholder implementation
            return {
                "uptime": Decimal("0.98"),
                "commission": Decimal("0.05"),
                "rewards_rate": Decimal("0.12"),  # 12% APY
                "slashing_events": 0,
            }

        except Exception as e:
            self.log.error(f"Error getting validator performance: {e}")
            return None

    async def _compound_staking_rewards(self) -> None:
        """
        Compound staking rewards automatically.

        This method claims rewards and restakes them to maximize compound growth.

        """
        try:
            # Claim rewards from all validators
            total_rewards = await self._claim_all_rewards()

            if total_rewards > Decimal("10.0"):  # Minimum threshold
                # Restake rewards
                await self._stake_tokens(total_rewards)

                self.log.info(f"Compounded {total_rewards} in staking rewards")

        except Exception as e:
            self.log.error(f"Error compounding staking rewards: {e}")

    async def _claim_all_rewards(self) -> Decimal:
        """
        Claim rewards from all validators.
        """
        try:
            total_rewards = Decimal(0)

            for validator_addr in self.staking_positions.keys():
                rewards = await self._claim_validator_rewards(validator_addr)
                total_rewards += rewards

            return total_rewards

        except Exception as e:
            self.log.error(f"Error claiming all rewards: {e}")
            return Decimal(0)

    async def _claim_validator_rewards(self, validator_addr: str) -> Decimal:
        """
        Claim rewards from a specific validator.
        """
        try:
            # In a real implementation, this would submit a claim rewards transaction

            # Placeholder implementation
            rewards = Decimal("50.0")  # $50 rewards

            self.log.debug(f"Claimed {rewards} rewards from {validator_addr}")

            return rewards

        except Exception as e:
            self.log.error(f"Error claiming validator rewards: {e}")
            return Decimal(0)

    async def _monitor_slashing_risks(self) -> None:
        """
        Monitor slashing risks and take protective actions.

        This method monitors validator behavior and network conditions to protect
        against slashing events.

        """
        try:
            for validator_addr in self.staking_positions.keys():
                risk_level = await self._assess_slashing_risk(validator_addr)

                if risk_level > 0.5:  # High risk
                    self.log.warning(
                        f"High slashing risk for validator {validator_addr}: {risk_level:.2%}",
                    )

                    # Consider undelegating from high-risk validators
                    if risk_level > 0.8:  # Very high risk
                        staked_amount = self.staking_positions[validator_addr]
                        await self._undelegate_from_validator(validator_addr, staked_amount)

        except Exception as e:
            self.log.error(f"Error monitoring slashing risks: {e}")

    async def _assess_slashing_risk(self, validator_addr: str) -> float:
        """
        Assess slashing risk for a validator.
        """
        try:
            # Get validator metrics
            performance = await self._get_validator_performance(validator_addr)

            if not performance:
                return 0.5  # Unknown risk

            # Calculate risk score based on metrics
            uptime = float(performance.get("uptime", 0.95))
            slashing_events = performance.get("slashing_events", 0)

            # Lower uptime = higher risk
            uptime_risk = 1 - uptime

            # Previous slashing events increase risk
            slashing_risk = min(slashing_events * 0.2, 0.8)

            # Combined risk score
            total_risk = min(uptime_risk + slashing_risk, 1.0)

            return total_risk

        except Exception as e:
            self.log.error(f"Error assessing slashing risk: {e}")
            return 0.5


# Example configuration and node setup
if __name__ == "__main__":
    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("STAKING-AMPLIFIER-001"),
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

    # Configure staking amplifier strategy
    strategy_config = StakingAmplifierConfig(
        staking_tokens=[
            InstrumentId.from_str("DYDX-USD-PERP.DYDX"),
        ],
        target_staking_ratio=Decimal("0.70"),  # 70% staked
        min_staking_apy=Decimal("0.08"),  # 8% minimum APY
        validator_preferences=["validator_1", "validator_2"],
        liquid_staking_enabled=True,
        auto_compound_frequency=24,  # Daily compounding
        slashing_protection_buffer=Decimal("0.05"),  # 5% buffer
    )

    # Instantiate the strategy
    strategy = StakingAmplifier(config=strategy_config)

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
