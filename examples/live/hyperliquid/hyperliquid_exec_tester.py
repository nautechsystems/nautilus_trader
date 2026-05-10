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

from decimal import Decimal
from enum import Enum

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
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


# Environment variables required:
# Mainnet: HYPERLIQUID_PK (and optionally HYPERLIQUID_VAULT)
# Testnet: HYPERLIQUID_TESTNET_PK (and optionally HYPERLIQUID_TESTNET_VAULT)


class HyperliquidProductType(Enum):
    SPOT = "SPOT"
    PERP = "PERP"


# Configuration - Change product_type to switch between trading modes
product_type = HyperliquidProductType.PERP  # SPOT or PERP
testnet = False  # Set to True for testnet, False for mainnet

# Symbol and settings based on product type
if product_type == HyperliquidProductType.SPOT:
    symbol = "HYPE-USDC-SPOT"
    order_qty = Decimal("0.5")  # 1 HYPE (minimum size)
    enable_sells = False  # May not own HYPE when starting fresh
    reduce_only_on_stop = False  # Not applicable on spot
elif product_type == HyperliquidProductType.PERP:
    symbol = "BTC-USD-PERP"
    order_qty = Decimal("0.001")  # 0.001 BTC (minimum size)
    enable_sells = True
    reduce_only_on_stop = True
else:
    raise ValueError(f"Unsupported product type: {product_type}")

instrument_id = InstrumentId.from_str(f"{symbol}.{HYPERLIQUID}")


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
        open_check_interval_secs=15.0,
        open_check_threshold_ms=10_000,
        open_check_open_only=False,
        open_check_lookback_mins=60,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
        purge_closed_orders_interval_mins=15,
        purge_closed_orders_buffer_mins=60,
        purge_closed_positions_interval_mins=15,
        purge_closed_positions_buffer_mins=60,
        purge_account_events_interval_mins=15,
        purge_account_events_lookback_mins=60,
        graceful_shutdown_on_exception=True,
    ),
    # cache=CacheConfig(
    #     database=DatabaseConfig(),
    #     timestamps_as_iso8601=True,
    #     persist_account_events=False,  # Useful for HFT ops where this can quickly accumulate
    #     buffer_interval_ms=100,
    # ),
    data_clients={
        HYPERLIQUID: HyperliquidDataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=testnet,
        ),
    },
    exec_clients={
        HYPERLIQUID: HyperliquidExecClientConfig(
            private_key=None,  # Loaded from env var based on testnet setting
            vault_address=None,  # Optional, loaded from env var
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=testnet,
            normalize_prices=True,  # Rounds prices to 5 significant figures (required for HL)
        ),
    },
    timeout_connection=30.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=10.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = ExecTesterConfig(
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    # subscribe_quotes=True,
    # subscribe_trades=True,
    # subscribe_book=True,
    order_qty=order_qty,
    open_position_on_start_qty=order_qty,
    open_position_time_in_force=TimeInForce.IOC,
    enable_limit_buys=True,
    enable_limit_sells=enable_sells,
    # enable_limit_buys=False,
    # enable_limit_sells=False,
    # enable_stop_buys=True,
    # enable_stop_sells=True,
    # tob_offset_ticks=0,  # Ticks away from top of book (0 = at market)
    use_post_only=True,  # Use post-only orders to get maker fees
    # modify_orders_to_maintain_tob_offset=True,
    # use_individual_cancels_on_stop=True,
    reduce_only_on_stop=reduce_only_on_stop,
    # cancel_orders_on_stop=False,
    # close_positions_on_stop=False,
    manage_stop=True,
    market_exit_reduce_only=reduce_only_on_stop,
    # test_reject_post_only=True,
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
