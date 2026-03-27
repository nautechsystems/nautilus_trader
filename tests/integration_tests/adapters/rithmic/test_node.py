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

import msgspec

from nautilus_trader.adapters.rithmic import RITHMIC
from nautilus_trader.adapters.rithmic import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic import RithmicExecClientConfig
from nautilus_trader.adapters.rithmic import RithmicEnvironment
from nautilus_trader.adapters.rithmic import RithmicLiveDataClientFactory
from nautilus_trader.adapters.rithmic import RithmicLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.functions import ensure_all_tasks_completed


RAW_RITHMIC_CONFIG = msgspec.json.encode(
    {
        "environment": "live",
        "trader_id": "TESTER-001",
        "logging": {"log_level": "ERROR", "log_level_file": "OFF", "use_pyo3": True},
        "data_clients": {
            "RITHMIC": {
                "path": "nautilus_trader.adapters.rithmic.config:RithmicDataClientConfig",
                "factory": {
                    "path": "nautilus_trader.adapters.rithmic.factories:RithmicLiveDataClientFactory",
                },
                "config": {
                    "environment": "demo",
                    "username": "u",
                    "password": "p",
                    "system_name": "Apex",
                    "instrument_provider": {
                        "load_all": False,
                        "filters": {"exchange": "CME"},
                    },
                },
            },
        },
        "exec_clients": {
            "RITHMIC": {
                "path": "nautilus_trader.adapters.rithmic.config:RithmicExecClientConfig",
                "factory": {
                    "path": "nautilus_trader.adapters.rithmic.factories:RithmicLiveExecClientFactory",
                },
                "config": {
                    "environment": "demo",
                    "username": "u",
                    "password": "p",
                    "system_name": "Apex",
                    "account_id": "A1",
                    "instrument_provider": {
                        "load_all": False,
                        "filters": {"exchange": "CME"},
                    },
                },
            },
        },
        "timeout_connection": 5.0,
        "timeout_reconciliation": 5.0,
        "timeout_disconnection": 1.0,
        "timeout_post_stop": 1.0,
    },
)


class TestRithmicTradingNodeIntegration:
    def teardown(self):
        ensure_all_tasks_completed()

    def test_build_node_with_rithmic_client_factories(self):
        config = TradingNodeConfig(
            trader_id=TraderId("TESTER-001"),
            logging=LoggingConfig(log_level="ERROR", log_level_file="OFF", use_pyo3=True),
            data_clients={
                RITHMIC: RithmicDataClientConfig(
                    environment=RithmicEnvironment.DEMO,
                    username="u",
                    password="p",
                    system_name="Apex",
                    instrument_provider=InstrumentProviderConfig(
                        load_all=False,
                        filters={"exchange": "CME"},
                    ),
                ),
            },
            exec_clients={
                RITHMIC: RithmicExecClientConfig(
                    environment=RithmicEnvironment.DEMO,
                    username="u",
                    password="p",
                    system_name="Apex",
                    account_id="A1",
                    instrument_provider=InstrumentProviderConfig(
                        load_all=False,
                        filters={"exchange": "CME"},
                    ),
                ),
            },
        )

        node = TradingNode(config=config)
        try:
            node.add_data_client_factory(RITHMIC, RithmicLiveDataClientFactory)
            node.add_exec_client_factory(RITHMIC, RithmicLiveExecClientFactory)
            node.build()
        finally:
            node.dispose()

    def test_parse_raw_node_config_with_rithmic_import_paths(self):
        config = TradingNodeConfig.parse(RAW_RITHMIC_CONFIG)

        node = TradingNode(config=config)
        try:
            node.build()
            assert node.trader.id.value == "TESTER-001"
        finally:
            node.dispose()
