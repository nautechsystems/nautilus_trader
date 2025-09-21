#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitExecClientConfig
from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit import BybitLiveExecClientFactory
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LiveRiskEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.algorithms.twap import TWAPExecAlgorithm
from nautilus_trader.examples.strategies.ema_cross_bracket_algo import EMACrossBracketAlgo
from nautilus_trader.examples.strategies.ema_cross_bracket_algo import EMACrossBracketAlgoConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# SPOT/LINEAR
product_type = BybitProductType.LINEAR
symbol = f"ETHUSDT-{product_type.value.upper()}"
trade_size = Decimal("0.010")

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    risk_engine=LiveRiskEngineConfig(debug=True),
    # cache=CacheConfig(
    #     database=DatabaseConfig(),
    #     buffer_interval_ms=100,
    # ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     streams_prefix="quoters",
    #     use_instance_id=False,
    #     timestamps_as_iso8601=True,
    #     # types_filter=[QuoteTick],
    #     autotrim_mins=1,
    #     heartbeat_interval_secs=1,
    # ),
    data_clients={
        BYBIT: BybitDataClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=[product_type],  # Will load all instruments
            testnet=False,  # If client uses the testnet
        ),
    },
    exec_clients={
        BYBIT: BybitExecClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws_private=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=[product_type],
            testnet=False,  # If client uses the testnet
            max_retries=3,
            retry_delay_initial_ms=1_000,
            retry_delay_max_ms=10_000,
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=3.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = EMACrossBracketAlgoConfig(
    order_id_tag="001",
    instrument_id=InstrumentId.from_str(f"{symbol}.BYBIT"),
    external_order_claims=[InstrumentId.from_str(f"{symbol}.BYBIT")],
    bar_type=BarType.from_str(f"{symbol}.BYBIT-1-MINUTE-LAST-EXTERNAL"),
    fast_ema_period=10,
    slow_ema_period=20,
    bracket_distance_atr=1.0,
    trade_size=trade_size,
)

# Instantiate your strategy and execution algorithm
strategy = EMACrossBracketAlgo(config=strat_config)
exec_algorithm = TWAPExecAlgorithm()

# Add your strategy and execution algorithm and modules
node.trader.add_strategy(strategy)
node.trader.add_exec_algorithm(exec_algorithm)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)
node.add_exec_client_factory(BYBIT, BybitLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
