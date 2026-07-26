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

from nautilus_trader.adapters.architect_ax import AX
from nautilus_trader.adapters.architect_ax import AxDataClientConfig
from nautilus_trader.adapters.architect_ax import AxDataClientFactory
from nautilus_trader.adapters.architect_ax import AxEnvironment
from nautilus_trader.adapters.architect_ax import AxExecClientConfig
from nautilus_trader.adapters.architect_ax import AxExecutionClientFactory
from nautilus_trader.common import Environment
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.model import AccountId
from nautilus_trader.model import TraderId


SMOKE_API_KEY = "test_key"
SMOKE_API_SECRET = "test_secret"


def test_architect_ax_factories_expose_python_names() -> None:
    assert AxDataClientFactory().name() == AX
    assert AxExecutionClientFactory().name() == AX


def test_live_node_builder_accepts_architect_ax_data_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")

    node = (
        LiveNode.builder("AX-DATA-PYTEST-001", trader_id, Environment.SANDBOX)
        .add_data_client(
            None,
            AxDataClientFactory(),
            AxDataClientConfig(environment=AxEnvironment.SANDBOX),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_live_node_builder_accepts_architect_ax_exec_factory() -> None:
    trader_id = TraderId.from_str("TESTER-001")
    account_id = AccountId.from_str("AX-001")

    node = (
        LiveNode.builder("AX-EXEC-PYTEST-001", trader_id, Environment.SANDBOX)
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .add_data_client(
            None,
            AxDataClientFactory(),
            AxDataClientConfig(environment=AxEnvironment.SANDBOX),
        )
        .add_exec_client(
            None,
            AxExecutionClientFactory(),
            AxExecClientConfig(
                trader_id=trader_id,
                account_id=account_id,
                api_key=SMOKE_API_KEY,
                api_secret=SMOKE_API_SECRET,
                environment=AxEnvironment.SANDBOX,
            ),
        )
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX
