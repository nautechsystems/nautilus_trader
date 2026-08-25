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
Stream Arbitrum DEX pool data with the built-in DataTester actor.

Running this example connects to the configured RPC endpoint and starts subscriptions
for the configured pool immediately, logging all received data. Set `ENVIO_API_TOKEN`
for HyperSync access. No orders are placed.

"""

from __future__ import annotations

import os

from nautilus_trader.adapters.blockchain import BlockchainDataClientConfig
from nautilus_trader.adapters.blockchain import BlockchainDataClientFactory
from nautilus_trader.adapters.blockchain import DexPoolFilters
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import Chain
from nautilus_trader.model import ClientId
from nautilus_trader.model import DexType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import TraderId
from nautilus_trader.testkit import DataTesterConfig


BLOCKCHAIN = "BLOCKCHAIN-Arbitrum"
TRADER_ID = TraderId.from_str("TESTER-001")
POOL_ID = InstrumentId.from_str("0x4CEf551255EC96d89feC975446301b5C4e164C59.Arbitrum:UniswapV3")
HTTP_RPC_URL = "https://arb1.arbitrum.io/rpc"
WSS_RPC_URL = None
USE_HYPERSYNC = True


def main() -> None:
    """
    Run the example.
    """
    if not os.getenv("ENVIO_API_TOKEN"):
        raise SystemExit("ENVIO_API_TOKEN must be set for HyperSync access")

    node = (
        LiveNode.builder("BLOCKCHAIN-DATA-TESTER-001", TRADER_ID, Environment.LIVE)
        .add_data_client(
            BLOCKCHAIN,
            BlockchainDataClientFactory(),
            BlockchainDataClientConfig(
                chain=Chain.ARBITRUM(),
                dex_ids=[DexType.UNISWAP_V3],
                http_rpc_url=HTTP_RPC_URL,
                wss_rpc_url=WSS_RPC_URL,
                use_hypersync_for_live_data=USE_HYPERSYNC,
                pool_filters=DexPoolFilters(remove_pools_with_empty_erc20_fields=True),
            ),
        )
        .build()
    )
    node.add_builtin_actor(
        "DataTester",
        DataTesterConfig(
            client_id=ClientId.from_str(BLOCKCHAIN),
            instrument_ids=[POOL_ID],
            request_instruments=True,
            log_data=True,
        ),
    )

    node.run()


if __name__ == "__main__":
    main()
