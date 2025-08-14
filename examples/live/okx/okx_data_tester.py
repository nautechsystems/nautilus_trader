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
from nautilus_trader.adapters.okx import OKXLiveDataClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.nautilus_pyo3 import OKXContractType
from nautilus_trader.core.nautilus_pyo3 import OKXInstrumentType
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


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
        reconciliation=False,  # Not applicable
    ),
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
    timeout_connection=20.0,
    timeout_disconnection=5.0,
    timeout_post_stop=0.0,  # No stop delay needed for data testing
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure and initialize the tester
config_tester = DataTesterConfig(
    instrument_ids=[InstrumentId.from_str(f"{symbol}.OKX")],
    bar_types=[BarType.from_str(f"{symbol}.OKX-1-MINUTE-LAST-EXTERNAL")],
    # subscribe_book_deltas=True,
    subscribe_book_at_interval=True,  # Only legacy Cython wrapped book (not PyO3)
    # subscribe_quotes=True,
    # subscribe_trades=True,
    # subscribe_mark_prices=True,
    # subscribe_index_prices=True if instrument_type == OKXInstrumentType.SPOT else False,
    subscribe_funding_rates=True,
    # subscribe_bars=True,
    # subscribe_instrument_status=False,
    # subscribe_instrument_close=False,
    # request_bars=True,
    # book_group_size=Decimal("1"),  # Only PyO3 wrapped book (not legacy Cython)
    # book_levels_to_print=20,
    book_interval_ms=10,
    # manage_book=True,
    # use_pyo3_book=True,
)
tester = DataTester(config=config_tester)

node.trader.add_actor(tester)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(OKX, OKXLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
