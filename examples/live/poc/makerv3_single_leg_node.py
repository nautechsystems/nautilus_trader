#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
import importlib.util
import os
from pathlib import Path

from nautilus_trader.adapters.binance import BINANCE
from nautilus_trader.adapters.binance import BinanceAccountType
from nautilus_trader.adapters.binance import BinanceDataClientConfig
from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory
from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitExecClientConfig
from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory
from nautilus_trader.adapters.bybit import BybitLiveExecClientFactory
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.config import DatabaseConfig
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.config import TradingNodeConfig
try:
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        MakerV3SingleLegQuoter,
    )
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        MakerV3SingleLegQuoterConfig,
    )
except ModuleNotFoundError:
    _strategy_path = (
        Path(__file__).resolve().parents[3]
        / "nautilus_trader/examples/strategies/makerv3_single_leg_quoter.py"
    )
    _spec = importlib.util.spec_from_file_location("makerv3_single_leg_quoter_local", _strategy_path)
    if _spec is None or _spec.loader is None:
        raise RuntimeError(f"Failed to load strategy module from {_strategy_path}")
    _module = importlib.util.module_from_spec(_spec)
    _spec.loader.exec_module(_module)
    MakerV3SingleLegQuoter = _module.MakerV3SingleLegQuoter
    MakerV3SingleLegQuoterConfig = _module.MakerV3SingleLegQuoterConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


BYBIT_EXEC_INSTRUMENT_ID = InstrumentId.from_str("PLUMEUSDT-LINEAR.BYBIT")
BINANCE_DATA_INSTRUMENT_ID = InstrumentId.from_str("PLUMEUSDT.BINANCE")
ENABLE_EXEC = os.getenv("POC_ENABLE_EXEC", "0") == "1"
REDIS_HOST = os.getenv("POC_REDIS_HOST", "127.0.0.1")
REDIS_PORT = int(os.getenv("POC_REDIS_PORT", "6379"))
REDIS_DB = int(os.getenv("POC_REDIS_DB", "0"))
REDIS_USERNAME = os.getenv("POC_REDIS_USERNAME") or None
REDIS_PASSWORD = os.getenv("POC_REDIS_PASSWORD") or None
POC_STRATEGY_ID = os.getenv("POC_STRATEGY_ID", "bybit_binance_plumeusdt_makerv3")

config_node = TradingNodeConfig(
    trader_id=TraderId("MAKER-POC-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    message_bus=MessageBusConfig(
        database=DatabaseConfig(
            type="redis",
            host=REDIS_HOST,
            port=REDIS_PORT,
            database=REDIS_DB,
            username=REDIS_USERNAME,
            password=REDIS_PASSWORD,
        ),
        encoding="json",
        use_trader_prefix=False,
        use_trader_id=False,
        use_instance_id=False,
        streams_prefix="maker_poc",
        stream_per_topic=False,
        types_filter=[OrderBookDeltas],
    ),
    data_clients={
        BYBIT: BybitDataClientConfig(
            api_key=None,
            api_secret=None,
            instrument_provider=InstrumentProviderConfig(
                load_ids=frozenset([BYBIT_EXEC_INSTRUMENT_ID]),
            ),
            product_types=(BybitProductType.LINEAR,),
            testnet=False,
            demo=False,
        ),
        BINANCE: BinanceDataClientConfig(
            api_key=None,
            api_secret=None,
            account_type=BinanceAccountType.SPOT,
            instrument_provider=InstrumentProviderConfig(
                load_ids=frozenset([BINANCE_DATA_INSTRUMENT_ID]),
            ),
        ),
    },
    exec_clients=(
        {
            BYBIT: BybitExecClientConfig(
                api_key=None,
                api_secret=None,
                instrument_provider=InstrumentProviderConfig(
                    load_ids=frozenset([BYBIT_EXEC_INSTRUMENT_ID]),
                ),
                product_types=(BybitProductType.LINEAR,),
                testnet=False,
                demo=False,
            ),
        }
        if ENABLE_EXEC
        else {}
    ),
    timeout_connection=20.0,
    timeout_reconciliation=10.0,
    timeout_portfolio=10.0,
    timeout_disconnection=10.0,
    timeout_post_stop=5.0,
)

node = TradingNode(config=config_node)

strategy_config = MakerV3SingleLegQuoterConfig(
    strategy_id="MAKERV3-SINGLELEG-001",
    bybit_instrument_id=BYBIT_EXEC_INSTRUMENT_ID,
    binance_instrument_id=BINANCE_DATA_INSTRUMENT_ID,
    external_strategy_id=POC_STRATEGY_ID,
    order_qty=Decimal("1000"),
    qty=Decimal("1000"),
    hedge_qty=Decimal("1000"),
    des_qty_global=0,
    max_qty_global=20_000,
    max_skew_bps_global=20,
    des_qty_local=0,
    max_qty_local=0,
    max_skew_bps_local=0,
    linear_offset_bps=0,
    bot_on=False,
    max_age_ms=10_000,
    bid_edge1=10,
    ask_edge1=10,
    place_edge1=2,
    distance1=5,
    n_orders1=5,
    bid_edge2=25,
    ask_edge2=25,
    place_edge2=2,
    distance2=5,
    n_orders2=5,
    bid_edge3=50,
    ask_edge3=50,
    place_edge3=2,
    distance3=5,
    n_orders3=5,
    bid_edge_hedge=10,
    ask_edge_hedge=10,
    distance_hedge=5,
    n_orders_hedge=5,
    place_edge_hedge=2,
    strategy_take_enabled=False,
    bid_edge_take=0,
    ask_edge_take=0,
    take_qty=0,
    take_cooldown=0,
    hedge_reduce_only=True,
    hedge_touch_at_max_qty=True,
    quote_fail_critical_after_count=3,
    quote_fail_critical_after_s=60.0,
)
strategy = MakerV3SingleLegQuoter(config=strategy_config)
node.trader.add_strategy(strategy)

node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)
if ENABLE_EXEC:
    node.add_exec_client_factory(BYBIT, BybitLiveExecClientFactory)
node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
node.build()


if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
