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
import signal
import threading
import time

from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers.common import IBContract
from nautilus_trader.adapters.interactive_brokers_pyo3 import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersV1LiveDataClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersV1LiveExecClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.config import MarketDataType
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.interactive_brokers import is_ib_endpoint_reachable
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint
from nautilus_trader.examples.strategies.subscribe import SubscribeStrategy
from nautilus_trader.examples.strategies.subscribe import SubscribeStrategyConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***

AUTO_STOP_DELAY_SECONDS = int(os.getenv("IB_PYO3_AUTO_STOP_SECONDS", "0"))
CONNECTION_TIMEOUT_SECONDS = int(os.getenv("IB_PYO3_CONNECTION_TIMEOUT_SECONDS", "10"))
NODE_CONNECTION_TIMEOUT_SECONDS = float(
    os.getenv(
        "IB_PYO3_NODE_CONNECTION_TIMEOUT_SECONDS",
        str(max(CONNECTION_TIMEOUT_SECONDS + 2, 5)),
    ),
)
DATA_CLIENT_ID = int(os.getenv("IB_PYO3_DATA_CLIENT_ID", "101"))
EXEC_CLIENT_ID = int(os.getenv("IB_PYO3_EXEC_CLIENT_ID", "102"))
IB_HOST, IB_PORT = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")
ENABLE_EXECUTION_CLIENT = os.getenv("IB_PYO3_ENABLE_EXECUTION", "0") == "1"
USE_EXISTING_CONNECTION = os.getenv(
    "IB_PYO3_FORCE_DOCKERIZED_GATEWAY",
    "0",
) != "1" and is_ib_endpoint_reachable(IB_HOST, IB_PORT)

ib_contracts = [
    IBContract(
        secType="STK",
        symbol="SPY",
        exchange="SMART",
        primaryExchange="ARCA",
        build_options_chain=True,
        min_expiry_days=7,
        max_expiry_days=14,
    ),
    IBContract(
        secType="CONTFUT",
        exchange="CME",
        symbol="ES",
        build_futures_chain=True,
    ),
    IBContract(secType="FUT", exchange="NYMEX", localSymbol="CLM6", build_futures_chain=False),
]


def resolve_gateway_config() -> DockerizedIBGatewayConfig | None:
    if USE_EXISTING_CONNECTION:
        return None

    username = os.getenv("TWS_USERNAME")
    password = os.getenv("TWS_PASSWORD")

    if not username or not password:
        print(
            "TWS_USERNAME/TWS_PASSWORD not set, falling back to the existing IB TWS/Gateway "
            f"at {IB_HOST}:{IB_PORT}",
        )
        return None

    return DockerizedIBGatewayConfig(
        username=username,
        password=password,
        trading_mode="paper",
        read_only_api=not ENABLE_EXECUTION_CLIENT,
    )


def schedule_auto_stop(node: TradingNode, delay_seconds: int) -> None:
    if delay_seconds <= 0:
        return

    def stop_after_delay() -> None:
        time.sleep(delay_seconds)
        os.kill(os.getpid(), signal.SIGINT)

    thread = threading.Thread(target=stop_after_delay, daemon=True)
    thread.start()


dockerized_gateway = resolve_gateway_config()
exec_account_id = os.getenv("TWS_ACCOUNT")

instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    build_futures_chain=False,
    build_options_chain=False,
    min_expiry_days=10,
    max_expiry_days=60,
    load_ids=frozenset(
        [
            "EUR/USD.IDEALPRO",
            "BTC/USD.PAXOS",
            "SPY.ARCA",
            "V.NYSE",
            "YMM6.CBOT",
            "CLM6.NYMEX",
            "ESM6.CME",
        ],
    ),
    load_contracts=frozenset(ib_contracts),
)

data_clients: dict[str, LiveDataClientConfig] = {
    IB: InteractiveBrokersDataClientConfig(
        ibg_host=IB_HOST,
        ibg_port=IB_PORT,
        ibg_client_id=DATA_CLIENT_ID,
        handle_revised_bars=False,
        use_regular_trading_hours=True,
        market_data_type=MarketDataType.DelayedFrozen,
        instrument_provider=instrument_provider,
        dockerized_gateway=dockerized_gateway,
        connection_timeout=CONNECTION_TIMEOUT_SECONDS,
    ),
}

exec_clients: dict[str, LiveExecClientConfig] = {}

if ENABLE_EXECUTION_CLIENT and exec_account_id is not None:
    exec_clients = {
        IB: InteractiveBrokersExecClientConfig(
            ibg_host=IB_HOST,
            ibg_port=IB_PORT,
            ibg_client_id=EXEC_CLIENT_ID,
            account_id=exec_account_id,
            dockerized_gateway=dockerized_gateway,
            instrument_provider=instrument_provider,
            routing=RoutingConfig(default=True),
            connection_timeout=CONNECTION_TIMEOUT_SECONDS,
        ),
    }
elif ENABLE_EXECUTION_CLIENT:
    print("IB_PYO3_ENABLE_EXECUTION=1 but TWS_ACCOUNT is not set, disabling execution client")
else:
    print(
        "IB_PYO3_ENABLE_EXECUTION is not enabled, starting the PyO3 example without an execution client",
    )

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id="TESTER-001",
    logging=LoggingConfig(log_level="INFO"),
    data_clients=data_clients,
    exec_clients=exec_clients,
    data_engine=LiveDataEngineConfig(
        time_bars_timestamp_on_close=False,  # Will use opening time as `ts_event` (same like IB)
        validate_data_sequence=True,  # Will discard any Bars received out of sequence
    ),
    timeout_connection=NODE_CONNECTION_TIMEOUT_SECONDS,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strategy_config = SubscribeStrategyConfig(
    instrument_id=InstrumentId.from_str("EUR/USD.IDEALPRO"),
    trade_ticks=False,
    quote_ticks=True,
)

# Instantiate your strategy
strategy = SubscribeStrategy(config=strategy_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(IB, InteractiveBrokersV1LiveDataClientFactory)

if exec_clients:
    node.add_exec_client_factory(IB, InteractiveBrokersV1LiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    schedule_auto_stop(node, AUTO_STOP_DELAY_SECONDS)
    try:
        node.run()
    finally:
        node.dispose()
