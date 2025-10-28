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

from nautilus_trader.adapters.hyperliquid import HYPERLIQUID
from nautilus_trader.adapters.hyperliquid import HyperliquidDataClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveDataClientFactory
from nautilus_trader.adapters.hyperliquid import HyperliquidLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***

# *** HYPERLIQUID TESTNET SETUP ***
# To use Hyperliquid testnet, you need to:
# 1. Get testnet credentials from https://app.hyperliquid-testnet.xyz/
# 2. Set environment variables:
#    - HYPERLIQUID_TESTNET_PK: Your testnet private key (required)
#    - HYPERLIQUID_TESTNET_VAULT: Your testnet vault address (optional)
# 3. Ensure 'testnet=True' is set in both data and exec client configs below
# 4. Run the script: python hyperliquid_exec_tester.py

# Strategy config params
symbol = "BTC-USD-PERP"
instrument_id = InstrumentId.from_str(f"{symbol}.{HYPERLIQUID}")
order_qty = Decimal("0.001")  # Small test order for BTC

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        # log_level_file="DEBUG",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
        open_check_interval_secs=5.0,
        open_check_open_only=False,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
        purge_closed_orders_interval_mins=15,  # Example of purging closed orders for HFT
        purge_closed_orders_buffer_mins=60,  # Purged orders closed for at least an hour
        purge_closed_positions_interval_mins=15,  # Example of purging closed positions for HFT
        purge_closed_positions_buffer_mins=60,  # Purge positions closed for at least an hour
        purge_account_events_interval_mins=15,  # Example of purging account events for HFT
        purge_account_events_lookback_mins=60,  # Purge account events occurring more than an hour ago
        graceful_shutdown_on_exception=True,
    ),
    data_clients={
        HYPERLIQUID: HyperliquidDataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=True,  # If client uses the testnet
        ),
    },
    exec_clients={
        HYPERLIQUID: HyperliquidExecClientConfig(
            private_key=None,  # 'HYPERLIQUID_TESTNET_PK' env var for testnet
            vault_address=None,  # 'HYPERLIQUID_TESTNET_VAULT' env var (optional)
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=True,  # If client uses the testnet
        ),
    },
    timeout_connection=30.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = ExecTesterConfig(
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    # subscribe_quotes=True,
    # subscribe_trades=True,
    # subscribe_book=False,
    # enable_buys=True,
    # enable_sells=True,
    order_qty=order_qty,
    # open_position_on_start_qty=order_qty,
    # tob_offset_ticks=0,  # Ticks away from top of book (0 = at market)
    use_post_only=True,  # Use post-only orders to get maker fees
    # manage_gtd_expiry=False,
    # cancel_orders_on_stop=True,
    # close_positions_on_stop=True,  # Close all positions when stopping
    # reduce_only_on_stop=True,
    log_data=False,  # Set to True for verbose data logging
)

# Instantiate your strategy
strategy = ExecTester(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(HYPERLIQUID, HyperliquidLiveDataClientFactory)
node.add_exec_client_factory(HYPERLIQUID, HyperliquidLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
