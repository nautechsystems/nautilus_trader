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

from decimal import Decimal

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.config import CacheConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from high_frequency_grid_trading import HighFrequencyGridTrading
from high_frequency_grid_trading import HighFrequencyGridTradingConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
import os


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
apiKey = os.getenv('API_KEY', 'Unknown')
apiSecret = os.getenv('API_SECRET', 'Unknown')
timeoutConnection = float(os.getenv('CONNECTION_TIME_OUT', 30))
timeoutReconciliation = float(os.getenv('RECONCILIATION_TIME_OUT', 10))
timeoutPortfolio = float(os.getenv('PORTFOLIO_TIME_OUT', 10))
timeoutDisconnection = float(os.getenv('DISCONNECTION_TIME_OUT', 10))
timeoutPostStop = float(os.getenv('POST_STOP_TIME_OUT', 5))

instrumentId = os.getenv('INSTRUMENT_ID', 'BNBUSDT-PERP.BINANCE')
maxTradeSize = os.getenv('MAX_TRADE_SIZE', '0.1')

inflightCheckRetries=int(os.getenv('INFLIGHT_CHECK_RETRY', 5))

# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("Grid-Trading"),
    logging=LoggingConfig(
        log_level="INFO",
        # log_level_file="DEBUG",
        # log_file_format="json",
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        reconciliation_lookback_mins=1440,
        filter_position_reports=True,
        inflight_check_interval_ms=2_000,
        inflight_check_threshold_ms=5_000,
        inflight_check_retries=inflightCheckRetries,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    
    cache=CacheConfig(
        database=None,
        timestamps_as_iso8601=True,
        flush_on_start=False,
    ),
    data_clients={
        "BINANCE": BinanceDataClientConfig(
            api_key=apiKey,
            api_secret=apiSecret,
            account_type=BinanceAccountType.USDT_FUTURE,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=False,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
            use_agg_trade_ticks=True
        ),
    },
    exec_clients={
        "BINANCE": BinanceExecClientConfig(
            api_key=apiKey,
            api_secret=apiSecret,
            account_type=BinanceAccountType.USDT_FUTURE,
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            us=False,  # If client is for Binance US
            testnet=False,  # If client uses the testnet
            instrument_provider=InstrumentProviderConfig(load_all=True),
            max_retries=3,
            retry_delay=1.0,
        ),
    },
    timeout_connection=timeoutConnection,
    timeout_reconciliation=timeoutReconciliation,
    timeout_portfolio=timeoutPortfolio,
    timeout_disconnection=timeoutDisconnection,
    timeout_post_stop=timeoutPostStop,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = HighFrequencyGridTradingConfig(
    instrument_id=InstrumentId.from_str(instrumentId),
    external_order_claims=[InstrumentId.from_str(instrumentId)],
    max_trade_size=Decimal(maxTradeSize),
)

# Instantiate your strategy
strategy = HighFrequencyGridTrading(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

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
