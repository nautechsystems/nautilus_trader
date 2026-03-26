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

import os

from nautilus_trader.adapters.rithmic import RITHMIC
from nautilus_trader.adapters.rithmic import RITHMIC_CLIENT_ID
from nautilus_trader.adapters.rithmic import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic import RithmicLiveDataClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


# *** THIS IS A TEST ACTOR WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

PROFILE = os.environ.get("RITHMIC_PROFILE")
SYMBOL = os.environ.get("RITHMIC_DATA_SYMBOL", "MNQM6")
EXCHANGE = os.environ.get("RITHMIC_DATA_EXCHANGE", "CME")
BAR_SPEC = os.environ.get("RITHMIC_DATA_BAR_SPEC", "1-MINUTE-LAST-EXTERNAL")

INSTRUMENT_ID = InstrumentId.from_str(f"{SYMBOL}.{EXCHANGE}.{RITHMIC}")
BAR_TYPE = BarType.from_str(f"{INSTRUMENT_ID}-{BAR_SPEC}")


def build_data_client_config() -> RithmicDataClientConfig:
    base = RithmicDataClientConfig.from_env(PROFILE)
    return RithmicDataClientConfig(
        environment=base.environment,
        username=base.username,
        password=base.password,
        system_name=base.system_name,
        app_name=base.app_name,
        app_version=base.app_version,
        fcm_id=base.fcm_id,
        ib_id=base.ib_id,
        instrument_provider=InstrumentProviderConfig(
            load_all=False,
            filters={"exchange": EXCHANGE},
        ),
    )


config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(reconciliation=False),
    data_clients={
        RITHMIC: build_data_client_config(),
    },
    timeout_connection=10.0,
    timeout_reconciliation=10.0,
    timeout_disconnection=2.0,
    timeout_post_stop=1.0,
)

config_tester = DataTesterConfig(
    client_id=RITHMIC_CLIENT_ID,
    instrument_ids=[INSTRUMENT_ID],
    bar_types=[BAR_TYPE],
    subscribe_instrument=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    request_bars=True,
)

node = TradingNode(config=config_node)
node.trader.add_actor(DataTester(config=config_tester))
node.add_data_client_factory(RITHMIC, RithmicLiveDataClientFactory)
node.build()

try:
    node.run()
except KeyboardInterrupt:
    node.stop()
finally:
    node.dispose()
