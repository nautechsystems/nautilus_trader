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
"""
DYdX v4 Market Maker example using the Rust-backed adapter.

This example demonstrates the volatility market maker strategy on dYdX v4
using the new Rust-backed HTTP, WebSocket, and gRPC clients.

Prerequisites:
  - Environment variables:
      DYDX_WALLET_ADDRESS (or DYDX_TESTNET_WALLET_ADDRESS for testnet)
      DYDX_MNEMONIC (or DYDX_TESTNET_MNEMONIC for testnet)

Usage:
  python dydx_v4_market_maker.py

"""

from decimal import Decimal

from nautilus_trader.adapters.dydx_v4 import DYDXv4DataClientConfig
from nautilus_trader.adapters.dydx_v4 import DYDXv4ExecClientConfig
from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveDataClientFactory
from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveExecClientFactory
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

# dYdX v4 perpetual market
symbol = "ETH-USD-PERP"
trade_size = Decimal("0.010")

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("DYDX-V4-MM-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
    ),
    cache=CacheConfig(
        timestamps_as_iso8601=True,
        buffer_interval_ms=100,
    ),
    data_clients={
        "DYDX": DYDXv4DataClientConfig(
            wallet_address=None,  # 'DYDX_WALLET_ADDRESS' or 'DYDX_TESTNET_WALLET_ADDRESS' env var
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,  # Mainnet
        ),
    },
    exec_clients={
        "DYDX": DYDXv4ExecClientConfig(
            wallet_address=None,  # 'DYDX_WALLET_ADDRESS' or 'DYDX_TESTNET_WALLET_ADDRESS' env var
            mnemonic=None,  # 'DYDX_MNEMONIC' or 'DYDX_TESTNET_MNEMONIC' env var
            subaccount=0,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            base_url_grpc=None,  # Override with custom gRPC endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            is_testnet=False,  # Mainnet
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

# Register your client factories with the node (using v4 Rust-backed factories)
node.add_data_client_factory("DYDX", DYDXv4LiveDataClientFactory)
node.add_exec_client_factory("DYDX", DYDXv4LiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
