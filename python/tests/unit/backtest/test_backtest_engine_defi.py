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

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import DataActor
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.model import Block
from nautilus_trader.model import Blockchain
from nautilus_trader.model import DefiData
from tests.unit.model.test_defi import _make_pool
from tests.unit.model.test_defi import _make_pool_liquidity_update


class DefiBlockActor(DataActor):
    received_blocks: list[int] = []

    def on_start(self):
        self.subscribe_blocks(Blockchain.BASE)

    def on_block(self, block):
        type(self).received_blocks.append(block.number)


def test_defi_data_uses_model_timestamp_contract():
    data = DefiData.Block(_make_block(7, 100))
    pool = _make_pool()
    liquidity = _make_pool_liquidity_update(pool)
    pool_data = DefiData.Pool(pool)
    liquidity_data = DefiData.PoolLiquidityUpdate(liquidity)

    assert data.block_position() == (7, 0, 0)
    assert data.block_number == 7
    assert data.transaction_index == 0
    assert data.log_index == 0
    assert data.timestamp == 100
    assert data.ts_event == 100
    assert data.ts_init == 100
    assert pool_data.block_position() == (1, 0, 0)
    assert pool_data.timestamp == 2
    assert pool_data.ts_event == 2
    assert pool_data.ts_init == 2
    assert liquidity_data.block_position() == (1, 0, 1)
    assert liquidity_data.timestamp == 10
    assert liquidity_data.ts_event == 10
    assert liquidity_data.ts_init == 10


def test_backtest_engine_replays_defi_blocks_to_actor_subscription():
    DefiBlockActor.received_blocks = []
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_actor_from_config(
        ImportableActorConfig(
            actor_path="tests.unit.backtest.test_backtest_engine_defi:DefiBlockActor",
            config_path="nautilus_trader.common:DataActorConfig",
            config={"actor_id": "DEFI-BLOCK-ACTOR-001"},
        ),
    )
    engine.add_defi_data(
        [
            DefiData.Block(_make_block(2, 20)),
            DefiData.Block(_make_block(1, 10)),
        ],
    )

    try:
        engine.run()

        assert DefiBlockActor.received_blocks == [1, 2]
        assert engine.iteration == 2
        assert engine.backtest_start == 10
        assert engine.backtest_end == 20
    finally:
        engine.dispose()


def test_backtest_engine_accepts_python_defi_pool_event_replay_data():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    pool = _make_pool()
    liquidity = _make_pool_liquidity_update(pool)

    engine.add_data(
        [
            DefiData.PoolLiquidityUpdate(liquidity),
            DefiData.Pool(pool),
        ],
    )

    try:
        engine.run()

        assert engine.iteration == 2
        assert engine.backtest_start == 2
        assert engine.backtest_end == 10
    finally:
        engine.dispose()


def _make_block(number: int, timestamp: int) -> Block:
    return Block(
        Blockchain.BASE,
        f"0x{number:064x}",
        "0x0000000000000000000000000000000000000000000000000000000000000000",
        number,
        "0x0000000000000000000000000000000000000001",
        30_000_000,
        21_000,
        timestamp,
    )
