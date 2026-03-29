#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from dataclasses import dataclass
from decimal import Decimal
from decimal import InvalidOperation
from pathlib import Path
from typing import Any
from typing import Mapping
from urllib.error import HTTPError
from urllib.error import URLError
from urllib.parse import quote
from urllib.request import urlopen

from flux.api.payloads import safe_int


PROFILE_SIGNALS_PATH = "/api/v1/signals?profile=tokenmm"
PROFILE_BALANCES_PATH = "/api/v1/balances?profile=tokenmm"
PROFILE_READINESS_PATH = "/api/v1/readiness?profile=tokenmm"
STRATEGY_BALANCES_PATH_PREFIX = "/api/v1/balances?strategy="
PULSE_JOBS_PATH = "/api/pulse/jobs"
BLOCKED_RECONCILIATION = "blocked_reconciliation"


class AuditError(RuntimeError):
    pass


@dataclass(frozen=True)
class SignalInventory:
    strategy_id: str
    base_asset: str
    local_qty_base: Decimal | None
    global_qty_base: Decimal | None
    global_qty_base_complete: bool | None
    aggregation_mode: str | None
    state_name: str


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[2]


def _default_config_path() -> Path:
    return _repo_root() / "deploy/tokenmm/tokenmm.live.toml"


def _normalize_base_url(raw: str) -> str:
    text = raw.strip()
    return text.rstrip("/") or "http://127.0.0.1:5022"


def _read_tokenmm_strategy_ids(config_path: Path) -> list[str]:
    if not config_path.is_file():
        return []
    with config_path.open("rb") as fh:
        payload = tomllib.load(fh)
    raw_ids = payload.get("api", {}).get("tokenmm_strategy_ids") or []
    return [str(item).strip() for item in raw_ids if str(item).strip()]


def _fetch_json(*, base_url: str, path: str, timeout: float) -> Any:
    url = f"{_normalize_base_url(base_url)}{path}"
    try:
        with urlopen(url, timeout=timeout) as response:
            body = response.read().decode("utf-8")
    except HTTPError as e:  # pragma: no cover - exercised by live operator use
        raise AuditError(f"{url} returned HTTP {e.code}") from e
    except URLError as e:  # pragma: no cover - exercised by live operator use
        raise AuditError(f"{url} could not be reached: {e.reason}") from e
    try:
        return json.loads(body)
    except json.JSONDecodeError as e:
        raise AuditError(f"{url} returned invalid JSON: {e}") from e


def _fetch_enveloped_data(*, base_url: str, path: str, timeout: float) -> Mapping[str, Any]:
    payload = _fetch_json(base_url=base_url, path=path, timeout=timeout)
    if not isinstance(payload, Mapping):
        raise AuditError(f"{path} did not return a JSON object")
    if payload.get("ok") is not True:
        raise AuditError(f"{path} returned ok={payload.get('ok')!r}")
    data = payload.get("data")
    if not isinstance(data, Mapping):
        raise AuditError(f"{path} envelope is missing a JSON object in data")
    return data


def _as_mapping(value: Any) -> Mapping[str, Any]:
    return value if isinstance(value, Mapping) else {}


def _decode_text(value: Any) -> str:
    if value is None:
        return ""
    return str(value).strip()


def _upper_text(value: Any) -> str:
    return _decode_text(value).upper()


def _to_decimal(value: Any) -> Decimal | None:
    if value is None or value == "":
        return None
    if isinstance(value, bool):
        return None
    try:
        return Decimal(str(value))
    except (InvalidOperation, ValueError):
        return None


def _first_decimal(*values: Any) -> Decimal | None:
    for value in values:
        parsed = _to_decimal(value)
        if parsed is not None:
            return parsed
    return None


def _to_bool(value: Any) -> bool | None:
    if isinstance(value, bool):
        return value
    if value is None:
        return None
    text = str(value).strip().lower()
    if text in {"true", "1", "yes"}:
        return True
    if text in {"false", "0", "no"}:
        return False
    return None


def _first_bool(*values: Any) -> bool | None:
    for value in values:
        parsed = _to_bool(value)
        if parsed is not None:
            return parsed
    return None


def _first_text(*values: Any) -> str | None:
    for value in values:
        text = _decode_text(value)
        if text:
            return text
    return None


def _extract_signal_inventory(strategy: Mapping[str, Any]) -> SignalInventory:
    state = _as_mapping(strategy.get("state"))
    pricing_debug = _as_mapping(state.get("pricing_debug"))
    skew = _as_mapping(pricing_debug.get("skew"))
    adjustment: Mapping[str, Any] = {}
    pricing_adjustments = strategy.get("pricing_adjustments")
    if isinstance(pricing_adjustments, list):
        for item in pricing_adjustments:
            if not isinstance(item, Mapping):
                continue
            if any(
                item.get(key) is not None
                for key in (
                    "local_qty_base",
                    "local_qty",
                    "global_qty_base",
                    "global_qty",
                    "global_qty_base_complete",
                    "global_qty_complete",
                    "aggregation_mode",
                )
            ):
                adjustment = item
                break

    meta = _as_mapping(strategy.get("meta"))
    return SignalInventory(
        strategy_id=_decode_text(strategy.get("id") or meta.get("strategy_id")),
        base_asset=_upper_text(meta.get("base_asset")),
        local_qty_base=_first_decimal(
            adjustment.get("local_qty_base"),
            adjustment.get("local_qty"),
            state.get("local_qty_base"),
            state.get("local_qty"),
        ),
        global_qty_base=_first_decimal(
            adjustment.get("global_qty_base"),
            adjustment.get("global_qty"),
            state.get("global_qty_base"),
            state.get("global_qty"),
        ),
        global_qty_base_complete=_first_bool(
            adjustment.get("global_qty_base_complete"),
            adjustment.get("global_qty_complete"),
            state.get("global_qty_base_complete"),
            state.get("global_qty_complete"),
        ),
        aggregation_mode=_first_text(
            adjustment.get("aggregation_mode"),
            state.get("aggregation_mode"),
            strategy.get("aggregation_mode"),
            skew.get("global_inventory_aggregation_mode"),
            skew.get("aggregation_mode"),
        ),
        state_name=_decode_text(state.get("state")).lower(),
    )


def _row_ts_ms(row: Mapping[str, Any]) -> int:
    raw = row.get("ts_ms")
    try:
        return int(raw) if raw is not None else 0
    except (TypeError, ValueError):
        return 0


def _row_id_order_key(row_id: Any) -> tuple[tuple[int, int | str], ...]:
    text = _decode_text(row_id)
    if not text:
        return tuple()
    parts = re.split(r"(\d+)", text)
    return tuple(
        (0, int(part)) if part.isdigit() else (1, part)
        for part in parts
        if part
    )


def _base_asset_row_account_identity(row: Mapping[str, Any]) -> str:
    explicit_account = _first_text(
        row.get("account_scope_id"),
        row.get("account"),
        row.get("account_id"),
        row.get("wallet"),
        row.get("subaccount"),
    )
    if explicit_account is not None:
        return explicit_account
    row_id = _decode_text(row.get("row_id"))
    if ":evt:" in row_id:
        return row_id.split(":evt:", maxsplit=1)[0]
    return row_id or "<unknown>"


def _position_qty_from_rows(rows: list[Mapping[str, Any]]) -> tuple[Decimal | None, str]:
    position_rows = [
        row for row in rows if _decode_text(row.get("kind")).lower() == "position"
    ]
    if not position_rows:
        return None, "no_position_rows"

    total = Decimal("0")
    for row in position_rows:
        qty = _first_decimal(
            row.get("signed_qty_base"),
            row.get("signed_qty"),
            row.get("quantity_base"),
            row.get("quantity"),
        )
        if qty is None:
            return None, "position_row_missing_qty"
        total += qty
    return total, "position_rows"


def _latest_base_asset_qty_from_rows(
    *,
    rows: list[Mapping[str, Any]],
    base_asset: str,
) -> tuple[Decimal | None, str]:
    base_rows = [
        row
        for row in rows
        if _decode_text(row.get("kind")).lower() != "position"
        and _upper_text(row.get("asset")) == base_asset
    ]
    if not base_rows:
        return None, "no_base_asset_rows"

    latest_rows_by_account: dict[str, Mapping[str, Any]] = {}
    for row in base_rows:
        account_id = _base_asset_row_account_identity(row)
        previous = latest_rows_by_account.get(account_id)
        row_key = (_row_ts_ms(row), _row_id_order_key(row.get("row_id")))
        previous_key = (
            (_row_ts_ms(previous), _row_id_order_key(previous.get("row_id")))
            if previous is not None
            else None
        )
        if previous_key is None or row_key > previous_key:
            latest_rows_by_account[account_id] = row

    total = Decimal("0")
    for row in latest_rows_by_account.values():
        qty = _first_decimal(
            row.get("total"),
            row.get("free"),
            row.get("quantity"),
            row.get("qty"),
        )
        if qty is None:
            return None, "base_asset_row_missing_qty"
        total += qty

    source = "latest_base_asset_rows_by_account"
    if len(latest_rows_by_account) == 1:
        source = "latest_base_asset_row"
    return total, source


def _strategy_local_qty_from_rows(
    *,
    rows: list[Mapping[str, Any]],
    base_asset: str,
    expected_local_qty: Decimal | None,
    component_local_position_qty: Decimal | None = None,
    component_local_spot_qty: Decimal | None = None,
) -> tuple[Decimal | None, str]:
    position_total, position_source = _position_qty_from_rows(rows)
    spot_total, spot_source = _latest_base_asset_qty_from_rows(rows=rows, base_asset=base_asset)

    if component_local_position_qty is not None or component_local_spot_qty is not None:
        total = Decimal("0")
        sources: list[str] = []
        if component_local_position_qty is not None:
            if position_total is None:
                total += component_local_position_qty
                sources.append("component_local_position_qty")
            else:
                total += position_total
                sources.append(position_source)
        if component_local_spot_qty is not None:
            if spot_total is None:
                total += component_local_spot_qty
                sources.append("component_local_spot_qty")
            else:
                total += spot_total
                sources.append(spot_source)
        return total, "+".join(sources) or "component_rows"

    if position_total is not None:
        return position_total, position_source

    if spot_total is not None:
        return spot_total, spot_source

    if expected_local_qty == Decimal("0"):
        return Decimal("0"), "absent_true_zero"

    return None, "no_position_or_base_asset_rows"


def _job_status_by_id(pulse_payload: Mapping[str, Any]) -> dict[str, Mapping[str, Any]]:
    jobs = pulse_payload.get("jobs")
    if not isinstance(jobs, list):
        raise AuditError(f"{PULSE_JOBS_PATH} payload is missing a jobs list")
    result: dict[str, Mapping[str, Any]] = {}
    for job in jobs:
        if not isinstance(job, Mapping):
            continue
        job_id = _decode_text(job.get("id") or job.get("name"))
        if job_id:
            result[job_id] = job
    return result


def _profile_components_by_strategy(
    balances_payload: Mapping[str, Any],
) -> dict[str, Mapping[str, Any]]:
    components = balances_payload.get("components")
    if not isinstance(components, list):
        return {}
    result: dict[str, Mapping[str, Any]] = {}
    for component in components:
        if not isinstance(component, Mapping):
            continue
        strategy_id = _decode_text(component.get("strategy_id"))
        if strategy_id:
            result[strategy_id] = component
    return result


def _strategy_balances_path(strategy_id: str) -> str:
    return f"{STRATEGY_BALANCES_PATH_PREFIX}{quote(strategy_id, safe='')}&limit=5000"


def _component_is_missing_optional(component: Mapping[str, Any] | None) -> bool:
    if not isinstance(component, Mapping):
        return False
    return bool(component.get("missing")) and not bool(component.get("required"))


def _append_error(errors: list[str], message: str) -> None:
    errors.append(message)


def _readiness_failure_message(readiness_payload: Mapping[str, Any]) -> str:
    summary = _as_mapping(readiness_payload.get("summary"))
    failed_checks = list(summary.get("failed_checks") or [])
    stale_state_streams = list(summary.get("stale_state_stream_strategy_ids") or [])
    missing_state_streams = list(summary.get("missing_state_stream_strategy_ids") or [])
    stale_signals = list(summary.get("stale_signal_strategy_ids") or [])
    missing_signals = list(summary.get("missing_signal_strategy_ids") or [])

    details: list[str] = []
    if failed_checks:
        details.append(f"failed_checks={','.join(str(item) for item in failed_checks)}")
    if stale_state_streams:
        details.append(
            "stale_state_stream_strategy_ids="
            + ",".join(str(item) for item in stale_state_streams),
        )
    if missing_state_streams:
        details.append(
            "missing_state_stream_strategy_ids="
            + ",".join(str(item) for item in missing_state_streams),
        )
    if stale_signals:
        details.append(
            "stale_signal_strategy_ids=" + ",".join(str(item) for item in stale_signals),
        )
    if missing_signals:
        details.append(
            "missing_signal_strategy_ids=" + ",".join(str(item) for item in missing_signals),
        )
    if not details:
        return "readiness returned ok=false without diagnostic summary"
    return "; ".join(details)


def _readiness_success_fragment(readiness_payload: Mapping[str, Any]) -> str:
    summary = _as_mapping(readiness_payload.get("summary"))
    ready_strategy_count = safe_int(summary.get("ready_strategy_count"))
    required_strategy_count = safe_int(summary.get("required_strategy_count"))
    state_stream_max_age_ms = safe_int(summary.get("state_stream_max_age_ms"))

    details: list[str] = []
    if ready_strategy_count is not None and required_strategy_count is not None:
        details.append(f"readiness={ready_strategy_count}/{required_strategy_count}")
    if state_stream_max_age_ms is not None:
        details.append(f"state_stream_max_age_ms={state_stream_max_age_ms}")
    return ", ".join(details)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Audit local qty and shared global_qty_base consistency across "
            "TokenMM signals, balances, and Pulse surfaces."
        ),
    )
    parser.add_argument(
        "--base-url",
        default="http://127.0.0.1:5022",
        help="Flux API base URL (default: %(default)s)",
    )
    parser.add_argument(
        "--config",
        default=str(_default_config_path()),
        help="TokenMM shared config used to discover expected strategy ids.",
    )
    parser.add_argument(
        "--strategy-id",
        action="append",
        dest="strategy_ids",
        default=[],
        help="Audit only the provided strategy id. May be passed more than once.",
    )
    parser.add_argument(
        "--timeout",
        type=float,
        default=5.0,
        help="HTTP timeout in seconds (default: %(default)s)",
    )
    args = parser.parse_args(argv)

    base_url = _normalize_base_url(args.base_url)
    config_path = Path(args.config)
    expected_strategy_ids = list(args.strategy_ids) or _read_tokenmm_strategy_ids(config_path)
    errors: list[str] = []

    readiness_payload = _fetch_enveloped_data(
        base_url=base_url,
        path=PROFILE_READINESS_PATH,
        timeout=args.timeout,
    )
    if readiness_payload.get("ok") is not True:
        _append_error(
            errors,
            "TokenMM readiness is unhealthy: "
            + _readiness_failure_message(readiness_payload),
        )

    signals_data = _fetch_enveloped_data(
        base_url=base_url,
        path=PROFILE_SIGNALS_PATH,
        timeout=args.timeout,
    )
    balances_profile = _fetch_enveloped_data(
        base_url=base_url,
        path=PROFILE_BALANCES_PATH,
        timeout=args.timeout,
    )
    pulse_payload = _fetch_json(base_url=base_url, path=PULSE_JOBS_PATH, timeout=args.timeout)
    if not isinstance(pulse_payload, Mapping):
        raise AuditError(f"{PULSE_JOBS_PATH} did not return a JSON object")

    strategies_payload = signals_data.get("strategies")
    if not isinstance(strategies_payload, list):
        raise AuditError(f"{PROFILE_SIGNALS_PATH} data is missing a strategies list")

    signal_rows = [
        _extract_signal_inventory(strategy)
        for strategy in strategies_payload
        if isinstance(strategy, Mapping)
    ]
    signals_by_id = {row.strategy_id: row for row in signal_rows if row.strategy_id}
    if not expected_strategy_ids:
        expected_strategy_ids = list(signals_by_id)

    missing_signal_rows = [sid for sid in expected_strategy_ids if sid not in signals_by_id]
    if missing_signal_rows:
        _append_error(
            errors,
            f"signals profile is missing expected strategies: {', '.join(missing_signal_rows)}",
        )

    profile_source = _decode_text(balances_profile.get("source"))
    if profile_source != "portfolio_snapshot":
        _append_error(
            errors,
            f"{PROFILE_BALANCES_PATH} returned source={profile_source or '<missing>'}, expected portfolio_snapshot",
        )

    profile_global_qty = _first_decimal(
        balances_profile.get("global_qty_base"),
        balances_profile.get("global_qty"),
    )
    profile_global_complete = _first_bool(
        balances_profile.get("global_qty_base_complete"),
        balances_profile.get("global_qty_complete"),
    )
    profile_aggregation_mode = _first_text(balances_profile.get("aggregation_mode"))
    profile_missing = list(balances_profile.get("missing_required") or [])
    profile_stale = list(balances_profile.get("stale_required") or [])
    profile_null = list(balances_profile.get("null_qty_required") or [])
    profile_degraded = bool(balances_profile.get("degraded", False))
    if (profile_degraded or profile_global_complete is False) and not any(
        (profile_missing, profile_stale, profile_null),
    ):
        _append_error(
            errors,
            "portfolio snapshot is degraded or incomplete without missing_required/stale_required/null_qty_required diagnostics",
        )

    components_by_id = _profile_components_by_strategy(balances_profile)
    pulse_jobs = _job_status_by_id(pulse_payload)

    for job_id, job in pulse_jobs.items():
        if _decode_text(job.get("group_key")) == "tokenmm" and _decode_text(
            job.get("status"),
        ) == "failed":
            _append_error(errors, f"Pulse reports failed TokenMM job: {job_id}")

    strategy_summaries: list[str] = []

    for strategy_id in expected_strategy_ids:
        signal_row = signals_by_id.get(strategy_id)
        if signal_row is None:
            continue

        component = components_by_id.get(strategy_id)
        component_local_position_qty = None
        component_local_spot_qty = None
        if component is None:
            _append_error(
                errors,
                f"{PROFILE_BALANCES_PATH} components list is missing {strategy_id}",
            )
        else:
            component_local_qty = _first_decimal(
                component.get("local_qty_base"),
                component.get("local_qty"),
            )
            component_local_position_qty = _first_decimal(component.get("local_position_qty_base"))
            component_local_spot_qty = _first_decimal(component.get("local_spot_qty"))
            if component_local_qty != signal_row.local_qty_base:
                _append_error(
                    errors,
                    f"{strategy_id}: profile component local_qty_base={component_local_qty} does not match signal local_qty_base={signal_row.local_qty_base}",
                )
            if _component_is_missing_optional(component) and signal_row.local_qty_base is None:
                strategy_summaries.append(
                    f"{strategy_id}: local_qty=<missing_optional_component> global_qty={signal_row.global_qty_base} aggregation={signal_row.aggregation_mode or '<missing>'}",
                )
                continue

        if signal_row.global_qty_base != profile_global_qty:
            _append_error(
                errors,
                f"{strategy_id}: signal global_qty_base={signal_row.global_qty_base} does not match profile global_qty_base={profile_global_qty}",
            )
        if signal_row.global_qty_base_complete != profile_global_complete:
            _append_error(
                errors,
                f"{strategy_id}: signal global_qty_base_complete={signal_row.global_qty_base_complete} does not match profile global_qty_base_complete={profile_global_complete}",
            )
        if signal_row.aggregation_mode != profile_aggregation_mode:
            _append_error(
                errors,
                f"{strategy_id}: signal aggregation_mode={signal_row.aggregation_mode!r} does not match profile aggregation_mode={profile_aggregation_mode!r}",
            )

        strategy_balances = _fetch_enveloped_data(
            base_url=base_url,
            path=_strategy_balances_path(strategy_id),
            timeout=args.timeout,
        )
        rows_payload = strategy_balances.get("rows")
        rows = [
            row for row in rows_payload if isinstance(row, Mapping)
        ] if isinstance(rows_payload, list) else []
        local_qty_from_rows, row_source = _strategy_local_qty_from_rows(
            rows=rows,
            base_asset=signal_row.base_asset,
            expected_local_qty=signal_row.local_qty_base,
            component_local_position_qty=component_local_position_qty,
            component_local_spot_qty=component_local_spot_qty,
        )
        if local_qty_from_rows is None:
            _append_error(
                errors,
                f"{strategy_id}: could not derive local qty from {STRATEGY_BALANCES_PATH_PREFIX}<id> ({row_source})",
            )
        elif local_qty_from_rows != signal_row.local_qty_base:
            _append_error(
                errors,
                f"{strategy_id}: strategy balances local qty={local_qty_from_rows} from {row_source} does not match signal local_qty_base={signal_row.local_qty_base}",
            )

        job = pulse_jobs.get(f"tokenmm-node-{strategy_id}")
        if job is not None and _decode_text(job.get("status")) == "active":
            if signal_row.state_name == BLOCKED_RECONCILIATION:
                _append_error(
                    errors,
                    f"{strategy_id}: Pulse job is active while signal state is blocked_reconciliation",
                )

        strategy_summaries.append(
            (
                f"{strategy_id}: local_qty_base={signal_row.local_qty_base} "
                f"global_qty_base={signal_row.global_qty_base} "
                f"state={signal_row.state_name or 'unknown'} "
                f"balance_source={row_source}"
            ),
        )

    if errors:
        print("TOKENMM RISK AUDIT FAILED", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    readiness_fragment = _readiness_success_fragment(readiness_payload)
    readiness_suffix = f", {readiness_fragment}" if readiness_fragment else ""
    print(
        "TOKENMM RISK AUDIT PASSED "
        f"({len(expected_strategy_ids)} strategies, "
        f"global_qty_base={profile_global_qty}, "
        f"aggregation_mode={profile_aggregation_mode}, "
        f"global_qty_base_complete={profile_global_complete}"
        f"{readiness_suffix})",
    )
    for summary in strategy_summaries:
        print(f"- {summary}")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except AuditError as exc:
        print(f"TOKENMM RISK AUDIT FAILED: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc
