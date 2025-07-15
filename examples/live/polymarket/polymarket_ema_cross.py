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
from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_MAX_PRECISION_TAKER
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.live.config import LiveRiskEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# To find active markets run `python nautilus_trader/adapters/polymarket/scripts/active_markets.py`

# will-the-indiana-pacers-win-the-2025-nba-finals
# https://polymarket.com/event/will-the-new-york-knicks-win-the-2025-nba-finals
condition_id = "0xf2a89afeddff5315e37211b0b0e4e93ed167fba2694cd35c252672d0aca73711"
token_id = "5044658213116494392261893544497225363846217319105609804585534197935770239191"

instrument_ids = [
    get_polymarket_instrument_id(condition_id, token_id),
]

filters = {
    # "next_cursor": "MTE3MDA=",
    "is_active": True,
}

load_ids = [str(x) for x in instrument_ids]
instrument_provider_config = InstrumentProviderConfig(load_ids=frozenset(load_ids))
# instrument_provider_config = InstrumentProviderConfig(load_all=True, filters=filters)

instrument_id = instrument_ids[0]
trade_size = Decimal("5.0")

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        # inflight_check_interval_ms=0,  # Uncomment to turn off in-flight order checks
        # open_check_interval_secs=0,  # Uncomment to turn off open order checks
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    risk_engine=LiveRiskEngineConfig(bypass=True),  # WIP: Improve risk engine integration
    # cache=CacheConfig(
    #     # database=DatabaseConfig(),  # <-- Recommend Redis cache backing for Polymarket
    #     encoding="msgpack",
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    # ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     encoding="json",
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    #     streams_prefix="quoters",
    #     use_instance_id=False,
    #     # types_filter=[QuoteTick],
    #     autotrim_mins=30,
    # ),
    # heartbeat_interval=1.0,
    data_clients={
        POLYMARKET: PolymarketDataClientConfig(
            private_key=None,  # 'POLYMARKET_PK' env var
            api_key=None,  # 'POLYMARKET_API_KEY' env var
            api_secret=None,  # 'POLYMARKET_API_SECRET' env var
            passphrase=None,  # 'POLYMARKET_PASSPHRASE' env var
            instrument_provider=instrument_provider_config,
            ws_connection_delay_secs=5,
            compute_effective_deltas=True,
            # signature_type=2,
        ),
    },
    exec_clients={
        POLYMARKET: PolymarketExecClientConfig(
            private_key=None,  # 'POLYMARKET_PK' env var
            api_key=None,  # 'POLYMARKET_API_KEY' env var
            api_secret=None,  # 'POLYMARKET_API_SECRET' env var
            passphrase=None,  # 'POLYMARKET_PASSPHRASE' env var
            instrument_provider=instrument_provider_config,
            generate_order_history_from_trades=False,
            # log_raw_ws_messages=True,
            # signature_type=2,
        ),
    },
    timeout_connection=60.0,
    timeout_reconciliation=20.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=10.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = EMACrossConfig(
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    # Note: Polymarket doesn't provide bars, so we'll use quote-based bars
    # This creates 1-second bars from quote ticks
    bar_type=BarType.from_str(f"{instrument_id}-1-SECOND-MID-INTERNAL"),
    fast_ema_period=2,  # Shorter periods for prediction markets
    slow_ema_period=4,  # which move quickly
    trade_size=trade_size,
    order_id_tag="001",
    subscribe_quote_ticks=True,
    subscribe_trade_ticks=True,
    request_bars=False,  # No historical bars available from Polymarket
    unsubscribe_data_on_stop=False,  # Unsubscribe not supported by Polymarket
    order_time_in_force=TimeInForce.IOC,
    order_quantity_precision=POLYMARKET_MAX_PRECISION_TAKER,
    close_positions_on_stop=True,
    reduce_only_on_stop=False,  # Reduce-only not supported by Polymarket
)

# Instantiate your strategy
strategy = EMACross(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node
node.add_data_client_factory("POLYMARKET", PolymarketLiveDataClientFactory)
node.add_exec_client_factory("POLYMARKET", PolymarketLiveExecClientFactory)
node.build()


if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
