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

from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.config import BybitDataClientConfig
from nautilus_trader.adapters.bybit.config import BybitExecClientConfig
from nautilus_trader.adapters.bybit.factories import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit.factories import BybitLiveExecClientFactory
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

# SPOT/LINEAR
product_type = BybitProductType.LINEAR
symbol = f"ETHUSDT-{product_type.value.upper()}"
trade_size = Decimal("0.010")

# INVERSE
# product_type = BybitProductType.INVERSE
# symbol = f"XRPUSD-{product_type.value.upper()}"  # Use for inverse
# trade_size = Decimal("100")  # Use for inverse

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
        reconciliation=True,
        open_check_interval_secs=5.0,
        open_check_open_only=True,
        # own_books_audit_interval_secs=2.0,
        # manage_own_order_books=True,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
        purge_closed_orders_interval_mins=15,  # Example of purging closed orders for HFT
        purge_closed_orders_buffer_mins=60,  # Purged orders closed for at least an hour
        purge_closed_positions_interval_mins=15,  # Example of purging closed positions for HFT
        purge_closed_positions_buffer_mins=60,  # Purge positions closed for at least an hour
        purge_account_events_interval_mins=15,  # Example of purging account events for HFT
        purge_account_events_lookback_mins=60,  # Purge account events occurring more than an hour ago
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
    #     types_filter=[QuoteTick],
    #     autotrim_mins=30,
    #     heartbeat_interval_secs=1,
    # ),
    data_clients={
        "BYBIT": BybitDataClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=[product_type],  # Will load all instruments
            demo=False,  # If client uses the demo API
            testnet=False,  # If client uses the testnet API
            recv_window_ms=5_000,  # Default
        ),
    },
    exec_clients={
        "BYBIT": BybitExecClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws_private=None,  # Override with custom endpoint
            use_ws_trade_api=True,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=[product_type],
            demo=False,  # If client uses the demo API
            testnet=False,  # If client uses the testnet API
            max_retries=3,
            retry_delay=1.0,
            recv_window_ms=5_000,  # Default
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
    instrument_id=InstrumentId.from_str(f"{symbol}.BYBIT"),
    external_order_claims=[InstrumentId.from_str(f"{symbol}.BYBIT")],
    bar_type=BarType.from_str(f"{symbol}.BYBIT-1-MINUTE-LAST-EXTERNAL"),
    atr_period=20,
    atr_multiple=3.0,
    trade_size=trade_size,
)
# Instantiate your strategy
strategy = VolatilityMarketMaker(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory("BYBIT", BybitLiveDataClientFactory)
node.add_exec_client_factory("BYBIT", BybitLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
