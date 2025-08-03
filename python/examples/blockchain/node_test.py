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
Python version of the Rust node_test.rs blockchain adapter demo.

This demonstrates the complete PyO3 interface for DeFi blockchain functionality,
mirroring the capabilities shown in crates/adapters/blockchain/bin/node_test.rs

"""

# ruff: noqa (under development)

import os

from nautilus_trader.adapters.blockchain import BlockchainDataClientConfig
from nautilus_trader.adapters.blockchain import BlockchainDataClientFactory
from nautilus_trader.common import ImportableActorConfig  # type: ignore[attr-defined]
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode  # type: ignore[attr-defined]
from nautilus_trader.model import Chain  # type: ignore[attr-defined]
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId


def main() -> None:
    # Environment setup
    environment = Environment.LIVE
    trader_id = TraderId("TESTER-001")
    node_name = "TESTER-001"

    print(f"Environment: {environment}")
    print(f"Trader ID: {trader_id}")
    print(f"Node name: {node_name}")

    # Chain setup
    chain = Chain.ARBITRUM()
    print(f"\nChain: {chain}")
    print(f"Chain ID: {chain.chain_id}")
    print(f"Chain name: {chain.name}")

    # RPC URLs (equivalent to get_env_var calls)
    http_rpc_url = os.getenv("RPC_HTTP_URL", "https://arb1.arbitrum.io/rpc")
    wss_rpc_url = os.getenv("RPC_WSS_URL", "wss://arb1.arbitrum.io/ws")
    from_block = 360_000_000  # Sync from reasonably recent block for now

    print(f"HTTP RPC URL: {http_rpc_url}")
    print(f"WSS RPC URL: {wss_rpc_url}")
    print(f"From block: {from_block:_}")

    # Client factory and configuration
    client_factory = BlockchainDataClientFactory()
    client_config = BlockchainDataClientConfig(
        chain=chain,
        dex_ids=["Arbitrum:UniswapV3"],
        http_rpc_url=http_rpc_url,
        wss_rpc_url=wss_rpc_url,
        use_hypersync_for_live_data=True,
        from_block=from_block,
    )

    builder = LiveNode.builder(node_name, trader_id, environment)
    builder.add_data_client(None, client_factory, client_config)
    node = builder.build()

    # Pool instrument IDs to monitor (TODO: Add to config)
    pools = [
        InstrumentId.from_str("WETH/USDC-3000.Arbitrum:UniswapV3"),  # Arbitrum WETH/USDC 0.30% pool
    ]

    actor_config = ImportableActorConfig(
        actor_path="actors:BlockchainActor",  # Import from local actors.py
        config_path="nautilus_trader.common:DataActorConfig",  # Not used yet, but required field
        config={
            "actor_id": "BLOCKCHAIN-001",
            "log_events": "true",
            "log_commands": "true",
        },
    )

    # Add actor using factory approach
    node.add_actor_from_config(actor_config)

    node.run()


if __name__ == "__main__":
    main()
