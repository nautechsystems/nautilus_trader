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

import os

from nautilus_trader.common import Environment
from nautilus_trader.common import ImportableActorConfig  # type: ignore[attr-defined]
from nautilus_trader.infrastructure import PostgresConnectOptions
from nautilus_trader.live import LiveNode  # type: ignore[attr-defined]
from nautilus_trader.model import TraderId


def test_factory_approach():
    """
    Test creating and adding actors using factory approach.

    Also tests PostgresConnectOptions Python bindings.

    """
    # Test PostgresConnectOptions creation
    postgres_config = PostgresConnectOptions(
        host=os.getenv("POSTGRES_HOST", "localhost"),
        port=int(os.getenv("POSTGRES_PORT", "5432")),
        user=os.getenv("POSTGRES_USERNAME", "nautilus"),
        password=os.getenv("POSTGRES_PASSWORD", "pass"),
        database=os.getenv("POSTGRES_DATABASE", "nautilus"),
    )
    print(f"PostgresConnectOptions created: {postgres_config}")
    print(f"  host: {postgres_config.host}")
    print(f"  port: {postgres_config.port}")
    print(f"  username: {postgres_config.username}")
    print(f"  database: {postgres_config.database}")

    trader_id = TraderId("TESTER-001")
    node = LiveNode.builder("test_factory", trader_id, Environment.SANDBOX).build()

    actor_config = ImportableActorConfig(
        actor_path="actors:BlockchainActor",
        config_path="actors:BlockchainActorConfig",
        config={
            "actor_id": "BLOCKCHAIN-001",
            "log_events": True,
            "log_commands": True,
            "chain": "Arbitrum",
            "client_id": "BLOCKCHAIN-Arbitrum",
            "pools": ["0xC31E54c7a869B9FcBEcc14363CF510d1c41fa443.Arbitrum:UniswapV3"],
        },
    )

    # Add actor using factory approach
    node.add_actor_from_config(actor_config)
    print("Successfully added actor from config")

    node.start()
    print("Successfully started node with factory-created actor")

    node.stop()
    print("Successfully stopped node")


if __name__ == "__main__":
    test_factory_approach()
