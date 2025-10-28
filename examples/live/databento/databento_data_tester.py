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

from nautilus_trader.adapters.databento import DATABENTO
from nautilus_trader.adapters.databento import DatabentoDataClientConfig
from nautilus_trader.adapters.databento import DatabentoLiveDataClientFactory
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.test_kit.strategies.tester_data import DataTester
from nautilus_trader.test_kit.strategies.tester_data import DataTesterConfig


# For correct subscription operation, you must specify all instruments to be immediately
# subscribed for as part of the data client configuration
instrument_ids = [
    # InstrumentId.from_str("ES.c.0.GLBX"),  # TODO: Continuous contracts only work with GLBX for now
    InstrumentId.from_str("ESZ5.XCME"),
    # InstrumentId.from_str("ES.FUT.XCME"),
    # InstrumentId.from_str("CL.FUT.NYMEX"),
    # InstrumentId.from_str("LO.OPT.NYMEX"),
    # InstrumentId.from_str("AAPL.XNAS"),
    # InstrumentId.from_str("AAPL.IEXG"),
]

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=False,  # Not applicable
    ),
    data_clients={
        DATABENTO: DatabentoDataClientConfig(
            api_key=None,  # 'DATABENTO_API_KEY' env var
            http_gateway=None,
            instrument_provider=InstrumentProviderConfig(load_all=True),
            use_exchange_as_venue=True,
            mbo_subscriptions_delay=10.0,
            instrument_ids=instrument_ids,
            parent_symbols={"GLBX.MDP3": {"ES.FUT"}},
            # venue_dataset_map={"XNAS": "EQUS.MINI"},
        ),
    },
    timeout_connection=30.0,
    timeout_disconnection=10.0,
    timeout_post_stop=1.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)

# Configure and initialize the tester
config_tester = DataTesterConfig(
    instrument_ids=instrument_ids,
    bar_types=[BarType.from_str(f"{instrument_ids[0]}-1-SECOND-LAST-EXTERNAL")],
    # bar_types=[BarType.from_str(f"{instrument_ids[0]}-1-DAY-LAST-EXTERNAL")],
    # subscribe_params={"schema": "bbo-1s"},
    # subscribe_params={"schema": "ohlcv-eod"},
    # subscribe_book_deltas=True,
    # subscribe_book_depth=True,
    # subscribe_book_at_interval=True,
    subscribe_quotes=True,
    subscribe_trades=True,
    subscribe_bars=True,
    subscribe_instrument_status=True,
    subscribe_instrument_close=False,
    can_unsubscribe=False,  # Unsubscribing not supported by Databento
    # request_bars=True,  # Requires knowing the available time range `end`
    book_type=BookType.L3_MBO,
    # book_interval_ms=100,
)
tester = DataTester(config=config_tester)

node.trader.add_actor(tester)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(DATABENTO, DatabentoLiveDataClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
