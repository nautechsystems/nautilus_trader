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
Example of blockchain actors.
"""

from typing import Any
from typing import Self

from nautilus_trader.common import DataActor
from nautilus_trader.common import LogColor
from nautilus_trader.common import TimeEvent
from nautilus_trader.config import DataActorConfig
from nautilus_trader.model import ActorId
from nautilus_trader.model import Block
from nautilus_trader.model import Chain
from nautilus_trader.model import ClientId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Pool
from nautilus_trader.model import PoolFeeCollect
from nautilus_trader.model import PoolFlash
from nautilus_trader.model import PoolLiquidityUpdate
from nautilus_trader.model import PoolSwap


class BlockchainActorConfig(DataActorConfig):
    """
    Collect blockchain actor config tests.
    """

    def __new__(cls, *args: Any, **kwargs: Any) -> Self:
        """
        Create a new instance.
        """
        # `actor_id` shares the base field name but widens the type to accept a string,
        # so keep it from the base constructor, which validates it as an `ActorId`
        kwargs.pop("actor_id", None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        *,
        actor_id: ActorId | str | None = None,
        log_events: bool = True,
        log_commands: bool = True,
        chain: Chain | str | None = None,
        client_id: ClientId | str | None = None,
        pools: list[InstrumentId | str] | None = None,
    ) -> None:
        """
        Initialize the instance.
        """
        self.actor_id = ActorId.from_str(actor_id) if isinstance(actor_id, str) else actor_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.chain = Chain.from_chain_name(chain) if isinstance(chain, str) else chain
        self.client_id = ClientId.from_str(client_id) if isinstance(client_id, str) else client_id
        self.pools = (
            [InstrumentId.from_str(pool) if isinstance(pool, str) else pool for pool in pools]
            if pools is not None
            else None
        )


class BlockchainActor(DataActor):
    """
    Collect blockchain actor tests.
    """

    def __init__(self, config: BlockchainActorConfig | None = None) -> None:
        """
        Initialize the instance.
        """
        if config is None:
            config = BlockchainActorConfig()
        super().__init__(config)

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

    def on_time_event(self, event: TimeEvent) -> None:
        """
        Actions to be performed on receiving a time event.
        """
        self.log.info(repr(event), LogColor.BLUE)

    def on_pool(self, pool: Pool) -> None:
        """
        On pool.
        """
        log_msg = f"Received pool: {pool.instrument_id}"
        self.log.info(log_msg, color=LogColor.GREEN)

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
            log_msg = f"Pool {pool_id} contains {total_ticks} active ticks and {total_positions} active positions with liquidity of {liquidity}"
            self.log.info(log_msg, color=LogColor.BLUE)
            log_msg = f"Pool {pool_id} has a liquidity utilization rate of {liquidity_utilization_rate * 100:.4f}%"
            self.log.info(log_msg, color=LogColor.BLUE)

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

    def on_pool_flash(self, flash: PoolFlash) -> None:
        """
        Actions to be performed on receiving a pool flash event.
        """
        self.log.info(repr(flash), LogColor.CYAN)
