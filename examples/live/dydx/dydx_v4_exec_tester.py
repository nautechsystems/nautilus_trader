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
DYdX v4 ExecTester example using the Rust-backed adapter.

This script demonstrates how to use the ExecTester strategy to validate
execution functionality for the dYdX v4 adapter.

Prerequisites:
  - Environment variables:
      DYDX_WALLET_ADDRESS (or DYDX_TESTNET_WALLET_ADDRESS for testnet)
      DYDX_MNEMONIC (or DYDX_TESTNET_MNEMONIC for testnet)

dYdX v4 order semantics:
  - Short-term orders: Live only in validator memory (~20 blocks / ~30 seconds)
  - Long-term orders: Required for conditional orders (STOP_MARKET, STOP_LIMIT)
  - All markets are perpetual futures (-PERP suffix)
  - Uses DYDXOrderTags to control short-term vs long-term, block lifetime, etc.

Note on DYDXOrderTags:
  The ExecTester uses short-term orders by default. For custom tag configuration
  (e.g., long-term orders for stop orders), see the dydx_v4_market_maker.py example
  which demonstrates passing tags via the order_factory.limit() method:

    order = self.order_factory.limit(
        ...
        tags=[DYDXOrderTags(is_short_term_order=False).value],
    )

Usage:
  python dydx_v4_exec_tester.py

"""

from decimal import Decimal

from nautilus_trader.adapters.dydx_v4 import DYDX_VENUE
from nautilus_trader.adapters.dydx_v4 import DYDXv4DataClientConfig
from nautilus_trader.adapters.dydx_v4 import DYDXv4ExecClientConfig
from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveDataClientFactory
from nautilus_trader.adapters.dydx_v4 import DYDXv4LiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.portfolio.config import PortfolioConfig
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# dYdX v4 perpetual markets
# All instruments follow {BASE}-{QUOTE}-PERP.DYDX naming
symbol = "ETH-USD-PERP"
instrument_id = InstrumentId.from_str(f"{symbol}.{DYDX_VENUE}")

# Order parameters
order_qty = Decimal("0.01")

# Only reconcile these instruments
reconciliation_instrument_ids = [instrument_id]

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("DYDX-EXEC-TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,  # 24 hours
        reconciliation_instrument_ids=reconciliation_instrument_ids,
        open_check_interval_secs=5.0,
        open_check_open_only=False,
        position_check_interval_secs=5.0,
        graceful_shutdown_on_exception=True,
    ),
    risk_engine=LiveRiskEngineConfig(bypass=True),
    portfolio=PortfolioConfig(min_account_state_logging_interval_ms=1_000),
    data_clients={
        "DYDX": DYDXv4DataClientConfig(
            wallet_address=None,  # 'DYDX_WALLET_ADDRESS' or 'DYDX_TESTNET_WALLET_ADDRESS' env var
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=frozenset(reconciliation_instrument_ids),
            ),
            is_testnet=False,  # Mainnet by default; flip to True for testnet
        ),
    },
    exec_clients={
        "DYDX": DYDXv4ExecClientConfig(
            wallet_address=None,  # 'DYDX_WALLET_ADDRESS' or 'DYDX_TESTNET_WALLET_ADDRESS' env var
            mnemonic=None,  # 'DYDX_MNEMONIC' or 'DYDX_TESTNET_MNEMONIC' env var
            subaccount=0,  # Default subaccount (created after first deposit/trade)
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            base_url_grpc=None,  # Override with custom gRPC endpoint
            instrument_provider=InstrumentProviderConfig(
                load_all=False,
                load_ids=frozenset(reconciliation_instrument_ids),
            ),
            is_testnet=False,  # Mainnet by default; flip to True for testnet
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

# Configure your execution tester
# Note: dYdX v4 does NOT support:
#   - Batch submit/modify/cancel
#   - OCO, iceberg, or bracket orders (emulated only)
#   - Trailing stop orders
#   - MKT_IF_TOUCHED, LMT_IF_TOUCHED (only STOP_MARKET, STOP_LIMIT)
config_tester = ExecTesterConfig(
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    subscribe_quotes=True,
    subscribe_trades=True,
    enable_limit_buys=True,
    enable_limit_sells=True,
    enable_stop_buys=False,  # Stop orders require long-term orders with DYDXOrderTags
    enable_stop_sells=False,  # Stop orders require long-term orders with DYDXOrderTags
    order_qty=order_qty,
    tob_offset_ticks=500,  # Definitely out of the market
    use_post_only=True,  # dYdX supports post-only for maker orders
    reduce_only_on_stop=True,  # dYdX supports reduce-only
    cancel_orders_on_stop=True,
    close_positions_on_stop=True,
    log_data=False,
)

# Instantiate your strategy
tester = ExecTester(config=config_tester)

# Add your strategies and modules
node.trader.add_strategy(tester)

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
