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
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Configuration - Change product_type to switch between trading modes
product_type = KrakenProductType.FUTURES  # SPOT or FUTURES
token = "ETH"

# Symbol and settings based on product type
if product_type == KrakenProductType.SPOT:
    symbol = f"{token}/USDT"
    order_qty = Decimal("0.001")
    enable_sells = False  # May not own base token when starting fresh
    reduce_only_on_stop = False  # Not supported on spot
    use_spot_position_reports = True
    environment = KrakenEnvironment.MAINNET
elif product_type == KrakenProductType.FUTURES:
    # Kraken Futures perpetual symbols use PI_ prefix (e.g., PI_XBTUSD, PI_ETHUSD)
    symbol = f"PF_{token}USD"
    order_qty = Decimal("0.001")
    enable_sells = True
    reduce_only_on_stop = True
    use_spot_position_reports = False  # Not applicable
    environment = KrakenEnvironment.MAINNET
    # environment = KrakenEnvironment.DEMO  # Use demo-futures.kraken.com
else:
    raise ValueError(f"Unsupported product type: {product_type}")

instrument_id = InstrumentId.from_str(f"{symbol}.{KRAKEN}")
product_types = (product_type,)

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
        position_check_interval_secs=30.0,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
        # purge_closed_orders_interval_mins=1,
        # purge_closed_orders_buffer_mins=0,
        # purge_closed_positions_interval_mins=1,
        # purge_closed_positions_buffer_mins=0,
        # purge_account_events_interval_mins=1,
        # purge_account_events_lookback_mins=0,
        # purge_from_database=False,
        # graceful_shutdown_on_exception=True,
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
            use_spot_position_reports=use_spot_position_reports,
            spot_positions_quote_currency="USDT",
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
    use_uuid_client_order_ids=True,
    # subscribe_book=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    order_qty=order_qty,
    open_position_on_start_qty=order_qty,
    open_position_time_in_force=TimeInForce.IOC,
    # order_expire_time_delta_mins=1,
    enable_limit_buys=True,
    enable_limit_sells=enable_sells,
    # enable_stop_buys=True,
    # enable_stop_sells=enable_sells,
    # tob_offset_ticks=0,
    # stop_order_type=OrderType.STOP_LIMIT,
    # stop_trigger_type=TriggerType.LAST_PRICE,
    # modify_orders_to_maintain_tob_offset=True,
    # modify_stop_orders_to_maintain_offset=True,
    # use_batch_cancel_on_stop=True,
    # use_individual_cancels_on_stop=True,
    use_post_only=True,
    # cancel_orders_on_stop=False,
    # close_positions_on_stop=False,
    reduce_only_on_stop=reduce_only_on_stop,
    # test_reject_post_only=True,
    log_data=False,
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
