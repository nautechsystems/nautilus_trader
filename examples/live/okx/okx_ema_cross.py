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

from nautilus_trader.adapters.okx import OKX
from nautilus_trader.adapters.okx import OKXDataClientConfig
from nautilus_trader.adapters.okx import OKXExecClientConfig
from nautilus_trader.adapters.okx import OKXLiveDataClientFactory
from nautilus_trader.adapters.okx import OKXLiveExecClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.examples.strategies.ema_cross import EMACross
from nautilus_trader.examples.strategies.ema_cross import EMACrossConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***

# Configuration - Change instrument_type to switch between trading modes
instrument_type = OKXInstrumentType.SWAP  # SPOT, SWAP, FUTURES, OPTION

# Symbol mapping based on instrument type
if instrument_type == OKXInstrumentType.SPOT:
    symbol = "ETH-USDT"
    contract_types: tuple[OKXContractType, ...] | None = None  # SPOT doesn't use contract types
    trade_size = Decimal("0.01")
elif instrument_type == OKXInstrumentType.SWAP:
    symbol = "ETH-USDT-SWAP"
    contract_types = (OKXContractType.LINEAR, OKXContractType.INVERSE)
    trade_size = Decimal("0.01")
elif instrument_type == OKXInstrumentType.FUTURES:
    # Note: ETH-USD futures follow same pattern as BTC-USD
    # Format: ETH-USD-YYMMDD (e.g., ETH-USD-241227, ETH-USD-250131)
    symbol = "ETH-USD-251226"  # ETH-USD futures expiring December 26, 2025
    contract_types = (OKXContractType.INVERSE,)  # ETH-USD futures are inverse contracts
    trade_size = Decimal(1)
elif instrument_type == OKXInstrumentType.OPTION:
    symbol = "ETH-USD-250328-4000-C"  # Example: ETH-USD call option, strike $4000, exp 2025-03-28
    contract_types = None  # OPTIONS don't use contract types in the same way
    trade_size = Decimal(1)
else:
    raise ValueError(f"Unsupported instrument type: {instrument_type}")

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    # cache=CacheConfig(
    #     database=DatabaseConfig(),
    #     buffer_interval_ms=100,
    # ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     streams_prefix="quoters",
    #     use_instance_id=False,
    #     timestamps_as_iso8601=True,
    #     # types_filter=[QuoteTick],
    #     autotrim_mins=1,
    #     heartbeat_interval_secs=1,
    # ),
    data_clients={
        OKX: OKXDataClientConfig(
            api_key=None,  # 'OKX_API_KEY' env var
            api_secret=None,  # 'OKX_API_SECRET' env var
            api_passphrase=None,  # 'OKX_API_PASSPHRASE' env var
            base_url_http=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(instrument_type,),  # Will load swap instruments
            contract_types=contract_types,  # Will load linear contracts
            is_demo=False,  # If client uses the demo API
            http_timeout_secs=10,  # Set to reasonable duration
        ),
    },
    exec_clients={
        OKX: OKXExecClientConfig(
            api_key=None,  # 'OKX_API_KEY' env var
            api_secret=None,  # 'OKX_API_SECRET' env var
            api_passphrase=None,  # 'OKX_API_PASSPHRASE' env var
            base_url_http=None,  # Override with custom endpoint
            base_url_ws=None,  # Override with custom endpoint
            instrument_provider=InstrumentProviderConfig(load_all=True),
            instrument_types=(instrument_type,),
            contract_types=contract_types,
            is_demo=False,  # If client uses the demo API
            http_timeout_secs=10,  # Set to reasonable duration
        ),
    },
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=5.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure your strategy
strat_config = EMACrossConfig(
    instrument_id=InstrumentId.from_str(f"{symbol}.OKX"),
    external_order_claims=[InstrumentId.from_str(f"{symbol}.OKX")],
    bar_type=BarType.from_str(f"{symbol}.OKX-1-MINUTE-LAST-EXTERNAL"),
    fast_ema_period=10,
    slow_ema_period=20,
    trade_size=trade_size,
    order_id_tag="001",
    use_hyphens_in_client_order_ids=False,  # OKX doesn't allow hyphens in client order IDs
)
# Instantiate your strategy
strategy = EMACross(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(OKX, OKXLiveDataClientFactory)
node.add_exec_client_factory(OKX, OKXLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
