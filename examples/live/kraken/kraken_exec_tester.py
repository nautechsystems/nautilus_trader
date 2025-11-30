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

from nautilus_trader.adapters.kraken import KRAKEN
from nautilus_trader.adapters.kraken import KrakenDataClientConfig
from nautilus_trader.adapters.kraken import KrakenEnvironment
from nautilus_trader.adapters.kraken import KrakenExecClientConfig
from nautilus_trader.adapters.kraken import KrakenLiveDataClientFactory
from nautilus_trader.adapters.kraken import KrakenLiveExecClientFactory
from nautilus_trader.adapters.kraken import KrakenProductType
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

# NOTE: This example uses Kraken Futures demo environment for testing
# Set up your demo account at https://demo-futures.kraken.com
# and configure environment variables:
# - KRAKEN_TESTNET_API_KEY
# - KRAKEN_TESTNET_API_SECRET

# Strategy config params
# Kraken Futures perpetual symbols use PI_ prefix (e.g., PI_XBTUSD, PI_ETHUSD)
symbol = "ETH/USD"  # Spot pair
# symbol = "PI_XBTUSD"  # BTC inverse perpetual futures
instrument_id = InstrumentId.from_str(f"{symbol}.{KRAKEN}")
# order_qty = Decimal(10)
order_qty = Decimal("0.001")

environment = KrakenEnvironment.MAINNET
# product_types = (KrakenProductType.SPOT, KrakenProductType.FUTURES)
product_types = (KrakenProductType.SPOT,)

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(
        log_level="INFO",
        log_level_file="DEBUG",
        log_colors=True,
        use_pyo3=True,
    ),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        open_check_interval_secs=5.0,
        open_check_open_only=False,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
        purge_closed_orders_interval_mins=1,
        purge_closed_orders_buffer_mins=0,
        purge_closed_positions_interval_mins=1,
        purge_closed_positions_buffer_mins=0,
        purge_account_events_interval_mins=1,
        purge_account_events_lookback_mins=0,
        purge_from_database=False,
        graceful_shutdown_on_exception=True,
    ),
    data_clients={
        KRAKEN: KrakenDataClientConfig(
            api_key=None,  # 'KRAKEN_API_KEY' env var
            api_secret=None,  # 'KRAKEN_API_SECRET' env var
            environment=environment,
            product_types=product_types,
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        KRAKEN: KrakenExecClientConfig(
            api_key=None,  # 'KRAKEN_API_KEY' env var
            api_secret=None,  # 'KRAKEN_API_SECRET' env var
            environment=environment,
            product_types=product_types,
            instrument_provider=InstrumentProviderConfig(load_all=True),
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
    enable_limit_buys=True,
    enable_limit_sells=True,
    # open_position_on_start_qty=order_qty,
    # tob_offset_ticks=0,
    # use_batch_cancel_on_stop=True,
    # use_individual_cancels_on_stop=True,
    use_post_only=True,
    # close_positions_on_stop=False,
    log_data=True,
)

# Instantiate your strategy
strategy = ExecTester(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node
node.add_data_client_factory(KRAKEN, KrakenLiveDataClientFactory)
node.add_exec_client_factory(KRAKEN, KrakenLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
