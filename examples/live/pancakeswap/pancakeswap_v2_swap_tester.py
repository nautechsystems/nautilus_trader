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

from __future__ import annotations

import time

from nautilus_trader.adapters.pancakeswap import PancakeSwapV2ExecClientConfig
from nautilus_trader.adapters.pancakeswap import PancakeSwapV2ExecClientFactory
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import TraderId


# This script demonstrates constructing a PancakeSwap execution client through
# the Rust PyO3 live-node path. It is intentionally minimal and does not submit
# live orders.

CONFIG = PancakeSwapV2ExecClientConfig(
    trader_id=TraderId("TESTER-001"),
    client_id=AccountId("SIM-001"),
    wallet_address="0x49E96E255bA418d08E66c35b588E2f2F3766E1d0",
    http_rpc_url="https://bsc.example.com",
    signer_endpoint="https://signer.example.com",
)


def main() -> None:
    builder = nautilus_pyo3.LiveNode.builder(
        name="PCS_V2_TESTER",
        trader_id=TraderId("TESTER-001"),
        environment=nautilus_pyo3.Environment.SANDBOX,
    )
    builder = PancakeSwapV2ExecClientFactory.add_to_builder(
        builder=builder,
        config=CONFIG,
        name="BLOCKCHAIN",
    )

    node = builder.build()

    try:
        node.start()
        time.sleep(2.0)
    finally:
        if node.is_running:
            node.stop()


if __name__ == "__main__":
    main()
