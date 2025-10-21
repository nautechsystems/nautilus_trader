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

from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceAccountType
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceExecClientConfig
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance import BinanceLiveExecClientFactory
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Strategy config params
symbol = "ETHUSDT-PERP"
instrument_id = InstrumentId.from_str(f"{symbol}.{BINANCE}")
order_qty = Decimal("0.02")

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        # log_level_file="DEBUG",
        # log_file_format="json",
        log_colors=True,
        use_pyo3=True,
    ),
    data_engine=LiveDataEngineConfig(
        external_clients=[ClientId(BINANCE)],
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        open_check_interval_secs=5.0,
        open_check_open_only=False,
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
    cache=CacheConfig(
        # database=DatabaseConfig(),
        timestamps_as_iso8601=True,
        flush_on_start=False,
    ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(timeout=2),
    #     timestamps_as_iso8601=True,
    #     use_instance_id=False,
    #     # types_filter=[QuoteTick],
    #     stream_per_topic=False,
    #     external_streams=["bybit"],
    #     autotrim_mins=30,
    #     heartbeat_interval_secs=1,
    # ),
    # streaming=StreamingConfig(catalog_path="catalog"),
    data_clients={
        BINANCE: BinanceDataClientConfig(
            api_key=None,  # 'BINANCE_API_KEY' env var
            api_secret=None,  # 'BINANCE_API_SECRET' env var
            account_type=BinanceAccountType.USDT_FUTURES,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=True,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        BINANCE: BinanceExecClientConfig(
            api_key=None,  # 'BINANCE_API_KEY' env var
            api_secret=None,  # 'BINANCE_API_SECRET' env var
            account_type=BinanceAccountType.USDT_FUTURES,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=True,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
            max_retries=3,
            retry_delay_initial_ms=1_000,
            retry_delay_max_ms=10_000,
            log_rejected_due_post_only_as_warning=False,
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
    # subscribe_book=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    order_qty=order_qty,
    # order_params={"price_match": "QUEUE_5"},
    # enable_buys=False,
    # enable_sells=False,
    open_position_on_start_qty=order_qty,
    # tob_offset_ticks=0,
    # use_batch_cancel_on_stop=True,
    # use_individual_cancels_on_stop=True,
    use_post_only=True,
    # close_positions_on_stop=False,
    # log_rejected_due_post_only_as_warning=False,
    # test_reject_post_only=True,
    log_data=False,
)

# Instantiate your strategy
strategy = ExecTester(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
node.add_exec_client_factory(BINANCE, BinanceLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
