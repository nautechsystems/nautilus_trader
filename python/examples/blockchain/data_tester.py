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
Blockchain Python v2 data tester example.

The default path builds a live node and attaches the native Rust DataTester without
connecting to the configured RPC endpoint. Pass --run to start subscriptions.

"""

from __future__ import annotations

import argparse

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


def main() -> None:
    args = parse_args()
    pool_id = InstrumentId.from_str(args.pool)

    builder = LiveNode.builder(
        "BLOCKCHAIN-DATA-TESTER-001",
        TraderId.from_str(args.trader_id),
        Environment.LIVE,
    ).add_data_client(
        BLOCKCHAIN,
        BlockchainDataClientFactory(),
        BlockchainDataClientConfig(
            chain=Chain.ARBITRUM(),
            dex_ids=[DexType.UNISWAP_V3],
            http_rpc_url=args.http_rpc_url,
            wss_rpc_url=args.wss_rpc_url,
            use_hypersync_for_live_data=args.use_hypersync,
            pool_filters=DexPoolFilters(remove_pools_with_empty_erc20_fields=True),
        ),
    )

    node = builder.build()
    node.add_native_actor(
        DataTesterConfig(
            client_id=ClientId.from_str(BLOCKCHAIN),
            instrument_ids=[pool_id],
            request_instruments=True,
            log_data=True,
        ),
    )

    if args.run:
        node.run()
    else:
        print("Built Blockchain data tester node. Pass --run to connect.")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Build or run the Blockchain Python v2 data tester.",
    )
    parser.add_argument("--trader-id", default="TESTER-001")
    parser.add_argument("--http-rpc-url", default="https://arb1.arbitrum.io/rpc")
    parser.add_argument("--wss-rpc-url", default=None)
    parser.add_argument(
        "--pool",
        default="0x4CEf551255EC96d89feC975446301b5C4e164C59.Arbitrum:UniswapV3",
    )
    parser.add_argument(
        "--use-hypersync",
        action=argparse.BooleanOptionalAction,
        default=True,
    )
    parser.add_argument("--run", action="store_true")
    return parser.parse_args()


if __name__ == "__main__":
    main()
