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

from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitExecClientConfig
from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit import BybitLiveExecClientFactory
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.portfolio.config import PortfolioConfig
from nautilus_trader.test_kit.strategies.tester_exec import ExecTester
from nautilus_trader.test_kit.strategies.tester_exec import ExecTesterConfig


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# SPOT/LINEAR
product_type = BybitProductType.LINEAR

if product_type == BybitProductType.SPOT:
    symbol = f"ETHUSDT-{product_type.value.upper()}"
    order_qty = Decimal("0.01")
    order_params = {"is_leverage": True}
    enable_sells = False
    use_spot_position_reports = True  # CAUTION: Experimental feature
elif product_type == BybitProductType.LINEAR:
    symbol = f"ETHUSDT-{product_type.value.upper()}"
    order_qty = Decimal("0.01")
    order_params = {}
    enable_sells = True
    use_spot_position_reports = False
elif product_type == BybitProductType.INVERSE:
    symbol = f"XRPUSD-{product_type.value.upper()}"
    order_qty = Decimal("50")
    enable_sells = True
    use_spot_position_reports = False
else:
    raise NotImplementedError

instrument_id = InstrumentId.from_str(f"{symbol}.{BYBIT}")
# instrument_id2 = InstrumentId.from_str(f"ETHUSDT-LINEAR.{BYBIT}")

# Only reconcile these instruments
reconciliation_instrument_ids = [instrument_id]
# reconciliation_instrument_ids = [instrument_id, instrument_id2]

product_types: list[BybitProductType] = [product_type]
# product_types: list[BybitProductType] = [product_type, BybitProductType.LINEAR]

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
        reconciliation_lookback_mins=2880,
        reconciliation_instrument_ids=reconciliation_instrument_ids,
        open_check_interval_secs=5.0,
        open_check_open_only=False,
        # filtered_client_order_ids=[ClientOrderId("1757985206157")],  # For demonstration
        # own_books_audit_interval_secs=2.0,
        # manage_own_order_books=True,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
        # purge_closed_orders_interval_mins=1,  # Example of purging closed orders for HFT
        # purge_closed_orders_buffer_mins=0,  # Purged orders closed for at least an hour
        # purge_closed_positions_interval_mins=1,  # Example of purging closed positions for HFT
        # purge_closed_positions_buffer_mins=0,  # Purge positions closed for at least an hour
        # purge_account_events_interval_mins=1,  # Example of purging account events for HFT
        # purge_account_events_lookback_mins=0,  # Purge account events occurring more than an hour ago
        # purge_from_database=True,  # Set True with caution
        graceful_shutdown_on_exception=True,
    ),
    risk_engine=LiveRiskEngineConfig(bypass=True),
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
        BYBIT: BybitDataClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=product_types,
            demo=False,  # If client uses the demo API
            testnet=False,  # If client uses the testnet API
            recv_window_ms=5_000,  # Default
        ),
    },
    exec_clients={
        BYBIT: BybitExecClientConfig(
            api_key=None,  # 'BYBIT_API_KEY' env var
            api_secret=None,  # 'BYBIT_API_SECRET' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws_private=None,  # Override with custom endpoint
            use_ws_trade_api=True,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            product_types=product_types,
            use_spot_position_reports=use_spot_position_reports,
            demo=False,  # If client uses the demo API
            testnet=False,  # If client uses the testnet API
            max_retries=3,
            retry_delay_initial_ms=1_000,
            retry_delay_max_ms=10_000,
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
config_tester = ExecTesterConfig(
    instrument_id=InstrumentId.from_str(f"{symbol}.{BYBIT}"),
    external_order_claims=[InstrumentId.from_str(f"{symbol}.{BYBIT}")],
    subscribe_quotes=True,
    subscribe_trades=True,
    # subscribe_book=True,
    enable_sells=enable_sells,
    order_qty=order_qty,
    open_position_on_start_qty=order_qty,
    # tob_offset_ticks=1,
    use_post_only=True,
    # test_reject_post_only=True,
    reduce_only_on_stop=False,  # Not supported for Bybit SPOT
    # cancel_orders_on_stop=False,
    # close_positions_on_stop=False,
    log_data=False,
    log_rejected_due_post_only_as_warning=False,
)

# Instantiate your strategy
tester = ExecTester(config=config_tester)

# Add your strategies and modules
node.trader.add_strategy(tester)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)
node.add_exec_client_factory(BYBIT, BybitLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
