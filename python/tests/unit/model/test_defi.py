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


from nautilus_trader.model import AmmType
from nautilus_trader.model import Block
from nautilus_trader.model import Blockchain
from nautilus_trader.model import Chain
from nautilus_trader.model import Dex
from nautilus_trader.model import DexType
from nautilus_trader.model import Pool
from nautilus_trader.model import PoolFeeCollect
from nautilus_trader.model import PoolFlash
from nautilus_trader.model import PoolLiquidityUpdate
from nautilus_trader.model import PoolLiquidityUpdateType
from nautilus_trader.model import PoolProfiler
from nautilus_trader.model import PoolSwap
from nautilus_trader.model import Token
from nautilus_trader.model import Transaction


def test_chain_construction_and_lookup():
    chain = Chain(Blockchain.BASE, 8453)
    lookup_by_name = Chain.from_chain_name("BASE")
    lookup_by_id = Chain.from_chain_id(8453)

    assert chain.name == Blockchain.BASE
    assert chain.chain_id == 8453
    assert isinstance(hash(chain), int)
    assert lookup_by_name.name == Blockchain.BASE
    assert lookup_by_name.chain_id == 8453
    assert lookup_by_id is not None
    assert lookup_by_id.name == Blockchain.BASE


def test_defi_enum_exports():
    assert AmmType.from_str("CLAMM") == AmmType.CLAMM
    assert Blockchain.from_str("BASE") == Blockchain.BASE
    assert DexType.UNISWAP_V3 is not None


def test_defi_public_module_names():
    assert Blockchain.__module__ == "nautilus_trader.model"
    assert Chain.__module__ == "nautilus_trader.model"
    assert Dex.__module__ == "nautilus_trader.model"
    assert DexType.__module__ == "nautilus_trader.model"


def test_dex_and_token_properties():
    chain = Chain(Blockchain.BASE, 8453)
    dex = _make_dex(chain)
    token0 = _make_token0(chain)
    token1 = _make_token1(chain)

    assert dex.chain == chain
    assert dex.name == DexType.UNISWAP_V3
    assert dex.factory == "0x0000000000000000000000000000000000000fac"
    assert dex.factory_creation_block == 1
    assert dex.amm_type == AmmType.CLAMM
    assert dex.pool_created_event.startswith("0x")
    assert dex.swap_created_event.startswith("0x")
    assert dex.mint_created_event.startswith("0x")
    assert dex.burn_created_event.startswith("0x")
    assert isinstance(hash(dex), int)
    assert token0.chain == chain
    assert token0.address == "0x0000000000000000000000000000000000000001"
    assert token0.name == "USD Coin"
    assert token0.symbol == "USDC"
    assert token0.decimals == 6
    assert token1.symbol == "WETH"
    assert token1.decimals == 18


def test_pool_construction_and_properties():
    pool = _make_pool()

    assert pool.chain.name == Blockchain.BASE
    assert pool.dex.name == DexType.UNISWAP_V3
    assert type(pool.instrument_id).__name__ == "InstrumentId"
    assert str(pool.instrument_id) == "0x0000000000000000000000000000000000000003.Base:UniswapV3"
    assert pool.address == "0x0000000000000000000000000000000000000003"
    assert pool.creation_block == 1
    assert pool.token0.symbol == "USDC"
    assert pool.token1.symbol == "WETH"
    assert pool.fee == 500
    assert pool.tick_spacing == 10
    assert pool.ts_init == 2
    assert isinstance(hash(pool), int)


def test_pool_event_types_construction():
    pool = _make_pool()
    swap = _make_pool_swap(pool)
    liquidity = _make_pool_liquidity_update(pool)
    fee_collect = _make_pool_fee_collect(pool)
    flash = _make_pool_flash(pool)

    assert swap.chain.name == Blockchain.BASE
    assert swap.dex.name == DexType.UNISWAP_V3
    assert swap.instrument_id == pool.instrument_id
    assert swap.pool_identifier == pool.address
    assert swap.block == 1
    assert (
        swap.transaction_hash
        == "0x3333333333333333333333333333333333333333333333333333333333333333"
    )
    assert swap.sender == "0x0000000000000000000000000000000000000004"
    assert swap.timestamp == 10
    assert liquidity.kind == PoolLiquidityUpdateType.MINT
    assert liquidity.owner == "0x0000000000000000000000000000000000000004"
    assert liquidity.position_liquidity == "10"
    assert liquidity.timestamp == 10
    assert fee_collect.owner == "0x0000000000000000000000000000000000000004"
    assert fee_collect.amount0 == "1"
    assert fee_collect.amount1 == "2"
    assert flash.sender == "0x0000000000000000000000000000000000000004"
    assert flash.recipient == "0x0000000000000000000000000000000000000005"
    assert flash.paid0 == "3"
    assert flash.paid1 == "4"


def test_transaction_and_opaque_defi_surfaces():
    tx = _make_transaction(Chain(Blockchain.BASE, 8453))

    assert tx.chain.name == Blockchain.BASE
    assert tx.hash == "0x1111111111111111111111111111111111111111111111111111111111111111"
    assert tx.block_hash == "0x2222222222222222222222222222222222222222222222222222222222222222"
    assert tx.block_number == 1
    assert tx.transaction_index == 0
    assert getattr(tx, "from") == "0x0000000000000000000000000000000000000004"
    assert tx.to == "0x0000000000000000000000000000000000000005"
    assert tx.value == "0"
    assert tx.gas == "21000"
    assert tx.gas_price == "100"
    assert hasattr(Block, "hash")
    assert hasattr(Block, "number")
    assert hasattr(Block, "parent_hash")
    assert hasattr(Block, "timestamp")


def test_pool_profiler_surface_methods():
    assert isinstance(PoolProfiler, type)
    assert PoolProfiler.__name__ == "PoolProfiler"
    assert hasattr(PoolProfiler, "pool")
    assert hasattr(PoolProfiler, "current_tick")
    assert hasattr(PoolProfiler, "swap_exact_in")
    assert hasattr(PoolProfiler, "swap_exact_out")
    assert hasattr(PoolProfiler, "size_for_impact_bps_detailed")


def _make_dex(chain):
    return Dex(
        chain=chain,
        name="UniswapV3",
        factory="0x0000000000000000000000000000000000000fac",
        factory_creation_block=1,
        amm_type="CLAMM",
        pool_created_event="PoolCreated",
        swap_event="Swap",
        mint_event="Mint",
        burn_event="Burn",
        collect_event="Collect",
    )


def _make_pool():
    chain = Chain(Blockchain.BASE, 8453)
    dex = _make_dex(chain)
    token0 = _make_token0(chain)
    token1 = _make_token1(chain)
    return Pool(
        chain=chain,
        dex=dex,
        address="0x0000000000000000000000000000000000000003",
        pool_identifier="0x0000000000000000000000000000000000000003",
        creation_block=1,
        token0=token0,
        token1=token1,
        fee=500,
        tick_spacing=10,
        ts_init=2,
    )


def _make_pool_fee_collect(pool):
    return PoolFeeCollect(
        chain=pool.chain,
        dex=pool.dex,
        pool_identifier=pool.address,
        instrument_id=pool.instrument_id,
        block=1,
        transaction_hash="0x5555555555555555555555555555555555555555555555555555555555555555",
        transaction_index=0,
        log_index=1,
        owner="0x0000000000000000000000000000000000000004",
        amount0="1",
        amount1="2",
        tick_lower=-10,
        tick_upper=10,
        timestamp=10,
    )


def _make_pool_flash(pool):
    return PoolFlash(
        chain=pool.chain,
        dex=pool.dex,
        pool_identifier=pool.address,
        instrument_id=pool.instrument_id,
        block=1,
        transaction_hash="0x6666666666666666666666666666666666666666666666666666666666666666",
        transaction_index=0,
        log_index=1,
        sender="0x0000000000000000000000000000000000000004",
        recipient="0x0000000000000000000000000000000000000005",
        amount0="1",
        amount1="2",
        paid0="3",
        paid1="4",
        timestamp=10,
    )


def _make_pool_liquidity_update(pool):
    return PoolLiquidityUpdate(
        chain=pool.chain,
        dex=pool.dex,
        pool_identifier=pool.address,
        instrument_id=pool.instrument_id,
        kind=PoolLiquidityUpdateType.MINT,
        block=1,
        transaction_hash="0x4444444444444444444444444444444444444444444444444444444444444444",
        transaction_index=0,
        log_index=1,
        sender=None,
        owner="0x0000000000000000000000000000000000000004",
        position_liquidity="10",
        amount0="1",
        amount1="2",
        tick_lower=-10,
        tick_upper=10,
        timestamp=10,
    )


def _make_pool_swap(pool):
    return PoolSwap(
        chain=pool.chain,
        dex=pool.dex,
        instrument_id=pool.instrument_id,
        pool_identifier=pool.address,
        block=1,
        transaction_hash="0x3333333333333333333333333333333333333333333333333333333333333333",
        transaction_index=0,
        log_index=1,
        timestamp=10,
        sender="0x0000000000000000000000000000000000000004",
        receiver="0x0000000000000000000000000000000000000005",
        amount0="1",
        amount1="-2",
        sqrt_price_x96="79228162514264337593543950336",
        liquidity=100,
        tick=1,
    )


def _make_token0(chain):
    return Token(
        chain=chain,
        address="0x0000000000000000000000000000000000000001",
        name="USD Coin",
        symbol="USDC",
        decimals=6,
    )


def _make_token1(chain):
    return Token(
        chain=chain,
        address="0x0000000000000000000000000000000000000002",
        name="Wrapped Ether",
        symbol="WETH",
        decimals=18,
    )


def _make_transaction(chain):
    return Transaction(
        chain,
        "0x1111111111111111111111111111111111111111111111111111111111111111",
        "0x2222222222222222222222222222222222222222222222222222222222222222",
        1,
        "0x0000000000000000000000000000000000000004",
        "0x0000000000000000000000000000000000000005",
        "21000",
        "100",
        0,
        "0",
    )
