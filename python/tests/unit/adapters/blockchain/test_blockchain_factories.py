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

import pytest
from unit.adapters.example_modules import capture_data_tester_main
from unit.adapters.example_modules import load_example_module

from nautilus_trader.adapters.blockchain import BlockchainDataClientConfig
from nautilus_trader.adapters.blockchain import BlockchainDataClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.model import Chain
from nautilus_trader.model import DexType
from nautilus_trader.model import TraderId


BLOCKCHAIN = "BLOCKCHAIN"
blockchain_data_tester = load_example_module("blockchain", "data_tester")


def test_blockchain_data_factory_exposes_python_name() -> None:
    assert BlockchainDataClientFactory().name() == BLOCKCHAIN


def test_live_node_builder_accepts_blockchain_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("BLOCKCHAIN-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            "BLOCKCHAIN-Arbitrum",
            BlockchainDataClientFactory(),
            BlockchainDataClientConfig(
                chain=Chain.ARBITRUM(),
                dex_ids=[DexType.UNISWAP_V3],
                http_rpc_url="https://arb1.arbitrum.io/rpc",
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_blockchain_data_tester_builds_offline(monkeypatch: pytest.MonkeyPatch) -> None:
    captured = capture_data_tester_main(monkeypatch, blockchain_data_tester, [])
    kwargs = captured["data_tester_kwargs"]

    assert isinstance(kwargs, dict)
    assert kwargs["request_instruments"] is True
    assert "exec_client_args" not in captured
    assert "run_called" not in captured
