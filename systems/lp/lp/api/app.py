from __future__ import annotations

import configparser
import json
import os
from collections.abc import Callable
from decimal import Decimal
from pathlib import Path
from typing import Any
from typing import Protocol

from flask import Flask
from flask import jsonify
from flask import request

from lp.config import LpHedgerConfig
from lp.config import load_lp_hedger_config
from lp.hedgers import LpHedgerMeta
from lp.hedgers import get_hedger_meta
from lp.hedgers import list_hedgers


class RedisClientProtocol(Protocol):
    def delete(self, key: str) -> int | None: ...

    def get(self, key: str) -> Any: ...

    def lpush(self, key: str, value: str) -> int: ...

    def lrange(self, key: str, start: int, end: int) -> list[Any]: ...

    def ltrim(self, key: str, start: int, end: int) -> None: ...

    def set(self, key: str, value: str) -> None: ...


class _NullRedis:
    def delete(self, key: str) -> int:
        return 0

    def get(self, key: str) -> None:
        return None

    def lpush(self, key: str, value: str) -> int:
        return 0

    def lrange(self, key: str, start: int, end: int) -> list[Any]:
        return []

    def ltrim(self, key: str, start: int, end: int) -> None:
        return None

    def set(self, key: str, value: str) -> None:
        return None


def _decode_json(raw: Any) -> dict[str, Any] | None:
    if raw is None:
        return None
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8")
    try:
        parsed = json.loads(raw)
    except Exception:
        return None
    return parsed if isinstance(parsed, dict) else None


def _decode_event(raw: Any) -> dict[str, Any] | None:
    if raw is None:
        return None
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8")
    try:
        parsed = json.loads(raw)
    except Exception:
        return None
    return parsed if isinstance(parsed, dict) else None


def _mode_flag(payload: dict[str, Any] | None, field: str, *, default: bool = False) -> bool:
    if not isinstance(payload, dict):
        return default
    value = payload.get(field)
    if isinstance(value, bool):
        return value
    if isinstance(value, int):
        return bool(value)
    if isinstance(value, str):
        return value.strip().lower() in {"1", "true", "yes", "y", "on"}
    return default


def _config_path_for_meta(meta: LpHedgerMeta) -> Path:
    return Path(os.getenv(meta.config_env_var, meta.config_default_path))


def _load_config_for_meta(meta: LpHedgerMeta) -> LpHedgerConfig:
    return load_lp_hedger_config(_config_path_for_meta(meta))


def _read_config_parser(path: Path) -> configparser.ConfigParser:
    parser = configparser.ConfigParser()
    if not parser.read(path):
        raise FileNotFoundError(path)
    return parser


def _ensure_section(parser: configparser.ConfigParser, name: str) -> configparser.SectionProxy:
    if not parser.has_section(name):
        parser.add_section(name)
    return parser[name]


def _build_config_summary(config: LpHedgerConfig) -> dict[str, Any]:
    return {
        "id": config.hedger_id,
        "label": config.label,
        "job_id": config.job_id,
        "state_key": config.state_key,
        "pool_address": config.pool_address,
        "token0_symbol": config.token0_symbol,
        "token1_symbol": config.token1_symbol,
        "token0_decimals": config.token0_decimals,
        "token1_decimals": config.token1_decimals,
        "initial_eth": str(config.initial_eth),
        "initial_plume": str(config.initial_plume),
        "initial_token0": str(config.initial_token0),
        "initial_token1": str(config.initial_token1),
        "price_lower": str(config.price_lower),
        "price_upper": str(config.price_upper),
        "target_net_eth": str(config.target_net_eth),
        "target_net_plume": str(config.target_net_plume),
        "target_net_token0": str(config.target_net_token0),
        "target_net_token1": str(config.target_net_token1),
        "perp_symbol_token0": config.perp_symbol_token0,
        "perp_symbol_token1": config.perp_symbol_token1,
        "eth_symbol": config.eth_symbol,
        "plume_symbol": config.plume_symbol,
        "qty_step_eth": str(config.eth_qty_step),
        "qty_step_plume": str(config.plume_qty_step),
        "price_move_pct": str(config.price_move_pct),
        "eth_exposure_usd_threshold": str(config.eth_exposure_usd_threshold),
        "plume_exposure_usd_threshold": str(config.plume_exposure_usd_threshold),
        "min_order_qty_eth": str(config.min_order_qty_eth),
        "min_order_qty_plume": str(config.min_order_qty_plume),
        "api_key_hint": config.api_key_hint,
        "hedge_token0": bool(config.hedge_token0),
        "hedge_token1": bool(config.hedge_token1),
    }


def _serialize_instance_meta(
    meta: LpHedgerMeta,
    *,
    config_summary: dict[str, Any] | None,
) -> dict[str, Any]:
    payload = {
        "id": meta.id,
        "job_id": meta.job_id,
        "state_key": meta.state_key,
        "config_env_var": meta.config_env_var,
        "config_default_path": meta.config_default_path,
    }
    if config_summary:
        payload.update(
            {
                "label": config_summary.get("label"),
                "token0_symbol": config_summary.get("token0_symbol"),
                "token1_symbol": config_summary.get("token1_symbol"),
                "api_key_hint": config_summary.get("api_key_hint"),
            },
        )
    return payload


def _serialize_config(config: LpHedgerConfig) -> dict[str, Any]:
    return {
        "id": config.hedger_id,
        "label": config.label,
        "lp_pool": {
            "pool_address": config.pool_address,
            "mode": config.lp_mode,
            "token0_symbol": config.token0_symbol,
            "token1_symbol": config.token1_symbol,
            "token0_decimals": config.token0_decimals,
            "token1_decimals": config.token1_decimals,
            "initial_token0": str(config.initial_token0),
            "initial_token1": str(config.initial_token1),
            "price_lower": str(config.price_lower),
            "price_upper": str(config.price_upper),
        },
        "target": {
            "target_net_token0": str(config.target_net_token0),
            "target_net_token1": str(config.target_net_token1),
        },
        "hedge": {
            "hedge_token0": bool(config.hedge_token0),
            "hedge_token1": bool(config.hedge_token1),
        },
        "bybit": {
            "perp_symbol_token0": config.perp_symbol_token0,
            "perp_symbol_token1": config.perp_symbol_token1,
        },
    }


def _state_redis_key(meta: LpHedgerMeta) -> str:
    return f"{meta.state_key}:state"


def _normalize_geometry_payload(payload: dict[str, Any]) -> dict[str, str]:
    values: dict[str, str] = {}
    mapping = {
        "initial_eth": "initial_eth",
        "initial_token0": "initial_eth",
        "initial_plume": "initial_plume",
        "initial_token1": "initial_plume",
        "price_lower": "price_lower",
        "price_upper": "price_upper",
    }
    for source_key, target_key in mapping.items():
        if source_key not in payload or payload[source_key] is None:
            continue
        text = str(payload[source_key]).strip()
        if text:
            values[target_key] = text
    return values


def _normalize_threshold_payload(payload: dict[str, Any]) -> dict[str, str]:
    values: dict[str, str] = {}
    mapping = {
        "eth_exposure_usd_threshold": "eth_exposure_usd_threshold",
        "token0_exposure_usd_threshold": "eth_exposure_usd_threshold",
        "plume_exposure_usd_threshold": "plume_exposure_usd_threshold",
        "token1_exposure_usd_threshold": "plume_exposure_usd_threshold",
        "price_move_pct": "price_move_pct",
    }
    for source_key, target_key in mapping.items():
        if source_key not in payload or payload[source_key] is None:
            continue
        text = str(payload[source_key]).strip()
        if text:
            values[target_key] = text
    return values


def _validate_geometry_payload(payload: dict[str, str]) -> None:
    try:
        initial_eth = Decimal(payload["initial_eth"])
        initial_plume = Decimal(payload["initial_plume"])
        price_lower = Decimal(payload["price_lower"])
        price_upper = Decimal(payload["price_upper"])
    except Exception as exc:
        raise ValueError("invalid_geometry_payload") from exc
    if initial_eth <= 0 or initial_plume <= 0:
        raise ValueError("invalid_geometry_payload")
    if price_lower <= 0 or price_upper <= 0 or price_lower >= price_upper:
        raise ValueError("invalid_geometry_payload")


def _validate_threshold_payload(payload: dict[str, str]) -> None:
    try:
        token0_threshold = Decimal(payload["eth_exposure_usd_threshold"])
        token1_threshold = Decimal(payload["plume_exposure_usd_threshold"])
        price_move_pct = Decimal(payload["price_move_pct"])
    except Exception as exc:
        raise ValueError("invalid_threshold_payload") from exc
    if token0_threshold <= 0 or token1_threshold <= 0 or price_move_pct <= 0:
        raise ValueError("invalid_threshold_payload")


def _geometry_effective_from_snapshot(snapshot: dict[str, Any] | None) -> dict[str, str] | None:
    if not snapshot:
        return None
    mapping = {
        "initial_eth": "initial_eth_effective",
        "initial_plume": "initial_plume_effective",
        "price_lower": "price_lower_effective",
        "price_upper": "price_upper_effective",
    }
    values: dict[str, str] = {}
    for field, snapshot_key in mapping.items():
        value = snapshot.get(snapshot_key)
        if value in (None, ""):
            return None
        values[field] = str(value)
    return values


def _threshold_effective_from_snapshot(snapshot: dict[str, Any] | None) -> dict[str, str] | None:
    if not snapshot:
        return None
    mapping = {
        "eth_exposure_usd_threshold": "eth_exposure_usd_threshold_effective",
        "plume_exposure_usd_threshold": "plume_exposure_usd_threshold_effective",
        "price_move_pct": "price_move_pct_effective",
    }
    values: dict[str, str] = {}
    for field, snapshot_key in mapping.items():
        value = snapshot.get(snapshot_key)
        if value in (None, ""):
            return None
        values[field] = str(value)
    return values


def _compute_geometry_effective(
    config: LpHedgerConfig | None,
    overrides: dict[str, str],
) -> dict[str, str] | None:
    if config is None:
        return None
    effective = {
        "initial_eth": str(config.initial_eth),
        "initial_plume": str(config.initial_plume),
        "price_lower": str(config.price_lower),
        "price_upper": str(config.price_upper),
    }
    effective.update(overrides)
    return effective


def _compute_threshold_effective(
    config: LpHedgerConfig | None,
    overrides: dict[str, str],
) -> dict[str, str] | None:
    if config is None:
        return None
    effective = {
        "eth_exposure_usd_threshold": str(config.eth_exposure_usd_threshold),
        "plume_exposure_usd_threshold": str(config.plume_exposure_usd_threshold),
        "price_move_pct": str(config.price_move_pct),
    }
    effective.update(overrides)
    return effective


def _validate_config_patch(payload: dict[str, Any]) -> None:  # noqa: C901
    if not isinstance(payload, dict):
        raise ValueError("invalid_payload")

    lp_payload = payload.get("lp_pool") or {}
    target_payload = payload.get("target") or {}
    hedge_payload = payload.get("hedge") or {}
    bybit_payload = payload.get("bybit") or {}

    if not isinstance(lp_payload, dict) or not isinstance(target_payload, dict):
        raise ValueError("invalid_payload")
    if not isinstance(hedge_payload, dict) or not isinstance(bybit_payload, dict):
        raise ValueError("invalid_payload")

    if "mode" in lp_payload and lp_payload["mode"] not in {"synthetic", "onchain"}:
        raise ValueError("invalid_lp_mode")

    for key in ("token0_decimals", "token1_decimals"):
        if key not in lp_payload:
            continue
        try:
            if int(lp_payload[key]) <= 0:
                raise ValueError(key)
        except Exception as exc:
            raise ValueError("invalid_decimals") from exc

    for key in ("initial_token0", "initial_token1", "price_lower", "price_upper"):
        if key not in lp_payload:
            continue
        try:
            value = Decimal(str(lp_payload[key]))
        except Exception as exc:
            raise ValueError("invalid_geometry_payload") from exc
        if value <= 0:
            raise ValueError("invalid_geometry_payload")

    if (
        "price_lower" in lp_payload
        and "price_upper" in lp_payload
        and Decimal(str(lp_payload["price_lower"])) >= Decimal(str(lp_payload["price_upper"]))
    ):
        raise ValueError("invalid_geometry_payload")

    for key in ("target_net_token0", "target_net_token1"):
        if key not in target_payload:
            continue
        try:
            Decimal(str(target_payload[key]))
        except Exception as exc:
            raise ValueError("invalid_target_payload") from exc

    for key in ("hedge_token0", "hedge_token1"):
        if key in hedge_payload and not isinstance(hedge_payload[key], bool):
            raise ValueError("invalid_hedge_flags")

    for key in ("perp_symbol_token0", "perp_symbol_token1"):
        if key in bybit_payload and bybit_payload[key] is not None and not isinstance(bybit_payload[key], str):
            raise ValueError("invalid_perp_symbol")


def _update_identity_section(identity: configparser.SectionProxy, payload: dict[str, Any]) -> None:
    label = payload.get("label")
    if label is not None:
        identity["label"] = str(label)


def _update_section_values(
    section: configparser.SectionProxy,
    payload: dict[str, Any],
    *,
    keys: tuple[str, ...],
) -> None:
    for key in keys:
        value = payload.get(key)
        if value is not None:
            section[key] = str(value)


def _update_hedge_section(hedge: configparser.SectionProxy, payload: dict[str, Any]) -> None:
    for key in ("hedge_token0", "hedge_token1"):
        if key in payload:
            hedge[key] = "1" if payload[key] else "0"


def _update_bybit_section(bybit: configparser.SectionProxy, payload: dict[str, Any]) -> None:
    for key in ("perp_symbol_token0", "perp_symbol_token1"):
        if key in payload:
            bybit[key] = str(payload[key] or "").strip()


def _hedge_flag_enabled(raw: str | None, *, default: bool = True) -> bool:
    if raw is None:
        return default
    return raw.strip() not in {"0", "false", "False"}


def _validate_required_perp_symbols(
    hedge: configparser.SectionProxy,
    bybit: configparser.SectionProxy,
) -> None:
    if _hedge_flag_enabled(hedge.get("hedge_token0", "1")) and not (bybit.get("perp_symbol_token0") or "").strip():
        raise ValueError("invalid_perp_symbol")
    if _hedge_flag_enabled(hedge.get("hedge_token1", "1")) and not (bybit.get("perp_symbol_token1") or "").strip():
        raise ValueError("invalid_perp_symbol")


def _write_config_parser(path: Path, parser: configparser.ConfigParser) -> None:
    tmp_path = path.with_suffix(f"{path.suffix}.tmp")
    with tmp_path.open("w", encoding="utf-8") as handle:
        parser.write(handle)
    os.replace(tmp_path, path)


def _patch_config_file(
    *,
    meta: LpHedgerMeta,
    payload: dict[str, Any],
) -> LpHedgerConfig:
    path = _config_path_for_meta(meta)
    parser = _read_config_parser(path)

    identity = _ensure_section(parser, "identity")
    lp_pool = _ensure_section(parser, "lp_pool")
    target = _ensure_section(parser, "target")
    hedge = _ensure_section(parser, "hedge")
    bybit = _ensure_section(parser, "bybit")

    _update_identity_section(identity, payload)
    _update_section_values(
        lp_pool,
        payload.get("lp_pool") or {},
        keys=(
            "pool_address",
            "mode",
            "token0_symbol",
            "token1_symbol",
            "token0_decimals",
            "token1_decimals",
            "initial_token0",
            "initial_token1",
            "price_lower",
            "price_upper",
        ),
    )
    _update_section_values(
        target,
        payload.get("target") or {},
        keys=("target_net_token0", "target_net_token1"),
    )
    _update_hedge_section(hedge, payload.get("hedge") or {})
    _update_bybit_section(bybit, payload.get("bybit") or {})
    _validate_required_perp_symbols(hedge, bybit)
    _write_config_parser(path, parser)
    return load_lp_hedger_config(path)


JobStatusReader = Callable[[str], Any]
JobController = Callable[[str, str], Any]


def _load_recent_events(
    client: RedisClientProtocol,
    meta: LpHedgerMeta,
    *,
    limit: int = 20,
) -> list[dict[str, Any]]:
    raw_events = client.lrange(meta.events_key, 0, max(limit - 1, 0))
    events: list[dict[str, Any]] = []
    for raw in raw_events:
        event = _decode_event(raw)
        if event is not None:
            events.append(event)
    return events


def _load_config_or_none(meta: LpHedgerMeta) -> LpHedgerConfig | None:
    try:
        return _load_config_for_meta(meta)
    except FileNotFoundError:
        return None


def _status_config_summary(meta: LpHedgerMeta, config: LpHedgerConfig | None) -> dict[str, Any]:
    if config is None:
        return _serialize_instance_meta(meta, config_summary=None)
    return _build_config_summary(config)


def _snapshot_timestamp(snapshot: dict[str, Any] | None) -> int | None:
    if not isinstance(snapshot, dict):
        return None
    timestamp = snapshot.get("timestamp")
    return timestamp if isinstance(timestamp, int) else None


def _recent_event_timestamp(events: list[dict[str, Any]]) -> int | None:
    if not events:
        return None
    timestamp = events[0].get("timestamp")
    return timestamp if isinstance(timestamp, int) else None


def _state_value(
    state: dict[str, Any] | None,
    primary_key: str,
    *,
    fallback_key: str | None = None,
) -> str | None:
    if not state:
        return None
    if primary_key in state:
        return str(state[primary_key])
    if fallback_key is not None and fallback_key in state:
        return str(state[fallback_key])
    return None


def _build_status_payload(
    meta: LpHedgerMeta,
    *,
    client: RedisClientProtocol,
    status_reader: JobStatusReader,
) -> dict[str, Any]:
    state = _decode_json(client.get(_state_redis_key(meta)))
    snapshot = _decode_json(client.get(meta.snapshot_key))
    recent_events = _load_recent_events(client, meta)
    mode = _decode_json(client.get(meta.mode_key)) or {}
    geometry_overrides = _normalize_geometry_payload(
        _decode_json(client.get(meta.geometry_overrides_key)) or {},
    )
    threshold_overrides = _normalize_threshold_payload(
        _decode_json(client.get(meta.threshold_overrides_key)) or {},
    )
    config = _load_config_or_none(meta)

    return {
        "id": meta.id,
        "job_id": meta.job_id,
        "job_status": str(status_reader(meta.job_id)),
        "last_tick_ts": _snapshot_timestamp(snapshot),
        "last_hedge_ts": _recent_event_timestamp(recent_events),
        "last_hedge_price": _state_value(state, "last_hedge_price"),
        "last_net_eth": _state_value(state, "last_net_eth", fallback_key="last_net_token0"),
        "last_net_plume": _state_value(state, "last_net_plume", fallback_key="last_net_token1"),
        "snapshot": snapshot,
        "recent_events": recent_events,
        "config_summary": _status_config_summary(meta, config),
        "geometry_overrides": geometry_overrides or None,
        "geometry_effective": _geometry_effective_from_snapshot(snapshot)
        or _compute_geometry_effective(config, geometry_overrides),
        "threshold_overrides": threshold_overrides or None,
        "threshold_effective": _threshold_effective_from_snapshot(snapshot)
        or _compute_threshold_effective(config, threshold_overrides),
        "hedger_enabled": _mode_flag(mode, "enabled", default=False),
        "dry_run": _mode_flag(mode, "dry_run", default=False),
    }


def _load_instance_payload(meta: LpHedgerMeta) -> dict[str, Any]:
    config = _load_config_or_none(meta)
    summary = _build_config_summary(config) if config is not None else None
    return _serialize_instance_meta(meta, config_summary=summary)


class _LpApiRoutes:
    def __init__(
        self,
        *,
        client: RedisClientProtocol,
        metas: tuple[LpHedgerMeta, ...],
        status_reader: JobStatusReader,
        job_controller: JobController,
    ) -> None:
        self.client = client
        self.metas = metas
        self.metas_by_id = {meta.id: meta for meta in metas}
        self.status_reader = status_reader
        self.job_controller = job_controller

    def register(self, app: Flask) -> None:
        app.get("/api/v1/hedgers/instances")(self.hedger_instances)
        app.get("/api/v1/hedgers/<hedger_id>")(self.hedger_status)
        app.post("/api/v1/hedgers/<hedger_id>/job")(self.hedger_job_action)
        app.get("/api/v1/hedgers/<hedger_id>/config")(self.hedger_config_get)
        app.patch("/api/v1/hedgers/<hedger_id>/config")(self.hedger_config_patch)
        app.post("/api/v1/hedgers/<hedger_id>/geometry-overrides")(self.hedger_geometry_overrides_set)
        app.delete("/api/v1/hedgers/<hedger_id>/geometry-overrides")(self.hedger_geometry_overrides_clear)
        app.post("/api/v1/hedgers/<hedger_id>/threshold-overrides")(self.hedger_threshold_overrides_set)
        app.delete("/api/v1/hedgers/<hedger_id>/threshold-overrides")(self.hedger_threshold_overrides_clear)
        app.post("/api/v1/hedgers/<hedger_id>/enabled")(self.hedger_enabled_set)
        app.post("/api/v1/hedgers/<hedger_id>/events/clear")(self.hedger_events_clear)

    @staticmethod
    def _error_response(error: str, status_code: int) -> tuple[Any, int]:
        return jsonify({"ok": False, "data": None, "error": error}), status_code

    @staticmethod
    def _request_body() -> dict[str, Any]:
        body = request.get_json(silent=True) or {}
        return body if isinstance(body, dict) else {}

    def _meta_or_404(self, hedger_id: str) -> LpHedgerMeta | None:
        return self.metas_by_id.get(hedger_id) or get_hedger_meta(hedger_id)

    def _status_payload(self, meta: LpHedgerMeta) -> dict[str, Any]:
        return _build_status_payload(meta, client=self.client, status_reader=self.status_reader)

    def hedger_instances(self):
        payload = [_load_instance_payload(meta) for meta in self.metas]
        return jsonify({"ok": True, "data": payload, "error": None})

    def hedger_status(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)
        return jsonify({"ok": True, "data": self._status_payload(meta), "error": None})

    def hedger_job_action(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        action = self._request_body().get("action")
        if action not in {"start", "stop", "restart"}:
            return self._error_response("invalid_action", 400)

        self.job_controller(meta.job_id, str(action))
        return jsonify({"ok": True, "data": self._status_payload(meta), "error": None})

    def hedger_config_get(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        try:
            config = _load_config_for_meta(meta)
        except FileNotFoundError:
            return self._error_response("config_not_found", 404)
        return jsonify({"ok": True, "data": _serialize_config(config), "error": None})

    def hedger_config_patch(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        body = self._request_body()
        try:
            _validate_config_patch(body)
            updated_config = _patch_config_file(meta=meta, payload=body)
        except FileNotFoundError:
            return self._error_response("config_not_found", 404)
        except ValueError as exc:
            return self._error_response(str(exc), 400)

        job_status = self.job_controller(meta.job_id, "restart")
        return jsonify(
            {
                "ok": True,
                "data": _serialize_config(updated_config),
                "error": None,
                "restart": "queued",
                "job_status": str(job_status) if job_status is not None else str(self.status_reader(meta.job_id)),
            },
        )

    def hedger_geometry_overrides_set(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        payload = _normalize_geometry_payload(self._request_body())
        if not payload:
            return self._error_response("empty_geometry_payload", 400)

        try:
            config = _load_config_for_meta(meta)
            effective = _compute_geometry_effective(config, payload)
            assert effective is not None
            _validate_geometry_payload(effective)
        except FileNotFoundError:
            return self._error_response("config_not_found", 404)
        except (AssertionError, ValueError) as exc:
            return self._error_response(str(exc), 400)

        self.client.set(meta.geometry_overrides_key, json.dumps(payload))
        return jsonify(
            {"ok": True, "data": {"geometry_overrides": payload, "geometry_effective": effective}, "error": None},
        )

    def hedger_geometry_overrides_clear(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        try:
            config = _load_config_for_meta(meta)
            effective = _compute_geometry_effective(config, {})
            assert effective is not None
        except FileNotFoundError:
            return self._error_response("config_not_found", 404)

        self.client.delete(meta.geometry_overrides_key)
        return jsonify(
            {"ok": True, "data": {"geometry_overrides": {}, "geometry_effective": effective}, "error": None},
        )

    def hedger_threshold_overrides_set(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        payload = _normalize_threshold_payload(self._request_body())
        if not payload:
            return self._error_response("empty_threshold_payload", 400)

        try:
            config = _load_config_for_meta(meta)
            effective = _compute_threshold_effective(config, payload)
            assert effective is not None
            _validate_threshold_payload(effective)
        except FileNotFoundError:
            return self._error_response("config_not_found", 404)
        except (AssertionError, ValueError) as exc:
            return self._error_response(str(exc), 400)

        self.client.set(meta.threshold_overrides_key, json.dumps(payload))
        return jsonify(
            {"ok": True, "data": {"threshold_overrides": payload, "threshold_effective": effective}, "error": None},
        )

    def hedger_threshold_overrides_clear(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        try:
            config = _load_config_for_meta(meta)
            effective = _compute_threshold_effective(config, {})
            assert effective is not None
        except FileNotFoundError:
            return self._error_response("config_not_found", 404)

        self.client.delete(meta.threshold_overrides_key)
        return jsonify(
            {"ok": True, "data": {"threshold_overrides": {}, "threshold_effective": effective}, "error": None},
        )

    def hedger_enabled_set(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        enabled = bool(self._request_body().get("enabled"))
        mode = _decode_json(self.client.get(meta.mode_key)) or {}
        mode["enabled"] = enabled
        self.client.set(meta.mode_key, json.dumps(mode))
        return jsonify({"ok": True, "data": {"hedger_enabled": enabled}, "error": None})

    def hedger_events_clear(self, hedger_id: str):
        meta = self._meta_or_404(hedger_id)
        if meta is None:
            return self._error_response("unknown_hedger", 404)

        cleared = len(self.client.lrange(meta.events_key, 0, 9999))
        self.client.delete(meta.events_key)
        return jsonify({"ok": True, "data": {"cleared": cleared}, "error": None})


def create_lp_api_app(
    *,
    redis_client: RedisClientProtocol | None = None,
    registry_metas: tuple[LpHedgerMeta, ...] | None = None,
    get_job_status: JobStatusReader | None = None,
    control_job: JobController | None = None,
) -> Flask:
    app = Flask(__name__)
    client = redis_client or _NullRedis()
    metas = tuple(list_hedgers() if registry_metas is None else registry_metas)
    status_reader = get_job_status or (lambda job_id: "unknown")
    job_controller = control_job or (lambda job_id, action: status_reader(job_id))
    _LpApiRoutes(
        client=client,
        metas=metas,
        status_reader=status_reader,
        job_controller=job_controller,
    ).register(app)
    return app


__all__ = ["create_lp_api_app"]
