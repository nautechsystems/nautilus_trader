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

from nautilus_trader.adapters.bitmex import BITMEX
from nautilus_trader.adapters.bitmex import BitmexDataClientConfig
from nautilus_trader.adapters.bitmex import BitmexExecClientConfig
from nautilus_trader.adapters.bitmex import BitmexLiveDataClientFactory
from nautilus_trader.adapters.bitmex import BitmexLiveExecClientFactory
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

# Example symbols for different BitMEX products
# Perpetual swap: XBTUSD (Bitcoin perpetual)
# Futures: XBTZ25 (Bitcoin futures expiring December 2025)
# Alt perpetuals: ETHUSD, SOLUSD, etc.

testnet = True  # If clients use the testnet API
symbol = "XBTUSD"  # Bitcoin perpetual swap
order_qty = Decimal("100")  # Contract size in USD

# symbol = "SOLUSDT"  # Solana quoted in USDT spot
# order_qty = Decimal("0.1")  # Fractional size

instrument_id = InstrumentId.from_str(f"{symbol}.{BITMEX}")

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_instrument_ids=[instrument_id],  # Only reconcile this instrument
        open_check_interval_secs=5.0,
        open_check_open_only=False,
        manage_own_order_books=True,
        own_books_audit_interval_secs=1.0,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    data_clients={
        BITMEX: BitmexDataClientConfig(
            api_key=None,  # 'BITMEX_API_KEY' env var
            api_secret=None,  # 'BITMEX_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=testnet,  # If client uses the testnet API
        ),
    },
    exec_clients={
        BITMEX: BitmexExecClientConfig(
            api_key=None,  # 'BITMEX_API_KEY' env var
            api_secret=None,  # 'BITMEX_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            testnet=testnet,  # If client uses the testnet API
        ),
    },
    timeout_connection=10.0,
    timeout_reconciliation=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
    timeout_shutdown=2.0,
)

# Configure the execution tester strategy
config_tester = ExecTesterConfig(
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    order_qty=order_qty,
    # order_display_qty=Decimal(0),  # Must be zero (hidden) or a positive multiple of lot size 100
    # subscribe_book=True,
    enable_buys=True,
    enable_sells=True,
    use_post_only=True,
    tob_offset_ticks=0,
    # modify_orders_to_maintain_tob_offset=True,
    open_position_on_start_qty=order_qty,
    open_position_time_in_force=TimeInForce.IOC,  # Market orders must be IOC
    close_positions_time_in_force=TimeInForce.IOC,  # Market orders must be IOC
    # enable_stop_buys=True,
    # enable_stop_sells=True,
    # stop_order_type=OrderType.STOP_MARKET,
    # stop_trigger_type=TriggerType.MARK_PRICE,
    # enable_brackets=True,
    # test_reject_post_only=True,
    # cancel_orders_on_stop=False,
    # close_positions_on_stop=False,
    # use_batch_cancel_on_stop=True,
    # use_individual_cancels_on_stop=True,
    log_data=False,
    # dry_run=True,
)
tester = ExecTester(config=config_tester)

# Setup and run the trading node
node = TradingNode(config=config_node)

# Add the strategy to the node
node.trader.add_strategy(tester)

# Register the client factories
node.add_data_client_factory(BITMEX, BitmexLiveDataClientFactory)
node.add_exec_client_factory(BITMEX, BitmexLiveExecClientFactory)
node.build()

# Run the node
try:
    node.run()
except KeyboardInterrupt:
    node.stop()
finally:
    node.dispose()
