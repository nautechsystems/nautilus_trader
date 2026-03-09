#!/usr/bin/env python3

from __future__ import annotations

import argparse
import configparser
import os
import tomllib
from collections.abc import Callable
from collections.abc import Mapping
from collections.abc import Sequence
from pathlib import Path
from typing import Any

from flask import abort
from flask import send_from_directory

from flux.pulse.api import PulseControlPlane
from lp.runners.run_hedger import get_redis_client


DEFAULT_BIND_HOST = "127.0.0.1"
DEFAULT_BIND_PORT = 5025
_VALID_JOB_ACTIONS = {"start", "stop", "restart"}


def _repo_root() -> Path:
    for candidate in Path(__file__).resolve().parents:
        if (candidate / "deploy").exists():
            return candidate
    return Path(__file__).resolve().parents[2]


DEFAULT_FLUXBOARD_DIST = _repo_root() / "fluxboard" / "dist"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Run hidden LP API backend.")
    parser.add_argument("--config", type=Path, required=True)
    parser.add_argument("--host", default=None)
    parser.add_argument("--port", type=int, default=None)
    parser.add_argument("--log-level", default=None)
    parser.add_argument(
        "--serve-fluxboard",
        action="store_true",
        help="Serve built Fluxboard static assets at /lp/* with SPA fallback.",
    )
    parser.add_argument(
        "--fluxboard-dist",
        type=Path,
        default=None,
        help="Path to Fluxboard dist directory (defaults to repo-root/fluxboard/dist).",
    )
    return parser


def _coerce_config_mapping(config: Mapping[str, Any] | None) -> dict[str, Any]:
    if config is None:
        return {}
    return dict(config)


def _table(config: Mapping[str, Any] | None, name: str) -> dict[str, Any]:
    data = _coerce_config_mapping(config)
    value = data.get(name, {})
    if not isinstance(value, Mapping):
        raise ValueError(f"[{name}] must be a table")
    return dict(value)


def parse_args(argv: Sequence[str] | None = None) -> argparse.Namespace:
    return _parser().parse_args(argv)


def load_config(path: Path) -> dict[str, Any]:
    if path.suffix.lower() == ".toml":
        with path.open("rb") as handle:
            data = tomllib.load(handle)
        if not isinstance(data, dict):
            raise ValueError(f"Config root must be a table: {path}")
        return data

    parser = configparser.ConfigParser()
    if not parser.read(path):
        raise FileNotFoundError(path)
    return {section: dict(parser[section].items()) for section in parser.sections()}


def resolve_bind_host(args: argparse.Namespace, config: Mapping[str, Any] | None) -> str:
    api_cfg = _table(config, "api")
    return str(getattr(args, "host", None) or api_cfg.get("host", DEFAULT_BIND_HOST)).strip() or DEFAULT_BIND_HOST


def resolve_bind_port(args: argparse.Namespace, config: Mapping[str, Any] | None) -> int:
    cli_port = getattr(args, "port", None)
    if cli_port is not None:
        return int(cli_port)

    api_cfg = _table(config, "api")
    configured_port = api_cfg.get("port")
    if configured_port in (None, ""):
        return DEFAULT_BIND_PORT
    return int(configured_port)


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _env_flag(name: str, *, default: bool = False) -> bool:
    value = os.getenv(name)
    if value is None:
        return default
    return value.strip().lower() in {"1", "true", "yes", "on"}


def _resolve_fluxboard_dist_path(args: argparse.Namespace, api_cfg: Mapping[str, Any] | None) -> Path:
    if args.fluxboard_dist is not None:
        return args.fluxboard_dist
    env_path = _optional_text(os.getenv("FLUXBOARD_DIST"))
    if env_path:
        return Path(env_path)
    configured_path = _optional_text(_table(api_cfg, "api").get("fluxboard_dist"))
    if configured_path:
        return Path(configured_path)
    return DEFAULT_FLUXBOARD_DIST


def _is_within(parent: Path, candidate: Path) -> bool:
    try:
        candidate.relative_to(parent)
    except ValueError:
        return False
    return True


def _attach_fluxboard_lp_routes(app: Any, *, dist_dir: Path) -> None:
    dist_root = dist_dir.resolve()
    index_path = dist_root / "index.html"
    if not index_path.is_file():
        raise FileNotFoundError(f"Fluxboard index not found at {index_path}")

    def _serve_index() -> Any:
        return send_from_directory(str(dist_root), "index.html")

    @app.get("/lp")
    @app.get("/lp/")
    def _lp_index() -> Any:
        return _serve_index()

    @app.get("/lp/assets/<path:asset_path>")
    def _lp_assets(asset_path: str) -> Any:
        normalized = asset_path.strip().lstrip("/")
        candidate = (dist_root / "assets" / normalized).resolve()
        if not candidate.is_file() or not _is_within(dist_root, candidate):
            abort(404)
        return send_from_directory(str(dist_root / "assets"), normalized)

    @app.get("/lp/<path:subpath>")
    def _lp_asset_or_spa(subpath: str) -> Any:
        normalized = subpath.strip().lstrip("/")
        candidate = (dist_root / normalized).resolve()
        if candidate.is_file() and _is_within(dist_root, candidate):
            return send_from_directory(str(dist_root), normalized)
        if normalized.startswith("assets/"):
            abort(404)
        return _serve_index()


def _resolve_job_status_reader(pulse: Any) -> Callable[[str], str]:
    if callable(getattr(pulse, "get_job_status", None)):
        return pulse.get_job_status

    def _status_reader(job_id: str) -> str:
        service = pulse._service_by_id(job_id)
        if service is None:
            return "unknown"
        payload = pulse._service_payload(service)
        return str(payload.get("status", "unknown"))

    return _status_reader


def _resolve_job_controller(
    pulse: Any,
    *,
    status_reader: Callable[[str], str],
) -> Callable[[str, str], str]:
    if callable(getattr(pulse, "control_job", None)):
        return pulse.control_job

    def _controller(job_id: str, action: str) -> str:
        if action not in _VALID_JOB_ACTIONS:
            raise ValueError(f"Unsupported action: {action}")
        service = pulse._service_by_id(job_id)
        if service is None:
            return "unknown"
        pulse._run_systemctl_action(job_id, action)
        return status_reader(job_id)

    return _controller


def create_app() -> Any:
    from lp.api import create_lp_api_app

    pulse = PulseControlPlane()
    status_reader = _resolve_job_status_reader(pulse)
    return create_lp_api_app(
        redis_client=get_redis_client(decode_responses=False),
        get_job_status=status_reader,
        control_job=_resolve_job_controller(pulse, status_reader=status_reader),
    )


def main(argv: Sequence[str] | None = None) -> None:
    args = parse_args(argv)
    config = load_config(args.config)
    api_cfg = _table(config, "api")
    app = create_app()
    if args.serve_fluxboard or _env_flag("FLUXBOARD_SERVE_DIST", default=False):
        _attach_fluxboard_lp_routes(app, dist_dir=_resolve_fluxboard_dist_path(args, api_cfg))
    app.run(
        host=resolve_bind_host(args, config),
        port=resolve_bind_port(args, config),
        debug=False,
        use_reloader=False,
    )


_parse_args = parse_args
_load_config = load_config
_resolve_bind_host = resolve_bind_host
_resolve_bind_port = resolve_bind_port


if __name__ == "__main__":
    main()
