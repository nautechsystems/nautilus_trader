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

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.volatility_market_maker import VolatilityMarketMaker
from nautilus_trader.examples.strategies.volatility_market_maker import VolatilityMarketMakerConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="DEBUG",
        # log_level_file="DEBUG",
        # log_file_format="json",
        use_pyo3=True,
    ),
    # data_engine=LiveDataEngineConfig(debug=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    cache=CacheConfig(
        # database=DatabaseConfig(),
        timestamps_as_iso8601=True,
        buffer_interval_ms=100,
        flush_on_start=False,
    ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     encoding="json",
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    #     streams_prefix="quoters",
    #     use_instance_id=False,
    #     types_filter=[QuoteTick],
    #     autotrim_mins=30,
    #     heartbeat_interval_secs=1,
    # ),
    # streaming=StreamingConfig(catalog_path="catalog"),
    data_clients={
        "BINANCE_SPOT": BinanceDataClientConfig(
            api_key=None,  # 'BINANCE_API_KEY' env var
            api_secret=None,  # 'BINANCE_API_SECRET' env var
            account_type=BinanceAccountType.SPOT,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=False,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
        "BINANCE_FUTURES": BinanceDataClientConfig(
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
        "BINANCE_SPOT": BinanceExecClientConfig(
            api_key=None,  # 'BINANCE_API_KEY' env var
            api_secret=None,  # 'BINANCE_API_SECRET' env var
            account_type=BinanceAccountType.SPOT,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=False,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
            max_retries=3,
            retry_delay_initial_ms=1_000,
            retry_delay_max_ms=10_000,
        ),
        "BINANCE_FUTURES": BinanceExecClientConfig(
            api_key=None,  # 'BINANCE_API_KEY' env var
            api_secret=None,  # 'BINANCE_API_SECRET' env var
            account_type=BinanceAccountType.USDT_FUTURE,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=True,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
            max_retries=3,
            retry_delay_initial_ms=1_000,
            retry_delay_max_ms=10_000,
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

# Configure your strategies
spot_symbol = "ETHUSDT"
strat_config_spot = VolatilityMarketMakerConfig(
    instrument_id=InstrumentId.from_str(f"{spot_symbol}.BINANCE"),
    external_order_claims=[InstrumentId.from_str(f"{spot_symbol}.BINANCE")],
    bar_type=BarType.from_str(f"{spot_symbol}.BINANCE-1-MINUTE-LAST-INTERNAL"),
    atr_period=20,
    atr_multiple=6.0,
    trade_size=Decimal("0.010"),
    client_id=ClientId("BINANCE_SPOT"),
)

futures_symbol = "ETHUSDT-PERP"
strat_config_futures = VolatilityMarketMakerConfig(
    instrument_id=InstrumentId.from_str(f"{futures_symbol}.BINANCE"),
    external_order_claims=[InstrumentId.from_str(f"{futures_symbol}.BINANCE")],
    bar_type=BarType.from_str(f"{futures_symbol}.BINANCE-1-MINUTE-LAST-EXTERNAL"),
    atr_period=20,
    atr_multiple=6.0,
    trade_size=Decimal("0.010"),
    client_id=ClientId("BINANCE_FUTURES"),
)

# Instantiate your strategies
strategy_spot = VolatilityMarketMaker(config=strat_config_spot)
strategy_futures = VolatilityMarketMaker(config=strat_config_futures)

# Add your strategies and modules
node.trader.add_strategies([strategy_spot, strategy_futures])

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory("BINANCE_SPOT", BinanceLiveDataClientFactory)
node.add_exec_client_factory("BINANCE_SPOT", BinanceLiveExecClientFactory)
node.add_data_client_factory("BINANCE_FUTURES", BinanceLiveDataClientFactory)
node.add_exec_client_factory("BINANCE_FUTURES", BinanceLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
