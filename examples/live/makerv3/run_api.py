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

from __future__ import annotations

import argparse
import os
import tomllib
from pathlib import Path
from typing import Any
from typing import cast

import redis
from flask import abort
from flask import send_from_directory

from nautilus_trader.flux.api import ContractCatalogEntry
from nautilus_trader.flux.api import StrategyMetadata
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.api.app import RedisClientProtocol
from nautilus_trader.flux.common.config import FLUX_DEFAULT_NAMESPACE
from nautilus_trader.flux.common.config import FLUX_SCHEMA_VERSION
from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.config import FluxRedisConfig
from nautilus_trader.flux.common.config import FluxVenuesConfig


SAFE_MODES = frozenset({"paper", "testnet", "live"})
DEFAULT_CONFIG_PATH = Path(__file__).with_name("config") / "makerv3.toml"
DEFAULT_TOKENMM_BASE_PATH = "/tokenmm"
DEFAULT_FLUXBOARD_DIST = Path(__file__).resolve().parents[3] / "fluxboard" / "dist"


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
    parser = argparse.ArgumentParser(description="Run Flux API app for MakerV3.")
    parser.add_argument("--config", type=Path, default=DEFAULT_CONFIG_PATH)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--host", default=None)
    parser.add_argument("--port", type=int, default=None)
    parser.add_argument(
        "--serve-fluxboard",
        action="store_true",
        help="Serve built Fluxboard static assets at /tokenmm/* with SPA fallback.",
    )
    parser.add_argument(
        "--fluxboard-dist",
        type=Path,
        default=None,
        help="Path to Fluxboard dist directory (defaults to repo-root/fluxboard/dist).",
    )
    return parser.parse_args()


def _resolve_mode(config: dict[str, Any], args: argparse.Namespace) -> str:
    flux = _table(config, "flux")
    mode = str(args.mode or flux.get("mode", "paper")).strip().lower()
    if mode not in SAFE_MODES:
        raise ValueError(f"Invalid mode {mode!r}; expected one of {sorted(SAFE_MODES)}")
    if mode == "live" and not args.confirm_live:
        raise ValueError("Live mode requires explicit --confirm-live")
    return mode


def _build_contract_catalog(config: dict[str, Any]) -> tuple[ContractCatalogEntry, ...]:
    contracts = config.get("contracts", [])
    if not isinstance(contracts, list):
        raise ValueError("[[contracts]] must be a TOML array of tables")

    out: list[ContractCatalogEntry] = []
    for index, item in enumerate(contracts):
        if not isinstance(item, dict):
            raise ValueError(f"contracts[{index}] must be a table")
        exchange = _optional_text(item.get("exchange"))
        symbol = _optional_text(item.get("symbol"))
        if not exchange or not symbol:
            raise ValueError(f"contracts[{index}] requires non-empty exchange and symbol")
        out.append(ContractCatalogEntry(exchange=exchange, symbol=symbol))

    if not out:
        venues = _table(config, "venues")
        out.append(
            ContractCatalogEntry(
                exchange=str(venues.get("execution_venue", "bybit")).lower(),
                symbol=str(venues.get("execution_symbol", "PLUMEUSDT")).upper(),
            ),
        )
        out.append(
            ContractCatalogEntry(
                exchange=str(venues.get("reference_venue", "binance")).lower(),
                symbol=str(venues.get("reference_symbol", "PLUMEUSDT")).upper(),
            ),
        )

    deduped: dict[tuple[str, str], ContractCatalogEntry] = {}
    for contract in out:
        key = (contract.exchange.strip().lower(), contract.symbol.strip().upper())
        deduped[key] = ContractCatalogEntry(exchange=key[0], symbol=key[1])

    return tuple(deduped.values())


def _build_flux_config(config: dict[str, Any], *, mode: str, confirm_live: bool) -> FluxConfig:
    flux = _table(config, "flux")
    identity = _table(config, "identity")
    redis_cfg = _table(config, "redis")
    venues = _table(config, "venues")

    strategy_id = _optional_text(identity.get("strategy_id")) or "makerv3"

    flux_identity = FluxIdentityConfig(
        namespace=_optional_text(flux.get("namespace")) or FLUX_DEFAULT_NAMESPACE,
        schema_version=_optional_text(flux.get("schema_version")) or FLUX_SCHEMA_VERSION,
        strategy_id=strategy_id,
        strategy_instance_id=_optional_text(identity.get("strategy_instance_id")) or strategy_id,
        trader_id=_optional_text(identity.get("trader_id")) or "flux_api",
        external_strategy_id=_optional_text(identity.get("external_strategy_id")) or strategy_id,
    )

    flux_redis = FluxRedisConfig(
        host=str(redis_cfg.get("host", "127.0.0.1")),
        port=int(redis_cfg.get("port", 6380)),
        db=int(redis_cfg.get("db", 0)),
        username=_optional_text(redis_cfg.get("username")),
        password=_optional_text(redis_cfg.get("password")),
        connect_timeout_secs=float(redis_cfg.get("connect_timeout_secs", 5.0)),
        read_timeout_secs=float(redis_cfg.get("read_timeout_secs", 5.0)),
    )

    flux_venues = FluxVenuesConfig(
        execution_venue=str(venues.get("execution_venue", "BYBIT")),
        reference_venue=str(venues.get("reference_venue", "BINANCE")),
        execution_symbol=str(venues.get("execution_symbol", "PLUMEUSDT")),
        reference_symbol=str(venues.get("reference_symbol", "PLUMEUSDT")),
    )

    return FluxConfig(
        mode=mode,
        confirm_live=confirm_live,
        identity=flux_identity,
        redis=flux_redis,
        venues=flux_venues,
    )


def _resolve_bind_host(config: dict[str, Any], args: argparse.Namespace) -> str:
    api_cfg = _table(config, "api")
    return str(args.host or api_cfg.get("host", "127.0.0.1"))


def _env_flag(name: str, *, default: bool = False) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _resolve_fluxboard_dist_path(args: argparse.Namespace, api_cfg: dict[str, Any]) -> Path:
    if args.fluxboard_dist is not None:
        return args.fluxboard_dist
    env_path = _optional_text(os.getenv("FLUXBOARD_DIST"))
    if env_path:
        return Path(env_path)
    config_path = _optional_text(api_cfg.get("fluxboard_dist"))
    if config_path:
        return Path(config_path)
    return DEFAULT_FLUXBOARD_DIST


def _is_within(parent: Path, candidate: Path) -> bool:
    try:
        candidate.relative_to(parent)
    except ValueError:
        return False
    return True


def _attach_fluxboard_tokenmm_routes(app: Any, *, dist_dir: Path) -> None:
    dist_root = dist_dir.resolve()
    index_path = dist_root / "index.html"
    if not index_path.is_file():
        raise FileNotFoundError(f"Fluxboard index not found at {index_path}")

    def _serve_index() -> Any:
        return send_from_directory(str(dist_root), "index.html")

    @app.get(DEFAULT_TOKENMM_BASE_PATH)
    @app.get(f"{DEFAULT_TOKENMM_BASE_PATH}/")
    def _tokenmm_index() -> Any:
        return _serve_index()

    @app.get(f"{DEFAULT_TOKENMM_BASE_PATH}/assets/<path:asset_path>")
    def _tokenmm_assets(asset_path: str) -> Any:
        normalized = asset_path.strip().lstrip("/")
        candidate = (dist_root / "assets" / normalized).resolve()
        if not candidate.is_file() or not _is_within(dist_root, candidate):
            abort(404)
        return send_from_directory(str(dist_root / "assets"), normalized)

    @app.get(f"{DEFAULT_TOKENMM_BASE_PATH}/<path:subpath>")
    def _tokenmm_asset_or_spa(subpath: str) -> Any:
        normalized = subpath.strip().lstrip("/")
        candidate = (dist_root / normalized).resolve()
        if candidate.is_file() and _is_within(dist_root, candidate):
            return send_from_directory(str(dist_root), normalized)
        if normalized.startswith("assets/"):
            abort(404)
        return _serve_index()


def _run_with_socketio_if_available(app: Any, *, host: str, port: int) -> None:
    socket_server = app.extensions.get("flux_socket_server")
    socketio = getattr(socket_server, "socketio", None)
    if socketio is None:
        socketio = app.extensions.get("flux_socketio")

    if socketio is None:
        app.run(host=host, port=port, debug=False, use_reloader=False)
        return

    run_kwargs: dict[str, Any] = {
        "host": host,
        "port": port,
        "debug": False,
        "use_reloader": False,
    }
    try:
        socketio.run(app, **run_kwargs, allow_unsafe_werkzeug=True)
    except TypeError as e:
        # Older flask-socketio versions do not accept this kwarg.
        if "allow_unsafe_werkzeug" not in str(e):
            raise
        socketio.run(app, **run_kwargs)


def main() -> None:
    args = _parse_args()
    config = _load_config(args.config)
    mode = _resolve_mode(config, args)

    api_cfg = _table(config, "api")
    contracts = _build_contract_catalog(config)
    flux_config = _build_flux_config(
        config,
        mode=mode,
        confirm_live=(mode != "live" or args.confirm_live),
    )

    metadata = StrategyMetadata(
        strategy_class=str(api_cfg.get("strategy_class", "maker_v3")),
        strategy_groups=str(api_cfg.get("strategy_groups", "tokenmm")),
        base_asset=str(api_cfg.get("base_asset", "BASE")),
        quote_asset=str(api_cfg.get("quote_asset", "QUOTE")),
    )

    redis_client = redis.Redis(
        host=flux_config.redis.host,
        port=flux_config.redis.port,
        db=flux_config.redis.db,
        username=flux_config.redis.username,
        password=flux_config.redis.password,
        socket_connect_timeout=flux_config.redis.connect_timeout_secs,
        socket_timeout=flux_config.redis.read_timeout_secs,
        decode_responses=False,
    )

    app = create_flux_api_app(
        flux_config,
        cast(RedisClientProtocol, redis_client),
        contract_catalog=contracts,
        strategy_metadata=metadata,
    )

    serve_fluxboard = args.serve_fluxboard or _env_flag("FLUXBOARD_SERVE_DIST", default=False)
    if serve_fluxboard:
        dist_path = _resolve_fluxboard_dist_path(args, api_cfg)
        _attach_fluxboard_tokenmm_routes(app, dist_dir=dist_path)

    host = _resolve_bind_host(config, args)
    port = int(args.port or api_cfg.get("port", 5022))
    _run_with_socketio_if_available(app, host=host, port=port)


if __name__ == "__main__":
    main()
