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

from nautilus_trader.adapters.polymarket import POLYMARKET
from nautilus_trader.adapters.polymarket import PolymarketDataClientConfig
from nautilus_trader.adapters.polymarket import PolymarketExecClientConfig
from nautilus_trader.adapters.polymarket import PolymarketLiveDataClientFactory
from nautilus_trader.adapters.polymarket import PolymarketLiveExecClientFactory
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.portfolio.config import PortfolioConfig
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# For correct subscription operation, you must specify all instruments to be immediately
# subscribed for as part of the data client configuration

# Note on pagination: The py_clob_client library handles pagination internally
# for both get_orders() and get_trades() methods. It automatically fetches all
# pages and returns the complete dataset, so no manual pagination is needed.

# To find active markets run `python nautilus_trader/adapters/polymarket/scripts/active_markets.py`

# Slug: fed-rate-hike-in-2025
# Active: True
# Condition ID: 0x4319532e181605cb15b1bd677759a3bc7f7394b2fdf145195b700eeaedfd5221
# Token IDs: 60487116984468020978247225474488676749601001829886755968952521846780452448915,
# 81104637750588840860328515305303028259865221573278091453716127842023614249200
# Link: https://polymarket.com/event/fed-rate-hike-in-2025
condition_id = "0x4319532e181605cb15b1bd677759a3bc7f7394b2fdf145195b700eeaedfd5221"
token_id = "60487116984468020978247225474488676749601001829886755968952521846780452448915"

instrument_id = get_polymarket_instrument_id(condition_id, token_id)

# Configure instrument provider to only load the specific instrument we're testing
# This avoids walking the entire Polymarket market space unnecessarily
load_ids = [str(instrument_id)]
instrument_provider_config = InstrumentProviderConfig(load_ids=frozenset(load_ids))

# Order configuration
order_qty = Decimal("10")  # Number of shares for limit orders, or notional value for market BUY

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        # log_level_file="DEBUG",
        # log_file_max_size=1_000_000_000,
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        convert_quote_qty_to_base=False,  # Required for submitting market BUY orders
        reconciliation=True,
        reconciliation_instrument_ids=[instrument_id],  # Only reconcile these instruments
        open_check_interval_secs=5.0,
        open_check_open_only=True,
        # own_books_audit_interval_secs=2.0,
        # manage_own_order_books=True,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
        purge_closed_orders_interval_mins=1,  # Example of purging closed orders for HFT
        purge_closed_orders_buffer_mins=0,  # Purged orders closed for at least an hour
        purge_closed_positions_interval_mins=1,  # Example of purging closed positions for HFT
        purge_closed_positions_buffer_mins=0,  # Purge positions closed for at least an hour
        purge_account_events_interval_mins=1,  # Example of purging account events for HFT
        purge_account_events_lookback_mins=0,  # Purge account events occurring more than an hour ago
        purge_from_database=True,  # Set True with caution
        graceful_shutdown_on_exception=True,
    ),
    # cache=CacheConfig(
    #     database=DatabaseConfig(),
    #     timestamps_as_iso8601=True,
    #     persist_account_events=False,  # Useful for HFT ops where this can quickly accumulate
    #     buffer_interval_ms=100,
    # ),
    portfolio=PortfolioConfig(min_account_state_logging_interval_ms=1_000),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    #     streams_prefix="polymarket",
    #     use_trader_prefix=False,
    #     use_trader_id=False,
    #     use_instance_id=False,
    #     stream_per_topic=False,
    #     types_filter=[QuoteTick],
    #     autotrim_mins=30,
    #     heartbeat_interval_secs=1,
    # ),
    data_clients={
        POLYMARKET: PolymarketDataClientConfig(
            api_key=None,  # 'POLYMARKET_API_KEY' env var
            api_secret=None,  # 'POLYMARKET_API_SECRET' env var
            passphrase=None,  # 'POLYMARKET_PASSPHRASE' env var
            # signature_type=2,  # Use if trading via Polymarket Proxy (enables UI verification, requires funder address)
            base_url_http=None,  # Override with custom endpoint
            instrument_provider=instrument_provider_config,
        ),
    },
    exec_clients={
        POLYMARKET: PolymarketExecClientConfig(
            api_key=None,  # 'POLYMARKET_API_KEY' env var
            api_secret=None,  # 'POLYMARKET_API_SECRET' env var
            passphrase=None,  # 'POLYMARKET_PASSPHRASE' env var
            # signature_type=2,  # Use if trading via Polymarket Proxy (enables UI verification, requires funder address)
            base_url_http=None,  # Override with custom endpoint
            instrument_provider=instrument_provider_config,
            generate_order_history_from_trades=False,
            max_retries=3,
            retry_delay_initial_ms=1_000,
            retry_delay_max_ms=10_000,
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
config_tester = ExecTesterConfig(
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    subscribe_quotes=True,
    subscribe_trades=True,
    # subscribe_book=True,
    # enable_buys=False,
    enable_sells=False,
    tob_offset_ticks=10,
    order_qty=order_qty,
    # open_position_on_start_qty=order_qty,
    # use_quote_quantity=True,  # Required for submitting market BUY orders
    use_post_only=False,  # Polymarket does not support post-only orders
    # test_reject_post_only=True,
    reduce_only_on_stop=False,  # Polymarket does not support reduce-only orders
    cancel_orders_on_stop=True,
    close_positions_on_stop=True,
    log_data=False,
    log_rejected_due_post_only_as_warning=False,
    can_unsubscribe=False,  # Polymarket does not support unsubscribing from ws streams
)

# Instantiate your strategy
tester = ExecTester(config=config_tester)

# Add your strategies and modules
node.trader.add_strategy(tester)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(POLYMARKET, PolymarketLiveDataClientFactory)
node.add_exec_client_factory(POLYMARKET, PolymarketLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
