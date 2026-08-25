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


from nautilus_trader.adapters.bitmex import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex import BitmexDataClientFactory
from nautilus_trader.adapters.bitmex import BitmexEnvironment
from nautilus_trader.adapters.bitmex import BitmexExecutionClientConfig
from nautilus_trader.adapters.bitmex import BitmexExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


BITMEX = "BITMEX"
SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"


def test_bitmex_factories_expose_python_names() -> None:
    assert BitmexDataClientFactory().name() == BITMEX
    assert BitmexExecutionClientFactory().name() == BITMEX


def test_live_node_builder_accepts_bitmex_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("BITMEX-DATA-PYTEST-001", trader_id, Environment.LIVE)
        .add_data_client(
            None,
            BitmexDataClientFactory(),
            BitmexDataClientConfig(environment=BitmexEnvironment.TESTNET),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE


def test_live_node_builder_accepts_bitmex_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("BITMEX-001")

    node = (
        LiveNode.builder("BITMEX-EXEC-PYTEST-001", trader_id, Environment.LIVE)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            BitmexDataClientFactory(),
            BitmexDataClientConfig(environment=BitmexEnvironment.TESTNET),
        )
        .add_exec_client(
            None,
            BitmexExecutionClientFactory(),
            BitmexExecutionClientConfig(
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                environment=BitmexEnvironment.TESTNET,
                account_id=account_id,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.LIVE
