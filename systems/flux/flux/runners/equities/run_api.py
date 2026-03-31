#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import os
import time
import tomllib
import uuid
from datetime import UTC
from datetime import datetime
from pathlib import Path
from typing import Callable
from typing import Any
from typing import cast

import redis
from flask import Response
from flask import abort
from flask import request
from flask import send_from_directory

from flux.api import ContractCatalogEntry
from flux.api import StrategyMetadata
from flux.api import create_flux_api_app
from flux.api.app import RedisClientProtocol
from flux.api.payloads import build_envelope
from flux.api.payloads import build_error
from flux.api.payloads import now_ms
from flux.common.account_scopes import decode_account_scopes
from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.common.config import FluxConfig
from flux.common.config import FluxIdentityConfig
from flux.common.config import FluxRedisConfig
from flux.common.config import FluxVenuesConfig
from flux.common.config import validate_identifier_part
from flux.common.params import MAKERV3_RUNTIME_PARAM_DEFAULTS
from flux.common.params import MAKERV3_RUNTIME_PARAM_SCHEMA
from flux.common.strategy_contracts import decode_strategy_contracts
from flux.pulse import PulseControlPlane
from flux.runners.equities.readiness import EquitiesReadinessThresholds
from flux.runners.equities.readiness import _collect_component_payloads
from flux.runners.equities.readiness import _collect_publisher_status_payload
from flux.runners.equities.readiness import _collect_projection_payloads
from flux.runners.equities.readiness import _expected_reference_account_scope_id
from flux.runners.equities.readiness import _expected_projection_scope_ids
from flux.runners.equities.readiness import evaluate_equities_readiness
from flux.runners.shared.logging import configure_python_logging
from flux.runners.shared.logging import emit_startup_banner
from flux.runners.shared.portfolio_runner import parse_required_strategy_ids
from flux.runners.shared.portfolio_runner import parse_strategy_ids
from flux.runners.shared.strategy_set import build_profile_strategy_maps
from flux.runners.shared.strategy_set import build_profile_summary
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.equities.node_groups import derive_equities_node_group_id
from flux.runners.equities.redis_runtime import apply_redis_env_overrides
from flux.strategies import get_strategy_spec
from flux.strategies.equities_maker.runtime_params import (
    EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS,
)
from flux.strategies.equities_maker.runtime_params import (
    EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA,
)
from flux.strategies.equities_taker.runtime_params import (
    EQUITIES_TAKER_RUNTIME_PARAM_DEFAULTS,
)
from flux.strategies.equities_taker.runtime_params import (
    EQUITIES_TAKER_RUNTIME_PARAM_SCHEMA,
)
from flux.strategies.registry import FluxStrategySpec
from flux.strategies.registry import resolve_strategy_spec_for_strategy_id
from flux.strategies.makerv4.runtime_params import MAKERV4_RUNTIME_PARAM_DEFAULTS
from flux.strategies.makerv4.runtime_params import MAKERV4_RUNTIME_PARAM_SCHEMA


SAFE_MODES = frozenset({"paper", "testnet", "live"})
EQUITIES_DESCRIPTOR = get_strategy_set_descriptor("equities")
DEFAULT_EQUITIES_STRATEGY_SPEC = get_strategy_spec("makerv3")
DEFAULT_EQUITIES_BASE_PATH = EQUITIES_DESCRIPTOR.base_path
EQUITIES_ALIAS_BASE_PATH = (
    EQUITIES_DESCRIPTOR.route_aliases[0] if EQUITIES_DESCRIPTOR.route_aliases else None
)
DEFAULT_PULSE_BASE_PATH = "/pulse"
DEFAULT_FLUXBOARD_STATIC_BASE_PATH = "/static/fluxboard"
EQUITIES_READINESS_PATH = "/api/v1/readiness"


def _repo_root() -> Path:
    for module_path in (Path(__file__), Path(__file__).resolve()):
        candidate = module_path.parents[4]
        if candidate.name == "systems":
            candidate = candidate.parent
        if (candidate / "deploy").exists():
            return candidate
    return Path(__file__).resolve().parents[4]


DEFAULT_FLUXBOARD_DIST = _repo_root() / "fluxboard" / "dist"
DEFAULT_PULSE_DIST = _repo_root() / "pulse-ui" / "dist"


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _enveloped_json_response(
    *,
    ok: bool,
    data: Any,
    error: dict[str, Any] | None,
    status: int,
) -> Response:
    body = build_envelope(
        ok=ok,
        api_version=FLUX_SCHEMA_VERSION,
        request_id=uuid.uuid4().hex,
        timestamp_ms=now_ms(),
        data=data,
        error=error,
    )
    return Response(
        json.dumps(body, separators=(",", ":"), sort_keys=False, allow_nan=False),
        status=status,
        mimetype="application/json",
    )


def _equities_profile_name_for_request() -> str:
    profile = (_optional_text(request.args.get("profile")) or EQUITIES_DESCRIPTOR.profile).lower()
    if profile not in {EQUITIES_DESCRIPTOR.profile, *EQUITIES_DESCRIPTOR.aliases}:
        raise ValueError(f"Unsupported equities readiness profile: {profile}")
    return EQUITIES_DESCRIPTOR.profile


def _pulse_status_to_running(status: str) -> bool | None:
    normalized = _optional_text(status)
    if normalized is None:
        return None
    if normalized == "active":
        return True
    if normalized in {"inactive", "failed", "restarting", "stopping"}:
        return False
    return None


def _equities_node_job_id_for_strategy(strategy_id: str) -> str:
    strategy_text = _optional_text(strategy_id)
    if strategy_text is None:
        raise ValueError("strategy_id must be non-empty")
    try:
        return f"equities-node-{derive_equities_node_group_id(strategy_text)}"
    except ValueError:
        return f"equities-node-{strategy_text}"


def _equities_node_jobs_for_strategy_ids(
    strategy_ids: list[str] | tuple[str, ...],
) -> tuple[dict[str, str], list[str]]:
    job_id_by_strategy: dict[str, str] = {}
    ordered_job_ids: list[str] = []
    seen_job_ids: set[str] = set()
    for strategy_id in strategy_ids:
        job_id = _equities_node_job_id_for_strategy(strategy_id)
        job_id_by_strategy[strategy_id] = job_id
        if job_id in seen_job_ids:
            continue
        seen_job_ids.add(job_id)
        ordered_job_ids.append(job_id)
    return job_id_by_strategy, ordered_job_ids


def _build_strategy_running_resolver(
    *,
    pulse_control: PulseControlPlane | None = None,
    cache_ttl_s: float = 1.0,
):
    pulse = pulse_control or PulseControlPlane()
    ttl_s = max(float(cache_ttl_s), 0.0)
    cached_running: dict[str, bool | None] = {}
    cache_expires_at = 0.0

    def _resolve(strategy_ids: list[str] | tuple[str, ...]) -> dict[str, bool | None]:
        nonlocal cache_expires_at, cached_running

        deduped_ids: list[str] = []
        seen: set[str] = set()
        for strategy_id in strategy_ids:
            strategy_text = _optional_text(strategy_id)
            if strategy_text is None or strategy_text in seen:
                continue
            seen.add(strategy_text)
            deduped_ids.append(strategy_text)
        if not deduped_ids:
            return {}

        refresh_needed = time.monotonic() >= cache_expires_at or any(
            strategy_id not in cached_running for strategy_id in deduped_ids
        )
        if refresh_needed:
            next_cache = {} if time.monotonic() >= cache_expires_at else dict(cached_running)
            job_id_by_strategy, ordered_job_ids = _equities_node_jobs_for_strategy_ids(deduped_ids)
            status_by_job_id = {
                job_id: _pulse_status_to_running(pulse.get_job_status(job_id))
                for job_id in ordered_job_ids
            }
            for strategy_id in deduped_ids:
                next_cache[strategy_id] = status_by_job_id[job_id_by_strategy[strategy_id]]
            cached_running = next_cache
            cache_expires_at = time.monotonic() + ttl_s

        return {strategy_id: cached_running.get(strategy_id) for strategy_id in deduped_ids}

    return _resolve


def _iso_timestamp_to_ms(value: Any) -> int | None:
    text = _optional_text(value)
    if text is None:
        return None
    normalized = f"{text[:-1]}+00:00" if text.endswith("Z") else text
    try:
        parsed = datetime.fromisoformat(normalized)
    except ValueError:
        return None
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=UTC)
    return int(round(parsed.timestamp() * 1000))


def _pulse_snapshot_to_alert_rows(
    strategy_id: str,
    *,
    job_id: str,
    snapshot: dict[str, Any] | None,
    now_ms_value: int,
) -> list[dict[str, Any]]:
    if snapshot is None:
        ts_ms = now_ms_value
        row_id = f"pulse:{strategy_id}:pulse_job_unknown:{ts_ms}"
        return [
            {
                "strategy_id": strategy_id,
                "row_id": row_id,
                "id": row_id,
                "level": "ERROR",
                "message": "Pulse runner is not registered",
                "alert_key": "pulse_job_unknown",
                "ts_ms": ts_ms,
                "source": "pulse",
                "status": "unknown",
                "error_preview": None,
                "error_count": 0,
                "last_seen": None,
            },
        ]

    status = _optional_text(snapshot.get("status")) or "unknown"
    if status == "active":
        return []

    errors = snapshot.get("errors")
    errors_map = errors if isinstance(errors, dict) else {}
    error_preview = _optional_text(errors_map.get("preview"))
    error_count = int(errors_map.get("count") or 0)
    last_seen = _optional_text(errors_map.get("last_seen"))
    ts_ms = _iso_timestamp_to_ms(last_seen) or now_ms_value
    level = {
        "failed": "CRITICAL",
        "restarting": "WARNING",
        "stopping": "WARNING",
        "inactive": "ERROR",
        "unknown": "ERROR",
    }.get(status, "ERROR")
    base_message = {
        "failed": "Pulse runner failed",
        "restarting": "Pulse runner restarting",
        "stopping": "Pulse runner stopping",
        "inactive": "Pulse runner inactive",
        "unknown": "Pulse runner status unknown",
    }.get(status, "Pulse runner has an unexpected status")
    message = f"{base_message}: {error_preview}" if error_preview else base_message
    alert_key = f"pulse_job_{status}"
    row_id = f"pulse:{strategy_id}:{alert_key}:{ts_ms}"
    return [
        {
            "strategy_id": strategy_id,
            "row_id": row_id,
            "id": row_id,
            "level": level,
            "message": message,
            "alert_key": alert_key,
            "ts_ms": ts_ms,
            "source": "pulse",
            "status": status,
            "error_preview": error_preview,
            "error_count": error_count,
            "last_seen": last_seen,
        },
    ]


def _build_strategy_alerts_resolver(
    *,
    pulse_control: PulseControlPlane | None = None,
    cache_ttl_s: float = 1.0,
    now_ms_fn: Callable[[], int] | None = None,
):
    pulse = pulse_control or PulseControlPlane()
    ttl_s = max(float(cache_ttl_s), 0.0)
    cached_rows: dict[str, list[dict[str, Any]]] = {}
    cache_expires_at = 0.0
    current_now_ms = now_ms_fn or (lambda: int(time.time() * 1000))

    def _resolve(strategy_ids: list[str] | tuple[str, ...]) -> dict[str, list[dict[str, Any]]]:
        nonlocal cache_expires_at, cached_rows

        deduped_ids: list[str] = []
        seen: set[str] = set()
        for strategy_id in strategy_ids:
            strategy_text = _optional_text(strategy_id)
            if strategy_text is None or strategy_text in seen:
                continue
            seen.add(strategy_text)
            deduped_ids.append(strategy_text)
        if not deduped_ids:
            return {}

        refresh_needed = time.monotonic() >= cache_expires_at or any(
            strategy_id not in cached_rows for strategy_id in deduped_ids
        )
        if refresh_needed:
            next_cache = {} if time.monotonic() >= cache_expires_at else dict(cached_rows)
            now_ms_value = int(current_now_ms())
            job_id_by_strategy, ordered_job_ids = _equities_node_jobs_for_strategy_ids(deduped_ids)
            snapshot_by_job_id = {
                job_id: pulse.get_job_snapshot(job_id)
                for job_id in ordered_job_ids
            }
            for strategy_id in deduped_ids:
                job_id = job_id_by_strategy[strategy_id]
                snapshot = snapshot_by_job_id[job_id]
                next_cache[strategy_id] = _pulse_snapshot_to_alert_rows(
                    strategy_id,
                    job_id=job_id,
                    snapshot=snapshot,
                    now_ms_value=now_ms_value,
                )
            cached_rows = next_cache
            cache_expires_at = time.monotonic() + ttl_s

        return {strategy_id: list(cached_rows.get(strategy_id, ())) for strategy_id in deduped_ids}

    return _resolve


def _load_config(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"Config root must be a table: {path}")
    return apply_redis_env_overrides(data)


def _table(data: dict[str, Any], name: str) -> dict[str, Any]:
    value = data.get(name, {})
    if not isinstance(value, dict):
        raise ValueError(f"[{name}] must be a TOML table")
    return value


def _parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run Flux API app for Equities.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--log-level", default=None)
    parser.add_argument("--host", default=None)
    parser.add_argument("--port", type=int, default=None)
    parser.add_argument(
        "--serve-fluxboard",
        action="store_true",
        help=(
            "Serve built Fluxboard static assets at /static/fluxboard/* and "
            "the SPA entry route at /equities with SPA fallback."
        ),
    )
    parser.add_argument(
        "--fluxboard-dist",
        type=Path,
        default=None,
        help="Path to Fluxboard dist directory (defaults to repo-root/fluxboard/dist).",
    )
    parser.add_argument(
        "--serve-pulse",
        action="store_true",
        help="Serve built Pulse static assets at /pulse/* with SPA fallback.",
    )
    parser.add_argument(
        "--pulse-dist",
        type=Path,
        default=None,
        help="Path to Pulse dist directory (defaults to repo-root/pulse-ui/dist).",
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
        instrument_id = _optional_text(item.get("instrument_id")) or ""
        if not exchange or not symbol:
            raise ValueError(f"contracts[{index}] requires non-empty exchange and symbol")
        out.append(
            ContractCatalogEntry(
                exchange=exchange,
                symbol=symbol,
                instrument_id=instrument_id,
            ),
        )

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

    deduped: dict[tuple[str, str, str], ContractCatalogEntry] = {}
    for contract in out:
        exchange = contract.exchange.strip().lower()
        symbol = contract.symbol.strip().upper()
        instrument_id = contract.instrument_id.strip().upper()
        key = (exchange, symbol, instrument_id)
        deduped[key] = ContractCatalogEntry(
            exchange=exchange,
            symbol=symbol,
            instrument_id=instrument_id,
        )

    return tuple(deduped.values())


def _build_contract_catalog_by_strategy(
    config: dict[str, Any],
    *,
    contract_catalog: Sequence[ContractCatalogEntry],
) -> dict[str, tuple[ContractCatalogEntry, ...]]:
    contracts_by_instrument_id = {
        contract.instrument_id.strip().upper(): contract
        for contract in contract_catalog
        if contract.instrument_id.strip()
    }
    contracts_by_strategy_id: dict[str, tuple[ContractCatalogEntry, ...]] = {}
    for strategy_contract in decode_strategy_contracts(config.get("strategy_contracts") or []):
        per_strategy: list[ContractCatalogEntry] = []
        seen: set[tuple[str, str, str]] = set()
        for instrument_id in (
            strategy_contract.maker_instrument_id,
            strategy_contract.reference_instrument_id,
        ):
            contract = contracts_by_instrument_id.get(instrument_id.strip().upper())
            if contract is None:
                raise ValueError(
                    "Missing contract catalog entry for "
                    f"{strategy_contract.strategy_id!r} instrument {instrument_id!r}",
                )
            key = (
                contract.exchange.strip().lower(),
                contract.symbol.strip().upper(),
                contract.instrument_id.strip().upper(),
            )
            if key in seen:
                continue
            seen.add(key)
            per_strategy.append(contract)
        contracts_by_strategy_id[strategy_contract.strategy_id] = tuple(per_strategy)
    return contracts_by_strategy_id


def _build_flux_config(config: dict[str, Any], *, mode: str, confirm_live: bool) -> FluxConfig:
    flux = _table(config, "flux")
    identity = _table(config, "identity")
    redis_cfg = _table(config, "redis")
    venues = _table(config, "venues")

    strategy_id = _optional_text(identity.get("strategy_id")) or "equities_api"

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
        ssl=bool(redis_cfg.get("ssl", False)),
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
    return str(args.host or api_cfg.get("host", "127.0.0.1")).strip() or "127.0.0.1"


def _build_profile_strategy_maps(
    api_cfg: dict[str, Any],
) -> tuple[dict[str, list[str]], dict[str, list[str]]]:
    return build_profile_strategy_maps(
        api_cfg,
        descriptor=EQUITIES_DESCRIPTOR,
        validate_identifier=validate_identifier_part,
    )


def _equities_profile_summary(
    profile_strategy_map: dict[str, list[str]],
    profile_required_strategy_map: dict[str, list[str]],
) -> str:
    return build_profile_summary(
        EQUITIES_DESCRIPTOR,
        profile_strategy_map,
        profile_required_strategy_map,
    )


def _resolve_strategy_name(api_cfg: dict[str, Any]) -> str:
    strategy_class = (_optional_text(api_cfg.get("strategy_class")) or "").lower()
    if not strategy_class:
        raise ValueError("`api.strategy_class` must be set explicitly for equities")
    strategy_name = {
        "maker_v4": "makerv4",
        "makerv4": "makerv4",
        "maker_v3": DEFAULT_EQUITIES_STRATEGY_SPEC.strategy_id,
        "makerv3": DEFAULT_EQUITIES_STRATEGY_SPEC.strategy_id,
    }.get(strategy_class)
    if strategy_name is None:
        try:
            strategy_name = get_strategy_spec(strategy_class).strategy_id
        except ValueError as exc:
            raise ValueError(
                "`api.strategy_class` must be one of {'maker_v4', 'makerv4', "
                "'maker_v3', 'makerv3', 'equities_maker', 'equities_taker'} "
                f"for equities, got {strategy_class!r}",
            ) from exc

    explicit_param_set = _optional_text(api_cfg.get("param_set"))
    expected_param_set = get_strategy_spec(strategy_name).param_set
    if explicit_param_set and explicit_param_set != expected_param_set:
        raise ValueError(
            f"`api.param_set` drift for equities: expected {expected_param_set!r}, got {explicit_param_set!r}",
        )
    return strategy_name


def _build_strategy_metadata(
    api_cfg: dict[str, Any],
    *,
    strategy_spec: FluxStrategySpec,
    base_asset: str | None = None,
) -> StrategyMetadata:
    return StrategyMetadata(
        strategy_class=str(strategy_spec.profile_key),
        strategy_groups=str(api_cfg.get("strategy_groups", EQUITIES_DESCRIPTOR.profile)),
        base_asset=str(base_asset or api_cfg.get("base_asset", "BASE")),
        quote_asset=str(api_cfg.get("quote_asset", "QUOTE")),
        param_set=strategy_spec.param_set,
        strategy_family=strategy_spec.strategy_family,
        strategy_version=strategy_spec.strategy_version,
    )


def build_equities_strategy_metadata_map(
    api_cfg: dict[str, Any],
    *,
    strategy_ids: list[str],
    strategy_contracts: Any = None,
) -> dict[str, StrategyMetadata]:
    configured_strategy_class = _optional_text(api_cfg.get("strategy_class"))
    default_strategy_spec = (
        get_strategy_spec(_resolve_strategy_name(api_cfg))
        if configured_strategy_class
        else DEFAULT_EQUITIES_STRATEGY_SPEC
    )
    raw_strategy_contracts = strategy_contracts
    if raw_strategy_contracts is None:
        raw_strategy_contracts = api_cfg.get("strategy_contracts")
    contracts_by_strategy_id = {
        contract.strategy_id: contract
        for contract in decode_strategy_contracts(raw_strategy_contracts or [])
    }
    metadata_by_strategy_id: dict[str, StrategyMetadata] = {}
    for strategy_id in strategy_ids:
        contract = contracts_by_strategy_id.get(strategy_id)
        strategy_spec = resolve_strategy_spec_for_strategy_id(
            strategy_id,
            default=default_strategy_spec,
        )
        metadata_by_strategy_id[strategy_id] = _build_strategy_metadata(
            api_cfg,
            strategy_spec=strategy_spec,
            base_asset=(contract.portfolio_asset_id if contract is not None else None),
        )
    return metadata_by_strategy_id


def build_strategy_metadata_for_test(strategy_name: str) -> StrategyMetadata:
    return _build_strategy_metadata({}, strategy_spec=get_strategy_spec(strategy_name))


def _resolve_runtime_params_payloads(
    strategy_name: str,
) -> tuple[dict[str, dict[str, Any]], dict[str, Any]]:
    if strategy_name == "makerv4":
        return MAKERV4_RUNTIME_PARAM_SCHEMA, MAKERV4_RUNTIME_PARAM_DEFAULTS
    if strategy_name == "equities_maker":
        return EQUITIES_MAKER_RUNTIME_PARAM_SCHEMA, EQUITIES_MAKER_RUNTIME_PARAM_DEFAULTS
    if strategy_name == "equities_taker":
        return EQUITIES_TAKER_RUNTIME_PARAM_SCHEMA, EQUITIES_TAKER_RUNTIME_PARAM_DEFAULTS
    defaults = dict(MAKERV3_RUNTIME_PARAM_DEFAULTS)
    # Equities canaries use 1-share order sizing by default; keep the API fallback aligned with node runtime.
    defaults["qty"] = 1.0
    return MAKERV3_RUNTIME_PARAM_SCHEMA, defaults


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


def _resolve_pulse_dist_path(args: argparse.Namespace, api_cfg: dict[str, Any]) -> Path:
    if args.pulse_dist is not None:
        return args.pulse_dist
    env_path = _optional_text(os.getenv("PULSE_DIST"))
    if env_path:
        return Path(env_path)
    config_path = _optional_text(api_cfg.get("pulse_dist"))
    if config_path:
        return Path(config_path)
    return DEFAULT_PULSE_DIST


def _is_within(parent: Path, candidate: Path) -> bool:
    try:
        candidate.relative_to(parent)
    except ValueError:
        return False
    return True


def _register_fluxboard_spa_base_path(
    app: Any,
    *,
    dist_root: Path,
    base_path: str,
    endpoint_prefix: str,
) -> None:
    def _serve_index() -> Any:
        return send_from_directory(str(dist_root), "index.html")

    def _serve_asset_or_spa(subpath: str) -> Any:
        normalized = subpath.strip().lstrip("/")
        if normalized.startswith("assets/"):
            abort(404)
        return _serve_index()

    app.add_url_rule(
        base_path,
        endpoint=f"{endpoint_prefix}_index",
        view_func=_serve_index,
        methods=["GET"],
    )
    app.add_url_rule(
        f"{base_path}/",
        endpoint=f"{endpoint_prefix}_index_slash",
        view_func=_serve_index,
        methods=["GET"],
    )
    app.add_url_rule(
        f"{base_path}/<path:subpath>",
        endpoint=f"{endpoint_prefix}_asset_or_spa",
        view_func=_serve_asset_or_spa,
        methods=["GET"],
    )


def _attach_fluxboard_equities_routes(app: Any, *, dist_dir: Path) -> None:
    dist_root = dist_dir.resolve()
    index_path = dist_root / "index.html"
    if not index_path.is_file():
        raise FileNotFoundError(f"Fluxboard index not found at {index_path}")

    def _serve_shared_static(subpath: str) -> Any:
        normalized = subpath.strip().lstrip("/")
        candidate = (dist_root / normalized).resolve()
        if not candidate.is_file() or not _is_within(dist_root, candidate):
            abort(404)
        return send_from_directory(str(dist_root), normalized)

    if EQUITIES_ALIAS_BASE_PATH:
        @app.get(EQUITIES_ALIAS_BASE_PATH)
        @app.get(f"{EQUITIES_ALIAS_BASE_PATH}/")
        def _tokenm_alias_index() -> Any:
            abort(404)

        @app.get(f"{EQUITIES_ALIAS_BASE_PATH}/<path:subpath>")
        def _tokenm_alias_subpath(subpath: str) -> Any:
            _ = subpath
            abort(404)

    @app.get(f"{DEFAULT_FLUXBOARD_STATIC_BASE_PATH}/<path:subpath>")
    def _fluxboard_shared_static(subpath: str) -> Any:
        return _serve_shared_static(subpath)

    _register_fluxboard_spa_base_path(
        app,
        dist_root=dist_root,
        base_path=DEFAULT_EQUITIES_BASE_PATH,
        endpoint_prefix="fluxboard_equities",
    )


def _attach_pulse_routes(app: Any, *, dist_dir: Path) -> None:
    dist_root = dist_dir.resolve()
    index_path = dist_root / "index.html"
    if not index_path.is_file():
        raise FileNotFoundError(f"Pulse index not found at {index_path}")

    def _serve_index() -> Any:
        return send_from_directory(str(dist_root), "index.html")

    @app.get(DEFAULT_PULSE_BASE_PATH)
    @app.get(f"{DEFAULT_PULSE_BASE_PATH}/")
    def _pulse_index() -> Any:
        return _serve_index()

    @app.get(f"{DEFAULT_PULSE_BASE_PATH}/assets/<path:asset_path>")
    def _pulse_assets(asset_path: str) -> Any:
        normalized = asset_path.strip().lstrip("/")
        candidate = (dist_root / "assets" / normalized).resolve()
        if not candidate.is_file() or not _is_within(dist_root, candidate):
            abort(404)
        return send_from_directory(str(dist_root / "assets"), normalized)

    @app.get(f"{DEFAULT_PULSE_BASE_PATH}/<path:subpath>")
    def _pulse_asset_or_spa(subpath: str) -> Any:
        normalized = subpath.strip().lstrip("/")
        candidate = (dist_root / normalized).resolve()
        if candidate.is_file() and _is_within(dist_root, candidate):
            return send_from_directory(str(dist_root), normalized)
        if normalized.startswith("assets/"):
            abort(404)
        return _serve_index()


def _load_equities_readiness(
    *,
    app: Any,
    config: dict[str, Any],
    redis_client: RedisClientProtocol,
) -> dict[str, Any]:
    profile_id = _equities_profile_name_for_request()
    api_cfg = _table(config, "api")
    portfolio_cfg = _table(config, "portfolio")
    flux_cfg = _table(config, "flux")
    publisher_cfg = _table(config, "ibkr_reference_publisher")
    strategy_ids = parse_strategy_ids(api_cfg, descriptor=EQUITIES_DESCRIPTOR)
    required_strategy_ids = tuple(
        parse_required_strategy_ids(
            api_cfg,
            descriptor=EQUITIES_DESCRIPTOR,
            fallback=strategy_ids,
        ),
    )
    strategy_id_set = set(strategy_ids)
    strategy_contracts = tuple(
        contract
        for contract in decode_strategy_contracts(config.get("strategy_contracts") or [])
        if contract.strategy_id in strategy_id_set
    )
    account_scopes = decode_account_scopes(config.get("account_scopes") or [])
    portfolio_id = (
        _optional_text(portfolio_cfg.get("portfolio_id"))
        or EQUITIES_DESCRIPTOR.default_portfolio_id
    )
    thresholds = EquitiesReadinessThresholds(
        ignore_reference_freshness_outside_regular_session=True,
    )
    namespace = _optional_text(flux_cfg.get("namespace")) or FLUX_DEFAULT_NAMESPACE
    schema_version = _optional_text(flux_cfg.get("schema_version")) or FLUX_SCHEMA_VERSION
    publisher_service_id = (
        _optional_text(publisher_cfg.get("service_id"))
        or "ibkr_reference_publisher"
    )
    publisher_account_scope_id = (
        _optional_text(publisher_cfg.get("account_scope_id"))
        or _expected_reference_account_scope_id(strategy_contracts)
    )
    expected_scope_ids = _expected_projection_scope_ids(
        strategy_contracts=strategy_contracts,
        account_scopes=account_scopes,
        overrides=thresholds.expected_projection_scope_ids,
    )
    projection_payloads = _collect_projection_payloads(
        redis_client=redis_client,
        profile_id=profile_id,
        scope_ids=expected_scope_ids,
        namespace=namespace,
        schema_version=schema_version,
    )
    component_payloads = _collect_component_payloads(
        redis_client=redis_client,
        strategy_contracts=strategy_contracts,
        portfolio_id=portfolio_id,
        namespace=namespace,
        schema_version=schema_version,
    )
    publisher_status_payload = _collect_publisher_status_payload(
        redis_client=redis_client,
        profile_id=profile_id,
        account_scope_id=publisher_account_scope_id,
        service_id=publisher_service_id,
        namespace=namespace,
        schema_version=schema_version,
    )
    with app.test_client() as client:
        balances_response = client.get("/api/v1/balances", query_string={"profile": profile_id})
        signals_response = client.get(
            "/api/v1/signals",
            query_string={"contract_version": 2, "profile": profile_id},
        )
    if balances_response.status_code != 200:
        raise RuntimeError("Equities balances snapshot is unavailable for readiness evaluation")
    if signals_response.status_code != 200:
        raise RuntimeError("Equities signals snapshot is unavailable for readiness evaluation")
    balances_payload = (balances_response.get_json(silent=True) or {}).get("data")
    signals_payload = (signals_response.get_json(silent=True) or {}).get("data")
    result = evaluate_equities_readiness(
        profile_id=profile_id,
        portfolio_id=portfolio_id,
        strategy_contracts=strategy_contracts,
        account_scopes=account_scopes,
        required_strategy_ids=required_strategy_ids,
        balances_payload=balances_payload if isinstance(balances_payload, dict) else None,
        signals_payload=signals_payload if isinstance(signals_payload, dict) else None,
        projection_payloads_by_scope_id=projection_payloads,
        component_payloads_by_strategy_id=component_payloads,
        publisher_status_payload=publisher_status_payload,
        now_ms_value=now_ms(),
        require_ibkr_reference_publisher=True,
        ibkr_reference_publisher_service_id=publisher_service_id,
        ibkr_reference_publisher_account_scope_id=publisher_account_scope_id,
        thresholds=thresholds,
    )
    return result.as_dict()


def _attach_equities_readiness_route(
    app: Any,
    *,
    readiness_loader: Callable[[], dict[str, Any]],
) -> None:
    @app.get(EQUITIES_READINESS_PATH)
    def _equities_readiness() -> Response:
        try:
            payload = readiness_loader()
        except ValueError as exc:
            return _enveloped_json_response(
                ok=False,
                data=None,
                error=build_error(
                    code="invalid_readiness_request",
                    message=str(exc),
                ),
                status=400,
            )
        except redis.RedisError as exc:
            return _enveloped_json_response(
                ok=False,
                data=None,
                error=build_error(
                    code="store_unavailable",
                    message="Data store unavailable.",
                    details={"error_type": type(exc).__name__},
                ),
                status=503,
            )
        except Exception as exc:
            return _enveloped_json_response(
                ok=False,
                data=None,
                error=build_error(
                    code="readiness_probe_failed",
                    message="Equities readiness evaluation failed.",
                    details={"error_type": type(exc).__name__},
                ),
                status=500,
            )

        return _enveloped_json_response(ok=True, data=payload, error=None, status=200)


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
    configure_python_logging(
        cli_level=args.log_level,
        config_level=api_cfg.get("log_level", "INFO"),
        service_env_var="FLUX_API_LOG_LEVEL",
    )
    contracts = _build_contract_catalog(config)
    flux_config = _build_flux_config(
        config,
        mode=mode,
        confirm_live=(mode != "live" or args.confirm_live),
    )
    strategy_name = _resolve_strategy_name(api_cfg)
    strategy_spec = get_strategy_spec(strategy_name)
    params_schema, params_defaults = _resolve_runtime_params_payloads(strategy_name)

    metadata = _build_strategy_metadata(
        api_cfg,
        strategy_spec=strategy_spec,
    )
    profile_strategy_map, profile_required_strategy_map = _build_profile_strategy_maps(api_cfg)
    strategy_metadata_map = build_equities_strategy_metadata_map(
        api_cfg,
        strategy_ids=profile_strategy_map.get(EQUITIES_DESCRIPTOR.profile, []),
        strategy_contracts=config.get("strategy_contracts"),
    )
    contract_catalog_by_strategy = _build_contract_catalog_by_strategy(
        config,
        contract_catalog=contracts,
    )
    emit_startup_banner(
        prefix="equities-run-api",
        message=_equities_profile_summary(profile_strategy_map, profile_required_strategy_map),
    )

    redis_client = redis.Redis(
        host=flux_config.redis.host,
        port=flux_config.redis.port,
        db=flux_config.redis.db,
        username=flux_config.redis.username,
        password=flux_config.redis.password,
        ssl=flux_config.redis.ssl,
        socket_connect_timeout=flux_config.redis.connect_timeout_secs,
        socket_timeout=flux_config.redis.read_timeout_secs,
        decode_responses=False,
    )

    app = create_flux_api_app(
        flux_config,
        cast(RedisClientProtocol, redis_client),
        contract_catalog=contracts,
        contract_catalog_resolver=lambda strategy_id: contract_catalog_by_strategy.get(
            strategy_id,
            contracts,
        ),
        strategy_running_resolver=_build_strategy_running_resolver(),
        strategy_alerts_resolver=_build_strategy_alerts_resolver(),
        strategy_metadata=metadata,
        strategy_metadata_resolver=strategy_metadata_map.__getitem__,
        profile_strategy_map=profile_strategy_map or None,
        profile_required_strategy_map=profile_required_strategy_map or None,
        strategy_contracts=config.get("strategy_contracts"),
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set=strategy_spec.param_set,
    )
    _attach_equities_readiness_route(
        app,
        readiness_loader=lambda: _load_equities_readiness(
            app=app,
            config=config,
            redis_client=cast(RedisClientProtocol, redis_client),
        ),
    )
    PulseControlPlane().register_routes(app)

    serve_fluxboard = args.serve_fluxboard or _env_flag("FLUXBOARD_SERVE_DIST", default=False)
    if serve_fluxboard:
        dist_path = _resolve_fluxboard_dist_path(args, api_cfg)
        _attach_fluxboard_equities_routes(app, dist_dir=dist_path)
    serve_pulse = args.serve_pulse or _env_flag("PULSE_SERVE_DIST", default=False)
    if serve_pulse:
        dist_path = _resolve_pulse_dist_path(args, api_cfg)
        _attach_pulse_routes(app, dist_dir=dist_path)

    host = _resolve_bind_host(config, args)
    port = int(args.port or api_cfg.get("port", 5022))
    _run_with_socketio_if_available(app, host=host, port=port)


if __name__ == "__main__":
    main()
