#!/usr/bin/env python3
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
Python version of the Rust node_test.rs blockchain adapter demo.

This demonstrates the complete PyO3 interface for DeFi blockchain functionality,
mirroring the capabilities shown in crates/adapters/blockchain/bin/node_test.rs. Running
this example connects to the configured RPC endpoint and starts pool subscriptions
immediately. Set `ENVIO_API_TOKEN` for HyperSync access.

"""

import os

from nautilus_trader.adapters.blockchain import BlockchainDataClientConfig
from nautilus_trader.adapters.blockchain import BlockchainDataClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.config import ImportableActorConfig
from nautilus_trader.infrastructure import PostgresConnectOptions
from nautilus_trader.live import LiveNode
from nautilus_trader.model import Chain
from nautilus_trader.model import DexType
from nautilus_trader.model import TraderId


ENVIRONMENT = Environment.LIVE
TRADER_ID = TraderId.from_str("TESTER-001")
NODE_NAME = "TESTER-001"
CHAIN = Chain.ARBITRUM()
HTTP_RPC_URL = os.getenv("RPC_HTTP_URL", "https://arb1.arbitrum.io/rpc")
WSS_RPC_URL = os.getenv("RPC_WSS_URL", "wss://arb1.arbitrum.io/ws")
FROM_BLOCK = 0
USE_HYPERSYNC_FOR_LIVE_DATA = True
USE_POSTGRES_CACHE = False


def main() -> None:
    if not os.getenv("ENVIO_API_TOKEN"):
        raise SystemExit("ENVIO_API_TOKEN must be set for HyperSync access")

    print(f"Environment: {ENVIRONMENT}")
    print(f"Trader ID: {TRADER_ID}")
    print(f"Node name: {NODE_NAME}")
    print(f"Chain: {CHAIN}")
    print(f"From block: {FROM_BLOCK:_}")

    postgres_config = None

    if USE_POSTGRES_CACHE:
        postgres_config = PostgresConnectOptions(
            host=os.getenv("POSTGRES_HOST", "localhost"),
            port=int(os.getenv("POSTGRES_PORT", "5432")),
            user=os.getenv("POSTGRES_USERNAME", "nautilus"),
            password=os.getenv("POSTGRES_PASSWORD", "pass"),
            database=os.getenv("POSTGRES_DATABASE", "nautilus"),
        )
        print(f"\nPostgres cache config: {postgres_config}")

    client_factory = BlockchainDataClientFactory()
    client_config = BlockchainDataClientConfig(
        chain=CHAIN,
        dex_ids=[
            DexType.UNISWAP_V3,
        ],
        http_rpc_url=HTTP_RPC_URL,
        wss_rpc_url=WSS_RPC_URL,
        use_hypersync_for_live_data=USE_HYPERSYNC_FOR_LIVE_DATA,
        from_block=FROM_BLOCK,
        postgres_cache_database_config=postgres_config,
    )

    builder = LiveNode.builder(NODE_NAME, TRADER_ID, ENVIRONMENT)
    builder.add_data_client("BLOCKCHAIN-Arbitrum", client_factory, client_config)
    node = builder.build()

    actor_config = ImportableActorConfig(
        actor_path="actors:BlockchainActor",
        config_path="actors:BlockchainActorConfig",
        config={
            "actor_id": "BLOCKCHAIN-001",
            "log_events": True,
            "log_commands": True,
            "chain": "Arbitrum",
            "client_id": "BLOCKCHAIN-Arbitrum",
            "pools": [
                "0xD491076C7316bC28fD4D35E3da9aB5286D079250.Arbitrum:UniswapV3",
            ],
        },
    )

    node.add_actor_from_config(actor_config)

    node.run()


if __name__ == "__main__":
    main()
