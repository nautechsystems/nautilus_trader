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
import socket
import sys
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
from nautilus_trader.live.config import LiveExecEngineConfig
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
    sys.modules[_spec.name] = _module
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
REDIS_PORT = int(os.getenv("POC_REDIS_PORT", "6380"))
REDIS_DB = int(os.getenv("POC_REDIS_DB", "0"))
REDIS_USERNAME = os.getenv("POC_REDIS_USERNAME") or None
REDIS_PASSWORD = os.getenv("POC_REDIS_PASSWORD") or None
POC_STRATEGY_ID = os.getenv("POC_STRATEGY_ID", "bybit_binance_plumeusdt_makerv3")
BYBIT_API_KEY = os.getenv("POC_BYBIT_API_KEY") or os.getenv("BYBIT_API_KEY")
BYBIT_API_SECRET = os.getenv("POC_BYBIT_API_SECRET") or os.getenv("BYBIT_API_SECRET")
BINANCE_API_KEY = os.getenv("POC_BINANCE_API_KEY") or os.getenv("BINANCE_API_KEY")
BINANCE_API_SECRET = os.getenv("POC_BINANCE_API_SECRET") or os.getenv("BINANCE_API_SECRET")
EXEC_RECONCILIATION = os.getenv("POC_EXEC_RECONCILIATION", "1") == "1"
EXEC_RECONCILIATION_LOOKBACK_MINS = int(os.getenv("POC_RECONCILIATION_LOOKBACK_MINS", "5"))
EXEC_RECONCILIATION_TIMEOUT_SEC = float(os.getenv("POC_RECONCILIATION_TIMEOUT_SEC", "30"))
EXEC_RECONCILIATION_STARTUP_DELAY_SEC = float(os.getenv("POC_RECONCILIATION_STARTUP_DELAY_SEC", "1"))

PARAMS_DEFAULTS: dict[str, object] = {
    "qty": 1000.0,
    "des_qty_global": 0.0,
    "max_qty_global": 40_000.0,
    "max_skew_bps_global": 20.0,
    "des_qty_local": 0.0,
    "max_qty_local": 0.0,
    "max_skew_bps_local": 0.0,
    "linear_offset_bps": 0.0,
    "n_orders1": 5,
    "distance1": 2.0,
    "bid_edge1": 10.0,
    "ask_edge1": 10.0,
    "place_edge1": 2.0,
    "n_orders2": 0,
    "distance2": 5.0,
    "bid_edge2": 25.0,
    "ask_edge2": 25.0,
    "place_edge2": 2.0,
    "n_orders3": 0,
    "distance3": 5.0,
    "bid_edge3": 50.0,
    "ask_edge3": 50.0,
    "place_edge3": 2.0,
    "quote_fail_critical_after_count": 3,
    "quote_fail_critical_after_s": 60.0,
    "max_age_ms": 10_000,
    "bot_on": False,
}


def _resp_command(*parts: str) -> bytes:
    encoded = [part.encode("utf-8") for part in parts]
    out = [f"*{len(encoded)}\r\n".encode("ascii")]
    for part in encoded:
        out.append(f"${len(part)}\r\n".encode("ascii"))
        out.append(part + b"\r\n")
    return b"".join(out)


def _readline(sock: socket.socket) -> bytes:
    chunks: list[bytes] = []
    while True:
        data = sock.recv(1)
        if not data:
            raise RuntimeError("redis socket closed")
        chunks.append(data)
        if len(chunks) >= 2 and chunks[-2] == b"\r" and chunks[-1] == b"\n":
            return b"".join(chunks[:-2])


def _read_redis_reply(sock: socket.socket) -> bytes | None:
    prefix = sock.recv(1)
    if not prefix:
        raise RuntimeError("redis socket closed")
    if prefix == b"+":
        return _readline(sock)
    if prefix == b"$":
        length = int(_readline(sock))
        if length < 0:
            return None
        payload = b""
        while len(payload) < length + 2:
            chunk = sock.recv(length + 2 - len(payload))
            if not chunk:
                raise RuntimeError("redis socket closed")
            payload += chunk
        return payload[:-2]
    if prefix == b":":
        return _readline(sock)
    if prefix == b"-":
        message = _readline(sock).decode("utf-8", errors="replace")
        raise RuntimeError(f"redis error: {message}")
    raise RuntimeError(f"unsupported redis reply prefix: {prefix!r}")


def _redis_get(key: str) -> str | None:
    try:
        with socket.create_connection((REDIS_HOST, REDIS_PORT), timeout=1.5) as sock:
            if REDIS_PASSWORD:
                if REDIS_USERNAME:
                    sock.sendall(_resp_command("AUTH", REDIS_USERNAME, REDIS_PASSWORD))
                else:
                    sock.sendall(_resp_command("AUTH", REDIS_PASSWORD))
                _read_redis_reply(sock)

            if REDIS_DB:
                sock.sendall(_resp_command("SELECT", str(REDIS_DB)))
                _read_redis_reply(sock)

            sock.sendall(_resp_command("GET", key))
            value = _read_redis_reply(sock)
            if value is None:
                return None
            return value.decode("utf-8", errors="replace")
    except Exception:
        return None


def _coerce_param(name: str, default: object, raw: str | None) -> object:
    if raw is None:
        return default
    text = raw.strip()
    if isinstance(default, bool):
        lower = text.lower()
        if lower in {"1", "true", "t", "yes", "y", "on", "enabled"}:
            return True
        if lower in {"0", "false", "f", "no", "n", "off", "disabled"}:
            return False
        return default
    if isinstance(default, int):
        try:
            return int(float(text))
        except ValueError:
            return default
    if isinstance(default, float):
        try:
            return float(text)
        except ValueError:
            return default
    return text


def _load_params() -> dict[str, object]:
    loaded: dict[str, object] = dict(PARAMS_DEFAULTS)
    for name, default in PARAMS_DEFAULTS.items():
        key = f"strategy.{POC_STRATEGY_ID}.{name}"
        loaded[name] = _coerce_param(name, default, _redis_get(key))
    return loaded


RUNTIME_PARAMS = _load_params()

config_node = TradingNodeConfig(
    trader_id=TraderId("MAKER-POC-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=EXEC_RECONCILIATION,
        reconciliation_lookback_mins=EXEC_RECONCILIATION_LOOKBACK_MINS,
        reconciliation_instrument_ids=[BYBIT_EXEC_INSTRUMENT_ID],
        reconciliation_startup_delay_secs=EXEC_RECONCILIATION_STARTUP_DELAY_SEC,
    ),
    message_bus=MessageBusConfig(
        database=DatabaseConfig(
            type="redis",
            host=REDIS_HOST,
            port=REDIS_PORT,
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
            api_key=BYBIT_API_KEY,
            api_secret=BYBIT_API_SECRET,
            instrument_provider=InstrumentProviderConfig(
                load_ids=frozenset([BYBIT_EXEC_INSTRUMENT_ID]),
            ),
            product_types=(BybitProductType.LINEAR,),
            testnet=False,
            demo=False,
        ),
        BINANCE: BinanceDataClientConfig(
            api_key=BINANCE_API_KEY,
            api_secret=BINANCE_API_SECRET,
            account_type=BinanceAccountType.SPOT,
            instrument_provider=InstrumentProviderConfig(
                load_ids=frozenset([BINANCE_DATA_INSTRUMENT_ID]),
            ),
        ),
    },
    exec_clients=(
        {
            BYBIT: BybitExecClientConfig(
                api_key=BYBIT_API_KEY,
                api_secret=BYBIT_API_SECRET,
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
    timeout_reconciliation=EXEC_RECONCILIATION_TIMEOUT_SEC,
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
    order_qty=Decimal(str(RUNTIME_PARAMS["qty"])),
    qty=Decimal(str(RUNTIME_PARAMS["qty"])),
    des_qty_global=float(RUNTIME_PARAMS["des_qty_global"]),
    max_qty_global=float(RUNTIME_PARAMS["max_qty_global"]),
    max_skew_bps_global=float(RUNTIME_PARAMS["max_skew_bps_global"]),
    des_qty_local=float(RUNTIME_PARAMS["des_qty_local"]),
    max_qty_local=float(RUNTIME_PARAMS["max_qty_local"]),
    max_skew_bps_local=float(RUNTIME_PARAMS["max_skew_bps_local"]),
    linear_offset_bps=float(RUNTIME_PARAMS["linear_offset_bps"]),
    bot_on=bool(RUNTIME_PARAMS["bot_on"]),
    max_age_ms=int(RUNTIME_PARAMS["max_age_ms"]),
    bid_edge1=float(RUNTIME_PARAMS["bid_edge1"]),
    ask_edge1=float(RUNTIME_PARAMS["ask_edge1"]),
    place_edge1=float(RUNTIME_PARAMS["place_edge1"]),
    distance1=float(RUNTIME_PARAMS["distance1"]),
    n_orders1=int(RUNTIME_PARAMS["n_orders1"]),
    bid_edge2=float(RUNTIME_PARAMS["bid_edge2"]),
    ask_edge2=float(RUNTIME_PARAMS["ask_edge2"]),
    place_edge2=float(RUNTIME_PARAMS["place_edge2"]),
    distance2=float(RUNTIME_PARAMS["distance2"]),
    n_orders2=int(RUNTIME_PARAMS["n_orders2"]),
    bid_edge3=float(RUNTIME_PARAMS["bid_edge3"]),
    ask_edge3=float(RUNTIME_PARAMS["ask_edge3"]),
    place_edge3=float(RUNTIME_PARAMS["place_edge3"]),
    distance3=float(RUNTIME_PARAMS["distance3"]),
    n_orders3=int(RUNTIME_PARAMS["n_orders3"]),
    quote_fail_critical_after_count=int(RUNTIME_PARAMS["quote_fail_critical_after_count"]),
    quote_fail_critical_after_s=float(RUNTIME_PARAMS["quote_fail_critical_after_s"]),
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
