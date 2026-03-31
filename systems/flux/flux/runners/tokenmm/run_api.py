#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import logging
import os
import time
import tomllib
import urllib.request as urllib_request
import uuid
from pathlib import Path
from typing import Any
from typing import Callable
from typing import Mapping
from typing import cast
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.parse import urlencode
from urllib.parse import urlsplit
from urllib.parse import urlunsplit

import redis
from flask import Response
from flask import abort
from flask import redirect
from flask import request
from flask import send_from_directory

from flux.api import ContractCatalogEntry
from flux.api import DEFAULT_PARAMS_DEFAULTS
from flux.api import DEFAULT_PARAMS_SCHEMA
from flux.api import FluxApiStore
from flux.api import StrategyMetadata
from flux.api import create_flux_api_app
from flux.api.app import RedisClientProtocol
from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.common.config import FluxConfig
from flux.common.config import FluxIdentityConfig
from flux.common.config import FluxRedisConfig
from flux.common.config import FluxVenuesConfig
from flux.common.config import validate_identifier_part
from flux.api.payloads import build_envelope
from flux.api.payloads import build_error
from flux.api.payloads import coerce_ts_ms
from flux.api.payloads import decode_text
from flux.api.payloads import now_ms
from flux.api.payloads import safe_bool
from flux.api.payloads import safe_int
from flux.pulse import PulseControlPlane
from flux.runners.shared.logging import configure_python_logging
from flux.runners.shared.logging import emit_startup_banner
from flux.runners.shared.strategy_set import build_profile_strategy_maps
from flux.runners.shared.strategy_set import build_profile_summary
from flux.runners.shared.strategy_set import get_strategy_set_descriptor
from flux.runners.shared.surface_proxy import SurfaceProxyDescriptor
from flux.runners.shared.surface_proxy import resolve_surface_backends
from flux.runners.shared.surface_proxy import resolve_surface_proxy_descriptor
from flux.runners.tokenmm.readiness import evaluate_tokenmm_readiness
from flux.runners.tokenmm.readiness import load_state_streams_by_strategy_id
from flux.runners.tokenmm.redis_runtime import apply_redis_env_overrides


SAFE_MODES = frozenset({"paper", "testnet", "live"})
DEFAULT_PULSE_BASE_PATH = "/pulse"
DEFAULT_FLUXBOARD_STATIC_BASE_PATH = "/static/fluxboard"
EQUITIES_PUBLIC_SOCKET_PATH = "/equities/socket.io"
TOKENMM_DESCRIPTOR = get_strategy_set_descriptor("tokenmm")
DEFAULT_TOKENMM_BASE_PATH = TOKENMM_DESCRIPTOR.base_path
TOKENMM_ALIAS_BASE_PATH = TOKENMM_DESCRIPTOR.route_aliases[0]
TOKENMM_READINESS_PATH = "/api/v1/readiness"
FLUXBOARD_SPA_BASE_PATHS: tuple[tuple[str, str], ...] = (
    (DEFAULT_TOKENMM_BASE_PATH, "tokenmm"),
    ("/lp", "lp"),
)
SURFACE_PROXY_DESCRIPTORS: tuple[SurfaceProxyDescriptor, ...] = (
    SurfaceProxyDescriptor(
        surface="equities",
        base_paths=("/equities",),
        backend_env_var="EQUITIES_API_BACKEND_URL",
        api_prefixes=("/api/v1", "/socket.io"),
        profile_names=("equities",),
    ),
    SurfaceProxyDescriptor(
        surface="lp",
        base_paths=("/api/v1/hedgers",),
        backend_env_var="LP_API_BACKEND_URL",
    ),
)


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
PROXY_TIMEOUT_SECS = 30.0
HOP_BY_HOP_HEADERS = {
    "connection",
    "content-length",
    "host",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailers",
    "transfer-encoding",
    "upgrade",
}
_LOG = logging.getLogger(__name__)


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _pulse_status_to_running(status: str) -> bool | None:
    normalized = _optional_text(status)
    if normalized is None:
        return None
    if normalized == "active":
        return True
    if normalized in {"inactive", "failed", "restarting", "stopping"}:
        return False
    return None


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
            for strategy_id in deduped_ids:
                status = pulse.get_job_status(f"tokenmm-node-{strategy_id}")
                next_cache[strategy_id] = _pulse_status_to_running(status)
            cached_running = next_cache
            cache_expires_at = time.monotonic() + ttl_s

        return {strategy_id: cached_running.get(strategy_id) for strategy_id in deduped_ids}

    return _resolve


def _signal_payload_to_active_alert_rows(
    signal_payload: Mapping[str, Any] | None,
    *,
    now_ms_value: int,
) -> list[dict[str, Any]]:
    if not isinstance(signal_payload, Mapping):
        return []

    strategy_id = _optional_text(signal_payload.get("id") or signal_payload.get("strategy_id"))
    if strategy_id is None:
        return []

    state = signal_payload.get("state")
    state_map = dict(state) if isinstance(state, Mapping) else {}
    params = signal_payload.get("params")
    params_map = dict(params) if isinstance(params, Mapping) else {}
    ts_ms = (
        coerce_ts_ms(
            signal_payload.get("ts_ms") or state_map.get("ts_ms") or state_map.get("ts_event"),
        )
        or now_ms_value
    )
    state_name = decode_text(state_map.get("state")).strip().lower()

    if safe_bool(signal_payload.get("blocked")) is not True:
        return []

    quote_health = state_map.get("quote_health")
    quote_health_map = dict(quote_health) if isinstance(quote_health, Mapping) else {}
    max_age_ms = safe_int(params_map.get("max_age_ms"))

    def _market_data_row(*, leg_role: str, default_reason_code: str) -> list[dict[str, Any]]:
        leg_health = quote_health_map.get(leg_role)
        leg_health_map = dict(leg_health) if isinstance(leg_health, Mapping) else {}
        reason_code = _optional_text(leg_health_map.get("reason_code")) or default_reason_code
        age_ms = safe_int(leg_health_map.get("quote_age_ms"))
        details: dict[str, Any] = {
            "strategy_id": strategy_id,
            "leg_role": leg_role,
        }
        if age_ms is not None:
            details["age_ms"] = age_ms
        if max_age_ms is not None:
            details["max_age_ms"] = max_age_ms
        message_parts = [
            f"Quoting blocked ({leg_role} data stale)",
            f"strategy_id={strategy_id}",
        ]
        if age_ms is not None:
            message_parts.append(f"age_ms={age_ms}")
        if max_age_ms is not None:
            message_parts.append(f"max_age_ms={max_age_ms}")
        row_id = f"active:{strategy_id}:{reason_code}"
        return [
            {
                "strategy_id": strategy_id,
                "row_id": row_id,
                "id": row_id,
                "alert_key": "market_data_blocked",
                "level": "warning",
                "code": reason_code,
                "reason_code": reason_code,
                "message": " ".join(message_parts),
                "details": details,
                "ts_ms": ts_ms,
                "source": "signals",
            },
        ]

    if state_name == "blocked_reference_md":
        return _market_data_row(
            leg_role="reference",
            default_reason_code="blocked_reference_md_stale",
        )
    if state_name == "blocked_maker_md":
        return _market_data_row(
            leg_role="maker",
            default_reason_code="blocked_maker_md_stale",
        )

    reason_code = _optional_text(signal_payload.get("reason")) or state_name or "blocked"
    row_id = f"active:{strategy_id}:{reason_code}"
    return [
        {
            "strategy_id": strategy_id,
            "row_id": row_id,
            "id": row_id,
            "alert_key": "strategy_blocked",
            "level": "warning",
            "code": reason_code,
            "reason_code": reason_code,
            "message": f"Strategy blocked ({reason_code.replace('_', ' ')}) strategy_id={strategy_id}",
            "details": {
                "strategy_id": strategy_id,
                "state": state_name or reason_code,
            },
            "ts_ms": ts_ms,
            "source": "signals",
        },
    ]


def _build_strategy_alerts_resolver(
    *,
    flux_config: FluxConfig | None = None,
    redis_client: RedisClientProtocol | None = None,
    contract_catalog: tuple[ContractCatalogEntry, ...] | None = None,
    strategy_metadata: StrategyMetadata | None = None,
    strategy_running_resolver: Callable[[list[str] | tuple[str, ...]], dict[str, bool | None]] | None = None,
    store: FluxApiStore | None = None,
    cache_ttl_s: float = 1.0,
    now_ms_fn: Callable[[], int] | None = None,
):
    if store is None:
        if (
            flux_config is None
            or redis_client is None
            or contract_catalog is None
            or strategy_metadata is None
        ):
            raise ValueError(
                "flux_config, redis_client, contract_catalog, and strategy_metadata are required",
            )
        store = FluxApiStore(
            flux_config=flux_config,
            redis_client=redis_client,
            contract_catalog=contract_catalog,
            strategy_running_resolver=strategy_running_resolver,
            strategy_alerts_resolver=None,
            alerts_include_history=True,
            params_schema=DEFAULT_PARAMS_SCHEMA,
            params_defaults=DEFAULT_PARAMS_DEFAULTS,
        )

    ttl_s = max(float(cache_ttl_s), 0.0)
    cached_rows: dict[str, list[dict[str, Any]]] = {}
    cache_expires_at = 0.0
    current_now_ms = now_ms_fn or now_ms
    fallback_metadata = strategy_metadata or StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups=TOKENMM_DESCRIPTOR.profile,
        base_asset="BASE",
        quote_asset="QUOTE",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )

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
            running_states = store.load_running_states(deduped_ids)
            now_ms_value = int(current_now_ms())
            for strategy_id in deduped_ids:
                try:
                    signal_payload = store.load_signals_payload(
                        strategy_id,
                        fallback_metadata,
                        running=running_states.get(strategy_id),
                    )
                except Exception:
                    _LOG.exception(
                        "TokenMM active alert resolver failed strategy_id=%s",
                        strategy_id,
                    )
                    next_cache[strategy_id] = []
                    continue
                next_cache[strategy_id] = _signal_payload_to_active_alert_rows(
                    signal_payload,
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
    parser = argparse.ArgumentParser(description="Run Flux API app for TokenMM.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--mode", choices=sorted(SAFE_MODES), default=None)
    parser.add_argument("--confirm-live", action="store_true")
    parser.add_argument("--log-level", default=None)
    parser.add_argument("--host", default=None)
    parser.add_argument("--port", type=int, default=None)
    parser.add_argument(
        "--serve-fluxboard",
        action="store_true",
        help="Serve Fluxboard SPA entry routes at /tokenmm and /lp, with shared static assets at /static/fluxboard/*.",
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
        descriptor=TOKENMM_DESCRIPTOR,
        validate_identifier=validate_identifier_part,
    )


def _tokenmm_profile_summary(
    profile_strategy_map: dict[str, list[str]],
    profile_required_strategy_map: dict[str, list[str]],
) -> str:
    return build_profile_summary(
        TOKENMM_DESCRIPTOR,
        profile_strategy_map,
        profile_required_strategy_map,
    )


def _env_flag(name: str, *, default: bool = False) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _resolve_surface_proxy_backends() -> dict[str, str]:
    return resolve_surface_backends(SURFACE_PROXY_DESCRIPTORS)


def _inject_fluxboard_runtime_config(
    html: str,
    *,
    socket_paths: dict[str, str] | None = None,
) -> str:
    runtime_config: dict[str, Any] = {}
    if socket_paths:
        runtime_config["socketPaths"] = socket_paths
    if not runtime_config:
        return html

    script = (
        "<script>"
        f"window.__FLUXBOARD_RUNTIME_CONFIG__={json.dumps(runtime_config, separators=(',', ':'))};"
        "</script>"
    )
    if "</head>" in html:
        return html.replace("</head>", f"{script}</head>", 1)
    if "</body>" in html:
        return html.replace("</body>", f"{script}</body>", 1)
    return f"{html}{script}"


def _maybe_inject_equities_runtime_config(response: Response) -> Response:
    content_type = (response.headers.get("Content-Type") or "").lower()
    if "text/html" not in content_type:
        return response
    html = response.get_data(as_text=True)
    response.set_data(
        _inject_fluxboard_runtime_config(
            html,
            socket_paths={"equities": EQUITIES_PUBLIC_SOCKET_PATH},
        ),
    )
    return response


def _proxy_request_to_backend(backend_url: str, *, path_override: str | None = None) -> Response:
    incoming = urlsplit(request.url)
    backend = urlsplit(backend_url)
    query = urlencode(list(request.args.items(multi=True)), doseq=True)
    target_url = urlunsplit(
        (
            backend.scheme,
            backend.netloc,
            path_override or request.path,
            query,
            "",
        ),
    )
    body = request.get_data(cache=True)
    headers = {
        key: value
        for key, value in request.headers.items()
        if key.lower() not in HOP_BY_HOP_HEADERS
    }
    headers["X-Forwarded-Proto"] = incoming.scheme
    headers["X-Forwarded-Host"] = incoming.netloc
    headers["X-Forwarded-For"] = request.remote_addr or "127.0.0.1"
    proxy_request = urllib_request.Request(
        target_url,
        data=body if body else None,
        headers=headers,
        method=request.method,
    )
    try:
        with urllib_request.urlopen(proxy_request, timeout=PROXY_TIMEOUT_SECS) as upstream:
            response = Response(upstream.read(), status=upstream.status)
            for key, value in upstream.headers.items():
                if key.lower() in HOP_BY_HOP_HEADERS:
                    continue
                response.headers[key] = value
            return response
    except HTTPError as exc:
        body = exc.read() if exc.fp is not None else b"Bad gateway"
        response = Response(body, status=exc.code)
        if exc.headers is not None:
            for key, value in exc.headers.items():
                if key.lower() in HOP_BY_HOP_HEADERS:
                    continue
                response.headers[key] = value
        return response
    except URLError:
        return Response("Bad gateway", status=502, content_type="text/plain")


def _attach_profile_router_proxy(app: Any, *, surface_backends: dict[str, str]) -> None:
    equities_backend = surface_backends.get("equities")

    if equities_backend:
        @app.route(EQUITIES_PUBLIC_SOCKET_PATH, methods=["GET", "POST", "OPTIONS"])
        @app.route(f"{EQUITIES_PUBLIC_SOCKET_PATH}/", defaults={"subpath": ""}, methods=["GET", "POST", "OPTIONS"])
        @app.route(f"{EQUITIES_PUBLIC_SOCKET_PATH}/<path:subpath>", methods=["GET", "POST", "OPTIONS"])
        def _proxy_equities_socketio(subpath: str = "") -> Response:
            normalized_subpath = subpath.strip().lstrip("/")
            suffix = f"/{normalized_subpath}" if normalized_subpath else "/"
            return _proxy_request_to_backend(
                equities_backend,
                path_override=f"/socket.io{suffix}",
            )

    @app.before_request
    def _proxy_surface_requests() -> Response | None:
        if equities_backend and (
            request.path == EQUITIES_PUBLIC_SOCKET_PATH
            or request.path.startswith(f"{EQUITIES_PUBLIC_SOCKET_PATH}/")
        ):
            normalized_subpath = request.path.removeprefix(EQUITIES_PUBLIC_SOCKET_PATH).strip().lstrip("/")
            suffix = f"/{normalized_subpath}" if normalized_subpath else "/"
            return _proxy_request_to_backend(
                equities_backend,
                path_override=f"/socket.io{suffix}",
            )

        descriptor = resolve_surface_proxy_descriptor(
            path=request.path,
            profile=_optional_text(request.args.get("profile")),
            descriptors=SURFACE_PROXY_DESCRIPTORS,
        )
        if descriptor is None:
            return None

        backend_url = surface_backends.get(descriptor.surface)
        if not backend_url:
            return None

        response = _proxy_request_to_backend(backend_url)
        if descriptor.surface == "equities" and request.path.startswith("/equities"):
            return _maybe_inject_equities_runtime_config(response)
        return response


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


def _tokenmm_profile_name_for_request() -> str:
    profile = (_optional_text(request.args.get("profile")) or TOKENMM_DESCRIPTOR.profile).lower()
    if profile not in {TOKENMM_DESCRIPTOR.profile, *TOKENMM_DESCRIPTOR.aliases}:
        raise ValueError(f"Unsupported TokenMM readiness profile: {profile}")
    return TOKENMM_DESCRIPTOR.profile


def _tokenmm_strategy_ids_for_request(
    *,
    profile_strategy_map: dict[str, list[str]],
    profile_required_strategy_map: dict[str, list[str]],
) -> tuple[tuple[str, ...], tuple[str, ...]]:
    requested_strategy_id = _optional_text(request.args.get("strategy"))
    if requested_strategy_id is not None:
        strategy_id = validate_identifier_part(requested_strategy_id, "strategy")
        return (strategy_id,), (strategy_id,)

    profile_name = _tokenmm_profile_name_for_request()
    strategy_ids = tuple(profile_strategy_map.get(profile_name, ()))
    if not strategy_ids:
        raise ValueError("TokenMM readiness requires at least one configured strategy id")
    required_strategy_ids = tuple(profile_required_strategy_map.get(profile_name, strategy_ids))
    return strategy_ids, required_strategy_ids


def _load_tokenmm_readiness(
    *,
    flux_config: FluxConfig,
    redis_client: RedisClientProtocol,
    contract_catalog: tuple[ContractCatalogEntry, ...],
    strategy_metadata: StrategyMetadata,
    profile_strategy_map: dict[str, list[str]],
    profile_required_strategy_map: dict[str, list[str]],
    strategy_running_resolver: Callable[[list[str] | tuple[str, ...]], dict[str, bool | None]] | None = None,
) -> dict[str, Any]:
    strategy_ids, required_strategy_ids = _tokenmm_strategy_ids_for_request(
        profile_strategy_map=profile_strategy_map,
        profile_required_strategy_map=profile_required_strategy_map,
    )
    running_resolver = strategy_running_resolver or _build_strategy_running_resolver()
    store = FluxApiStore(
        flux_config=flux_config,
        redis_client=redis_client,
        contract_catalog=contract_catalog,
        strategy_running_resolver=running_resolver,
        strategy_alerts_resolver=None,
        params_schema=DEFAULT_PARAMS_SCHEMA,
        params_defaults=DEFAULT_PARAMS_DEFAULTS,
    )
    running_states = store.load_running_states(strategy_ids)
    response_now_ms = now_ms()
    signals_payload = {
        "server_ts_ms": response_now_ms,
        "strategies": [
            store.load_signals_payload(
                strategy_id,
                strategy_metadata,
                running=running_states.get(strategy_id),
            )
            for strategy_id in strategy_ids
        ],
    }
    state_streams_by_strategy_id = load_state_streams_by_strategy_id(
        redis_client=redis_client,
        strategy_ids=strategy_ids,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
        environment=flux_config.mode,
        now_ms_value=response_now_ms,
    )
    return evaluate_tokenmm_readiness(
        required_strategy_ids=required_strategy_ids,
        signals_payload=signals_payload,
        state_streams_by_strategy_id=state_streams_by_strategy_id,
        now_ms_value=response_now_ms,
    ).as_dict()


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


def _should_enable_pulse_routes(args: argparse.Namespace, api_cfg: dict[str, Any]) -> bool:
    return bool(args.serve_pulse or api_cfg.get("enable_pulse_routes", False))


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

    app.add_url_rule(base_path, endpoint=f"{endpoint_prefix}_index", view_func=_serve_index, methods=["GET"])
    app.add_url_rule(f"{base_path}/", endpoint=f"{endpoint_prefix}_index_slash", view_func=_serve_index, methods=["GET"])
    app.add_url_rule(
        f"{base_path}/<path:subpath>",
        endpoint=f"{endpoint_prefix}_asset_or_spa",
        view_func=_serve_asset_or_spa,
        methods=["GET"],
    )


def _attach_fluxboard_tokenmm_routes(
    app: Any,
    *,
    dist_dir: Path,
    surface_backends: dict[str, str] | None = None,
) -> None:
    dist_root = dist_dir.resolve()
    index_path = dist_root / "index.html"
    if not index_path.is_file():
        raise FileNotFoundError(f"Fluxboard index not found at {index_path}")
    resolved_surface_backends = surface_backends or {}

    def _serve_shared_static(subpath: str) -> Any:
        normalized = subpath.strip().lstrip("/")
        candidate = (dist_root / normalized).resolve()
        if candidate.is_file() and _is_within(dist_root, candidate):
            return send_from_directory(str(dist_root), normalized)
        equities_backend = resolved_surface_backends.get("equities")
        if equities_backend:
            return _proxy_request_to_backend(equities_backend)
        abort(404)

    def _redirect_tokenm_alias(subpath: str | None = None) -> Any:
        target = DEFAULT_TOKENMM_BASE_PATH
        if subpath:
            target = f"{target}/{subpath.strip().lstrip('/')}"
        query = request.query_string.decode("utf-8").strip()
        if query:
            target = f"{target}?{query}"
        return redirect(target, code=302)

    @app.get(TOKENMM_ALIAS_BASE_PATH)
    @app.get(f"{TOKENMM_ALIAS_BASE_PATH}/")
    def _tokenm_alias_index() -> Any:
        return _redirect_tokenm_alias()

    @app.get(f"{TOKENMM_ALIAS_BASE_PATH}/<path:subpath>")
    def _tokenm_alias_subpath(subpath: str) -> Any:
        return _redirect_tokenm_alias(subpath)

    @app.get(f"{DEFAULT_FLUXBOARD_STATIC_BASE_PATH}/<path:subpath>")
    def _fluxboard_shared_static(subpath: str) -> Any:
        return _serve_shared_static(subpath)

    for base_path, endpoint_prefix in FLUXBOARD_SPA_BASE_PATHS:
        _register_fluxboard_spa_base_path(
            app,
            dist_root=dist_root,
            base_path=base_path,
            endpoint_prefix=f"fluxboard_{endpoint_prefix}",
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


def _attach_tokenmm_readiness_route(
    app: Any,
    *,
    readiness_loader: Callable[[], dict[str, Any]],
) -> None:
    @app.get(TOKENMM_READINESS_PATH)
    def _tokenmm_readiness() -> Response:
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
                    message="TokenMM readiness evaluation failed.",
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

    metadata = StrategyMetadata(
        strategy_class=str(api_cfg.get("strategy_class", "maker_v3")),
        strategy_groups=str(api_cfg.get("strategy_groups", TOKENMM_DESCRIPTOR.profile)),
        base_asset=str(api_cfg.get("base_asset", "BASE")),
        quote_asset=str(api_cfg.get("quote_asset", "QUOTE")),
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )
    profile_strategy_map, profile_required_strategy_map = _build_profile_strategy_maps(api_cfg)
    emit_startup_banner(
        prefix="tokenmm-run-api",
        message=_tokenmm_profile_summary(profile_strategy_map, profile_required_strategy_map),
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
    strategy_running_resolver = _build_strategy_running_resolver()
    strategy_alerts_resolver = _build_strategy_alerts_resolver(
        flux_config=flux_config,
        redis_client=cast(RedisClientProtocol, redis_client),
        contract_catalog=contracts,
        strategy_metadata=metadata,
        strategy_running_resolver=strategy_running_resolver,
    )

    app = create_flux_api_app(
        flux_config,
        cast(RedisClientProtocol, redis_client),
        contract_catalog=contracts,
        strategy_metadata=metadata,
        profile_strategy_map=profile_strategy_map or None,
        profile_required_strategy_map=profile_required_strategy_map or None,
        strategy_running_resolver=strategy_running_resolver,
        strategy_alerts_resolver=strategy_alerts_resolver,
        alerts_include_history=False,
        strategy_contracts=config.get("strategy_contracts"),
    )
    _attach_tokenmm_readiness_route(
        app,
        readiness_loader=lambda: _load_tokenmm_readiness(
            flux_config=flux_config,
            redis_client=cast(RedisClientProtocol, redis_client),
            contract_catalog=contracts,
            strategy_metadata=metadata,
            profile_strategy_map=profile_strategy_map,
            profile_required_strategy_map=profile_required_strategy_map,
            strategy_running_resolver=strategy_running_resolver,
        ),
    )
    if _should_enable_pulse_routes(args, api_cfg):
        PulseControlPlane().register_routes(app)
    surface_backends = _resolve_surface_proxy_backends()
    if surface_backends:
        _attach_profile_router_proxy(app, surface_backends=surface_backends)

    serve_fluxboard = args.serve_fluxboard or _env_flag("FLUXBOARD_SERVE_DIST", default=False)
    if serve_fluxboard:
        dist_path = _resolve_fluxboard_dist_path(args, api_cfg)
        _attach_fluxboard_tokenmm_routes(
            app,
            dist_dir=dist_path,
            surface_backends=surface_backends,
        )
    if args.serve_pulse:
        dist_path = _resolve_pulse_dist_path(args, api_cfg)
        _attach_pulse_routes(app, dist_dir=dist_path)

    host = _resolve_bind_host(config, args)
    port = int(args.port or api_cfg.get("port", 5022))
    _run_with_socketio_if_available(app, host=host, port=port)


if __name__ == "__main__":
    main()
