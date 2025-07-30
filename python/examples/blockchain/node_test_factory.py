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

from nautilus_trader.common import Environment
from nautilus_trader.common import ImportableActorConfig  # type: ignore[attr-defined]
from nautilus_trader.live import LiveNode  # type: ignore[attr-defined]
from nautilus_trader.model import TraderId


def test_factory_approach():
    """
    Test creating and adding actors using factory approach.
    """
    # Create LiveNode
    trader_id = TraderId("TESTER-001")
    node = LiveNode.builder("test_factory", trader_id, Environment.SANDBOX).build()

    # Create ImportableActorConfig for BlockchainActor
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
    print("Successfully added actor from config")

    node.start()
    print("Successfully started node with factory-created actor")

    node.stop()
    print("Successfully stopped node")


if __name__ == "__main__":
    test_factory_approach()
