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

# ruff: noqa (under development)

from dataclasses import dataclass

from nautilus_trader.common import DataActor  # type: ignore[attr-defined]
from nautilus_trader.common import DataActorConfig  # type: ignore[attr-defined]
from nautilus_trader.common import LogColor  # type: ignore[attr-defined]
from nautilus_trader.model import ActorId  # type: ignore[attr-defined]
from nautilus_trader.model import Block  # type: ignore[attr-defined]
from nautilus_trader.model import Chain  # type: ignore[attr-defined]
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import PoolFeeCollect  # type: ignore[attr-defined]
from nautilus_trader.model import PoolFlash  # type: ignore[attr-defined]
from nautilus_trader.model import PoolLiquidityUpdate  # type: ignore[attr-defined]
from nautilus_trader.model import PoolSwap  # type: ignore[attr-defined]


@dataclass
class BlockchainActorConfig(DataActorConfig):
    # Inherited fields from DataActorConfig (must be included for now)
    actor_id: ActorId | None = None
    log_events: bool = True
    log_commands: bool = True

    # Blockchain-specific fields
    chain: Chain | None = None
    client_id: ClientId | None = None
    pools: list[InstrumentId] | None = None

    def __post_init__(self):
        if isinstance(self.actor_id, str):
            self.actor_id = ActorId(self.actor_id)

        if isinstance(self.client_id, str):
            self.client_id = ClientId(self.client_id)

        if isinstance(self.pools, list) and self.pools and isinstance(self.pools[0], str):
            self.pools = [InstrumentId.from_str(pool_str) for pool_str in self.pools]

        if isinstance(self.chain, str):
            self.chain = Chain.from_chain_name(self.chain)


class BlockchainActor(DataActor):

    def __init__(self, config: BlockchainActorConfig | None = None) -> None:
        if config is None:
            config = BlockchainActorConfig()
        super().__init__()

        self.chain = config.chain or Chain.ARBITRUM()
        self.client_id = config.client_id or ClientId(f"BLOCKCHAIN-{self.chain.name}")
        self.pools = config.pools or [
            InstrumentId.from_str("0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:UniswapV3"),
        ]

    def on_start(self) -> None:
        """
        Actions to be performed on actor start.
        """
        self.subscribe_blocks(self.chain.name)

        for instrument_id in self.pools:
            self.subscribe_pool(instrument_id, self.client_id)
            self.subscribe_pool_swaps(instrument_id, self.client_id)
            self.subscribe_pool_liquidity_updates(instrument_id, self.client_id)
            self.subscribe_pool_fee_collects(instrument_id, self.client_id)
            self.subscribe_pool_flash_events(instrument_id, self.client_id)

        # TODO: Uncomment to demonstrate timers
        # import pandas as pd
        # self.clock.set_timer("TEST-TIMER-SECONDS-1", pd.Timedelta(seconds=1))
        # self.clock.set_timer("TEST-TIMER-SECONDS-2", pd.Timedelta(seconds=2))

    def on_stop(self) -> None:
        """
        Actions to be performed on actor stop.
        """
        self.unsubscribe_blocks(self.chain.name)

        for instrument_id in self.pools:
            self.unsubscribe_pool(instrument_id, self.client_id)
            self.unsubscribe_pool_swaps(instrument_id, self.client_id)
            self.unsubscribe_pool_liquidity_updates(instrument_id, self.client_id)
            self.unsubscribe_pool_fee_collects(instrument_id, self.client_id)
            self.unsubscribe_pool_flash_events(instrument_id, self.client_id)

    def on_time_event(self, event) -> None:
        """
        Actions to be performed on receiving a time event.
        """
        self.log.info(repr(event), LogColor.BLUE)

    def on_pool(self, pool) -> None:
        self.log.info(f"Received pool: {pool.instrument_id}", LogColor.GREEN)

    def on_block(self, block: Block) -> None:
        """
        Actions to be performed on receiving a block.
        """
        self.log.info(repr(block), LogColor.CYAN)

        for pool_id in self.pools:
            pool = self.cache.pool_profiler(pool_id)
            if pool is None:
                continue
            total_ticks = pool.get_active_tick_count()
            total_positions = pool.get_total_active_positions()
            liquidity = pool.get_active_liquidity()
            liquidity_utilization_rate = pool.liquidity_utilization_rate()
            self.log.info(
                f"Pool {pool_id} contains {total_ticks} active ticks and {total_positions} active positions with liquidity of {liquidity}",
                LogColor.BLUE,
            )
            self.log.info(
                f"Pool {pool_id} has a liquidity utilization rate of {liquidity_utilization_rate * 100:.4f}%",
                LogColor.BLUE,
            )

    def on_pool_swap(self, swap: PoolSwap) -> None:
        """
        Actions to be performed on receiving a pool swap.
        """
        self.log.info(repr(swap), LogColor.CYAN)

    def on_pool_liquidity_update(self, update: PoolLiquidityUpdate) -> None:
        """
        Actions to be performed on receiving a pool liquidity update.
        """
        self.log.info(repr(update), LogColor.CYAN)

    def on_pool_fee_collect(self, update: PoolFeeCollect) -> None:
        """
        Actions to be performed on receiving a pool fee collect event.
        """
        self.log.info(repr(update), LogColor.CYAN)

    def on_pool_flash(self, event: PoolFlash) -> None:
        """
        Actions to be performed on receiving a pool flash event.
        """
        self.log.info(repr(event), LogColor.CYAN)
