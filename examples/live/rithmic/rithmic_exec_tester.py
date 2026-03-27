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
from decimal import Decimal

from nautilus_trader.adapters.rithmic import RITHMIC
from nautilus_trader.adapters.rithmic import RITHMIC_CLIENT_ID
from nautilus_trader.adapters.rithmic import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic import RithmicExecClientConfig
from nautilus_trader.adapters.rithmic import RithmicLiveDataClientFactory
from nautilus_trader.adapters.rithmic import RithmicLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

PROFILE = os.environ.get("RITHMIC_PROFILE")
SYMBOL = os.environ.get("RITHMIC_EXEC_SYMBOL", "MNQM6")
EXCHANGE = os.environ.get("RITHMIC_EXEC_EXCHANGE", "CME")
ORDER_QTY = Decimal(os.environ.get("RITHMIC_EXEC_ORDER_QTY", "1"))
TOB_OFFSET_TICKS = int(os.environ.get("RITHMIC_EXEC_TOB_OFFSET_TICKS", "20"))

INSTRUMENT_ID = InstrumentId.from_str(f"{SYMBOL}.{EXCHANGE}.{RITHMIC}")


def build_provider_config() -> InstrumentProviderConfig:
    return InstrumentProviderConfig(
        load_all=False,
        filters={"exchange": EXCHANGE},
    )


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
        server=base.server,
        alt_server=base.alt_server,
        instrument_provider=build_provider_config(),
    )


def build_exec_client_config() -> RithmicExecClientConfig:
    base = RithmicExecClientConfig.from_env(PROFILE)
    return RithmicExecClientConfig(
        environment=base.environment,
        username=base.username,
        password=base.password,
        system_name=base.system_name,
        account_id=base.account_id,
        app_name=base.app_name,
        app_version=base.app_version,
        fcm_id=base.fcm_id,
        ib_id=base.ib_id,
        server=base.server,
        alt_server=base.alt_server,
        execution_replay_lookback_secs=base.execution_replay_lookback_secs,
        native_bracket_state_path=base.native_bracket_state_path,
        instrument_provider=build_provider_config(),
    )


config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_instrument_ids=[INSTRUMENT_ID],
        open_check_interval_secs=5.0,
        open_check_open_only=False,
    ),
    data_clients={
        RITHMIC: build_data_client_config(),
    },
    exec_clients={
        RITHMIC: build_exec_client_config(),
    },
    timeout_connection=10.0,
    timeout_reconciliation=10.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
    timeout_shutdown=2.0,
)

config_tester = ExecTesterConfig(
    instrument_id=INSTRUMENT_ID,
    client_id=RITHMIC_CLIENT_ID,
    order_qty=ORDER_QTY,
    enable_limit_buys=True,
    enable_limit_sells=True,
    tob_offset_ticks=TOB_OFFSET_TICKS,
    limit_time_in_force=TimeInForce.DAY,
    close_positions_time_in_force=TimeInForce.IOC,
    log_data=False,
)

node = TradingNode(config=config_node)
node.trader.add_strategy(ExecTester(config=config_tester))
node.add_data_client_factory(RITHMIC, RithmicLiveDataClientFactory)
node.add_exec_client_factory(RITHMIC, RithmicLiveExecClientFactory)
node.build()

try:
    node.run()
except KeyboardInterrupt:
    node.stop()
finally:
    node.dispose()
