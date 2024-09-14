#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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


from datetime import datetime

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.config import ImportableControllerConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO"),
    exec_engine=LiveExecEngineConfig(
        # debug=True,
        reconciliation=True,
        reconciliation_lookback_mins=1440,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    cache=CacheConfig(
        # database=DatabaseConfig(timeout=2),
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
    # ),
    controller=ImportableControllerConfig(
        controller_path="nautilus_trader.examples.strategies.simple_instrument_selector_controller:BinanceFutureInstrumentSelectorController",
        config_path="nautilus_trader.examples.strategies.simple_instrument_selector_controller:BinanceFutureInstrumentSelectorControllerConfig",
        config={
            "interval_secs": 3600,
            "min_notional_threshold": 6,
            "quote_asset": "USDT",
            "onboard_date_filter_type": "range",
            "onboard_date_reference_date": datetime(2024, 1, 1),
            "onboard_date_end_date": datetime(2024, 6, 1),
        },
    ),
    data_clients={
        "BINANCE": BinanceDataClientConfig(
            api_key=None,  # 'BINANCE_API_KEY' env var
            api_secret=None,  # 'BINANCE_API_SECRET' env var
            account_type=BinanceAccountType.USDT_FUTURE,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=True,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        "BINANCE": BinanceExecClientConfig(
            api_key=None,  # 'BINANCE_API_KEY' env var
            api_secret=None,  # 'BINANCE_API_SECRET' env var
            account_type=BinanceAccountType.USDT_FUTURE,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=True,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
            use_position_ids=False,
            max_retries=3,
            retry_delay=1.0,
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

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory("BINANCE", BinanceLiveDataClientFactory)
node.add_exec_client_factory("BINANCE", BinanceLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
