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

from nautilus_trader.adapters.dydx.config import DYDXDataClientConfig
from nautilus_trader.adapters.dydx.config import DYDXExecClientConfig
from nautilus_trader.adapters.dydx.factories import DYDXLiveDataClientFactory
from nautilus_trader.adapters.dydx.factories import DYDXLiveExecClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.volatility_market_maker import VolatilityMarketMaker
from nautilus_trader.examples.strategies.volatility_market_maker import VolatilityMarketMakerConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***

# Perpetual
symbol = "ETH-USD-PERP"
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
    cache=CacheConfig(
        # database=DatabaseConfig(),
        timestamps_as_iso8601=True,
        buffer_interval_ms=100,
    ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    #     streams_prefix="bybit",
    #     use_trader_prefix=False,
    #     use_trader_id=False,
    #     use_instance_id=False,
    #     stream_per_topic=False,
    #     # types_filter=[QuoteTick],
    #     autotrim_mins=30,
    #     heartbeat_interval_secs=1,
    # ),
    data_clients={
        "DYDX": DYDXDataClientConfig(
            wallet_address=None,  # 'DYDX_WALLET_ADDRESS' env var
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=True,  # If client uses the testnet API
        ),
    },
    exec_clients={
        "DYDX": DYDXExecClientConfig(
            wallet_address=None,  # 'DYDX_WALLET_ADDRESS' env var
            mnemonic=None,  # 'DYDX_MNEMONIC' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=True,  # If client uses the testnet API
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = VolatilityMarketMakerConfig(
    instrument_id=InstrumentId.from_str(f"{symbol}.DYDX"),
    external_order_claims=[InstrumentId.from_str(f"{symbol}.DYDX")],
    bar_type=BarType.from_str(f"{symbol}.DYDX-1-MINUTE-LAST-EXTERNAL"),
    atr_period=20,
    atr_multiple=3.0,
    trade_size=trade_size,
)
# Instantiate your strategy
strategy = VolatilityMarketMaker(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory("DYDX", DYDXLiveDataClientFactory)
node.add_exec_client_factory("DYDX", DYDXLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
