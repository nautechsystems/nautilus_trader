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

"""Run a live MakerV3 trading node using canonical strategy exports."""

from __future__ import annotations

import argparse
from decimal import Decimal
import os
from pathlib import Path
from typing import Any
import tomllib

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
from nautilus_trader.flux.common.config import FLUX_DEFAULT_NAMESPACE
from nautilus_trader.flux.common.config import FLUX_SCHEMA_VERSION
from nautilus_trader.flux.strategies import MakerV3Strategy
from nautilus_trader.flux.strategies import MakerV3StrategyConfig
from nautilus_trader.live.config import LiveExecEngineConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId


SAFE_MODES = frozenset({"paper", "testnet", "live"})
DEFAULT_CONFIG_PATH = Path(__file__).with_name("config") / "makerv3.toml"


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _load_config(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"Config root must be a table: {path}")
    return data


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    value = data.get(name, {})
    if not isinstance(value, dict):
        raise ValueError(f"[{name}] must be a TOML table")
    return value


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run MakerV3 trading node using flux production modules.")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG_PATH)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--enable-execution", action="store_true")
    return parser.parse_args()


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    flux = _table(config, "flux")
    mode = str(args.mode or flux.get("mode", "paper")).strip().lower()
    if mode not in SAFE_MODES:
        raise ValueError(f"Invalid mode {mode!r}; expected one of {sorted(SAFE_MODES)}")
    if mode == "live" and not args.confirm_live:
        raise ValueError("Live mode requires explicit --confirm-live")
    return mode


def _resolve_secret(section: dict[str, Any], *, value_key: str, env_key: str) -> str | None:
    inline = _optional_text(section.get(value_key))
    if inline:
        return inline

    env_name = _optional_text(section.get(env_key))
    if not env_name:
        return None

    value = os.getenv(env_name)
    if value is None:
        return None
    return value


def _enum_member(enum_type: Any, raw_value: Any, *, field_name: str) -> Any:
    if isinstance(raw_value, enum_type):
        return raw_value

    name = str(raw_value).strip().upper()
    if not name:
        raise ValueError(f"Missing {field_name}")

    try:
        return enum_type[name]
    except (KeyError, TypeError) as exc:
        try:
            return getattr(enum_type, name)
        except AttributeError:
            raise ValueError(f"Invalid {field_name} {raw_value!r}") from exc


def build_node(config: dict[str, Any], *, mode: str, force_enable_execution: bool) -> TradingNode:
    """Build and return a configured trading node for MakerV3."""
    flux = _table(config, "flux")
    identity = _table(config, "identity")
    redis_cfg = _table(config, "redis")
    node_cfg = _table(config, "node")
    bybit_cfg = _table(node_cfg, "bybit")
    binance_cfg = _table(node_cfg, "binance")
    strategy_cfg = _table(config, "strategy")

    strategy_id = _optional_text(identity.get("strategy_id")) or "makerv3"
    external_strategy_id = _optional_text(identity.get("external_strategy_id")) or strategy_id
    trader_id = _optional_text(identity.get("trader_id")) or "MAKER-PAPER-001"
    namespace = _optional_text(flux.get("namespace")) or FLUX_DEFAULT_NAMESPACE
    schema_version = _optional_text(flux.get("schema_version")) or FLUX_SCHEMA_VERSION

    maker_instrument_id = InstrumentId.from_str(str(node_cfg.get("maker_instrument_id", "PLUMEUSDT-LINEAR.BYBIT")))
    reference_instrument_id = InstrumentId.from_str(str(node_cfg.get("reference_instrument_id", "PLUMEUSDT.BINANCE")))

    bybit_api_key = _resolve_secret(bybit_cfg, value_key="api_key", env_key="api_key_env")
    bybit_api_secret = _resolve_secret(bybit_cfg, value_key="api_secret", env_key="api_secret_env")
    binance_api_key = _resolve_secret(binance_cfg, value_key="api_key", env_key="api_key_env")
    binance_api_secret = _resolve_secret(binance_cfg, value_key="api_secret", env_key="api_secret_env")

    enable_execution = bool(node_cfg.get("enable_execution", False)) or force_enable_execution

    config_node = TradingNodeConfig(
        trader_id=TraderId(trader_id),
        logging=LoggingConfig(
            log_level=str(node_cfg.get("log_level", "INFO")),
            use_pyo3=True,
        ),
        exec_engine=LiveExecEngineConfig(
            reconciliation=bool(node_cfg.get("exec_reconciliation", True)),
            reconciliation_lookback_mins=int(node_cfg.get("exec_reconciliation_lookback_mins", 5)),
            reconciliation_instrument_ids=[maker_instrument_id],
            reconciliation_startup_delay_secs=float(node_cfg.get("exec_reconciliation_startup_delay_secs", 1.0)),
        ),
        message_bus=MessageBusConfig(
            database=DatabaseConfig(
                type="redis",
                host=str(redis_cfg.get("host", "127.0.0.1")),
                port=int(redis_cfg.get("port", 6380)),
                username=_optional_text(redis_cfg.get("username")),
                password=_optional_text(redis_cfg.get("password")),
            ),
            encoding="json",
            use_trader_prefix=False,
            use_trader_id=False,
            use_instance_id=False,
            streams_prefix=f"{namespace}:{schema_version}:in:stream:{mode}:{strategy_id}",
            stream_per_topic=True,
            types_filter=[OrderBookDeltas],
        ),
        data_clients={
            BYBIT: BybitDataClientConfig(
                api_key=bybit_api_key,
                api_secret=bybit_api_secret,
                instrument_provider=InstrumentProviderConfig(load_ids=frozenset([maker_instrument_id])),
                product_types=(
                    _enum_member(
                        BybitProductType,
                        bybit_cfg.get("product_type", "LINEAR"),
                        field_name="node.bybit.product_type",
                    ),
                ),
                testnet=bool(bybit_cfg.get("testnet", mode != "live")),
                demo=bool(bybit_cfg.get("demo", False)),
            ),
            BINANCE: BinanceDataClientConfig(
                api_key=binance_api_key,
                api_secret=binance_api_secret,
                account_type=_enum_member(
                    BinanceAccountType,
                    binance_cfg.get("account_type", "SPOT"),
                    field_name="node.binance.account_type",
                ),
                instrument_provider=InstrumentProviderConfig(load_ids=frozenset([reference_instrument_id])),
            ),
        },
        exec_clients=(
            {
                BYBIT: BybitExecClientConfig(
                    api_key=bybit_api_key,
                    api_secret=bybit_api_secret,
                    instrument_provider=InstrumentProviderConfig(load_ids=frozenset([maker_instrument_id])),
                    product_types=(
                        _enum_member(
                            BybitProductType,
                            bybit_cfg.get("product_type", "LINEAR"),
                            field_name="node.bybit.product_type",
                        ),
                    ),
                    testnet=bool(bybit_cfg.get("testnet", mode != "live")),
                    demo=bool(bybit_cfg.get("demo", False)),
                ),
            }
            if enable_execution
            else {}
        ),
        timeout_connection=float(node_cfg.get("timeout_connection", 20.0)),
        timeout_reconciliation=float(node_cfg.get("timeout_reconciliation", 30.0)),
        timeout_portfolio=float(node_cfg.get("timeout_portfolio", 10.0)),
        timeout_disconnection=float(node_cfg.get("timeout_disconnection", 10.0)),
        timeout_post_stop=float(node_cfg.get("timeout_post_stop", 5.0)),
    )

    order_qty = Decimal(str(strategy_cfg.get("order_qty", "1000")))
    qty_raw = strategy_cfg.get("qty", strategy_cfg.get("order_qty", "1000"))
    qty = Decimal(str(qty_raw)) if qty_raw is not None else None

    strategy = MakerV3Strategy(
        config=MakerV3StrategyConfig(
            strategy_id=str(strategy_cfg.get("strategy_id", "MAKERV3-001")),
            maker_instrument_id=maker_instrument_id,
            reference_instrument_id=reference_instrument_id,
            external_strategy_id=external_strategy_id,
            order_qty=order_qty,
            qty=qty,
            bot_on=bool(strategy_cfg.get("bot_on", False)),
            des_qty_global=float(strategy_cfg.get("des_qty_global", 0.0)),
            max_qty_global=float(strategy_cfg.get("max_qty_global", 40_000.0)),
            max_skew_bps_global=float(strategy_cfg.get("max_skew_bps_global", 20.0)),
            des_qty_local=float(strategy_cfg.get("des_qty_local", 0.0)),
            max_qty_local=float(strategy_cfg.get("max_qty_local", 0.0)),
            max_skew_bps_local=float(strategy_cfg.get("max_skew_bps_local", 0.0)),
            linear_offset_bps=float(strategy_cfg.get("linear_offset_bps", 0.0)),
            max_age_ms=int(strategy_cfg.get("max_age_ms", 10_000)),
            bid_edge1=float(strategy_cfg.get("bid_edge1", 10.0)),
            ask_edge1=float(strategy_cfg.get("ask_edge1", 10.0)),
            place_edge1=float(strategy_cfg.get("place_edge1", 2.0)),
            distance1=float(strategy_cfg.get("distance1", 2.0)),
            n_orders1=int(strategy_cfg.get("n_orders1", 5)),
            bid_edge2=float(strategy_cfg.get("bid_edge2", 25.0)),
            ask_edge2=float(strategy_cfg.get("ask_edge2", 25.0)),
            place_edge2=float(strategy_cfg.get("place_edge2", 2.0)),
            distance2=float(strategy_cfg.get("distance2", 5.0)),
            n_orders2=int(strategy_cfg.get("n_orders2", 0)),
            bid_edge3=float(strategy_cfg.get("bid_edge3", 50.0)),
            ask_edge3=float(strategy_cfg.get("ask_edge3", 50.0)),
            place_edge3=float(strategy_cfg.get("place_edge3", 2.0)),
            distance3=float(strategy_cfg.get("distance3", 5.0)),
            n_orders3=int(strategy_cfg.get("n_orders3", 0)),
            quote_fail_critical_after_count=int(strategy_cfg.get("quote_fail_critical_after_count", 3)),
            quote_fail_critical_after_s=float(strategy_cfg.get("quote_fail_critical_after_s", 60.0)),
        ),
    )

    node = TradingNode(config=config_node)
    node.trader.add_strategy(strategy)
    node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)
    node.add_data_client_factory(BINANCE, BinanceLiveDataClientFactory)
    if enable_execution:
        node.add_exec_client_factory(BYBIT, BybitLiveExecClientFactory)
    node.build()
    return node


def main() -> None:
    """Parse CLI arguments and run the MakerV3 trading node."""
    args = _parse_args()
    config = _load_config(args.config)
    mode = _resolve_mode(config, args)

    node = build_node(
        config,
        mode=mode,
        force_enable_execution=bool(args.enable_execution),
    )

    try:
        node.run()
    finally:
        node.dispose()


if __name__ == "__main__":
    main()
