from __future__ import annotations

import json
from collections.abc import Mapping
from collections.abc import Sequence
from dataclasses import dataclass
from typing import Any
from typing import Protocol


class AccountProjectionProvider(Protocol):
    def snapshot(self) -> dict[str, Any] | None: ...


@dataclass(frozen=True, slots=True)
class ProfileAccountProviderBinding:
    account_scope_id: str
    source_strategy_ids: tuple[str, ...]
    provider: AccountProjectionProvider | None


def _safe_float(value: Any) -> float | None:
    try:
        out = float(value)
    except (TypeError, ValueError):
        return None
    return out if out == out and out not in (float("inf"), float("-inf")) else None


def _format_money_display(value: float) -> str:
    return f"{'-$' if value < 0 else '$'}{abs(value):.2f}"


def _safe_int(value: Any) -> int | None:
    try:
        return int(value)
    except (TypeError, ValueError):
        return None


def _normalized_text(value: Any) -> str:
    return str(value or "").strip()


def _normalized_projection_account(*, exchange: str, account: Any) -> str:
    normalized = _normalized_text(account).upper()
    if exchange != "ibkr" or not normalized:
        return normalized
    for prefix in ("IBKR-", "IBKR:", "INTERACTIVE_BROKERS-", "INTERACTIVE_BROKERS:", "IB-", "IB:"):
        if normalized.startswith(prefix):
            stripped = normalized.removeprefix(prefix).strip()
            if stripped:
                return stripped
    return normalized


def _merge_projection_totals(
    current: dict[str, Any],
    incoming: Mapping[str, Any],
) -> dict[str, Any]:
    merged = dict(current)
    for key in ("account_equity_raw", "withdrawable_raw"):
        value = _safe_float(incoming.get(key))
        if value is None:
            continue
        merged[key] = (_safe_float(merged.get(key)) or 0.0) + value
    if "account_equity_raw" in merged:
        merged["account_equity_display"] = _format_money_display(float(merged["account_equity_raw"]))
    if "withdrawable_raw" in merged:
        merged["withdrawable_display"] = _format_money_display(float(merged["withdrawable_raw"]))
    return merged


def _projection_scope_stale(projection_status: Mapping[str, Any] | None) -> bool:
    if not isinstance(projection_status, Mapping):
        return False
    last_attempt_ts_ms = _safe_int(projection_status.get("last_attempt_ts_ms"))
    last_success_ts_ms = _safe_int(projection_status.get("last_success_ts_ms"))
    stale_after_ms = _safe_int(projection_status.get("stale_after_ms")) or 0
    if last_attempt_ts_ms is None or last_success_ts_ms is None or stale_after_ms <= 0:
        return not bool(projection_status.get("healthy", False))
    return (last_attempt_ts_ms - last_success_ts_ms) > stale_after_ms


def _projection_rows_excluded_from_reconciliation(raw_rows: Any) -> bool:
    if not isinstance(raw_rows, list):
        return False
    for row in raw_rows:
        if not isinstance(row, Mapping):
            continue
        if bool(row.get("stale")) or row.get("include_in_reconciliation") is False:
            return True
    return False


def _profile_account_row_id(
    *,
    profile_id: str,
    account_scope_id: str,
    row: Mapping[str, Any],
    row_index: int,
) -> str:
    exchange = str(row.get("exchange") or row.get("venue") or "").strip().lower()
    account = str(row.get("account") or row.get("account_id") or "").strip()
    kind = str(row.get("kind") or "").strip().lower()
    if kind == "position":
        instrument = str(
            row.get("instrument_id")
            or row.get("symbol")
            or row.get("asset")
            or row.get("coin")
            or row.get("base")
            or ""
        ).strip().upper()
        if exchange and instrument:
            return (
                f"{profile_id}:shared:{account_scope_id}:pos:"
                f"{exchange}:{account}:{instrument}"
            )

    asset = str(
        row.get("asset")
        or row.get("currency")
        or row.get("coin")
        or row.get("base")
        or ""
    ).strip().upper()
    if exchange and asset:
        return (
            f"{profile_id}:shared:{account_scope_id}:cash:"
            f"{exchange}:{account}:{asset}"
        )

    raw_row_id = str(row.get("row_id") or "").strip()
    if raw_row_id:
        return f"{profile_id}:shared:{account_scope_id}:{raw_row_id}"
    return f"{profile_id}:shared:{account_scope_id}:row:{row_index}"


def projection_totals_identity(raw_rows: Any) -> tuple[str, str] | None:
    if not isinstance(raw_rows, list):
        return None
    identities: set[tuple[str, str]] = set()
    for row in raw_rows:
        if not isinstance(row, Mapping):
            continue
        exchange = _normalized_text(row.get("exchange") or row.get("venue")).lower()
        account = _normalized_projection_account(
            exchange=exchange,
            account=row.get("account") or row.get("account_id"),
        )
        if exchange and account:
            identities.add((exchange, account))
    if len(identities) != 1:
        return None
    return next(iter(identities))


def build_profile_account_snapshot(
    *,
    profile_id: str,
    bindings: Sequence[ProfileAccountProviderBinding],
    ts_ms: int,
) -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    account_scope_ids: list[str] = []
    totals: dict[str, Any] = {}
    scope_status: list[dict[str, Any]] = []

    for binding in bindings:
        provider = binding.provider
        if provider is None:
            continue
        provider_snapshot = provider.snapshot()
        if not isinstance(provider_snapshot, Mapping):
            continue
        source_scope = str(provider_snapshot.get("source_scope") or "shared_account").strip() or "shared_account"
        account_scope_id = str(binding.account_scope_id).strip()
        if account_scope_id and account_scope_id not in account_scope_ids:
            account_scope_ids.append(account_scope_id)
        projection_status = provider_snapshot.get("projection_status")
        projection_status_mapping = (
            dict(projection_status)
            if isinstance(projection_status, Mapping)
            else None
        )
        if account_scope_id and projection_status_mapping is not None:
            scope_status.append(
                {
                    "account_scope_id": account_scope_id,
                    "source_scope": source_scope,
                    "projection_status": projection_status_mapping,
                },
            )
        provider_totals = provider_snapshot.get("totals")
        if (
            isinstance(provider_totals, Mapping)
            and not _projection_scope_stale(projection_status_mapping)
            and not _projection_rows_excluded_from_reconciliation(provider_snapshot.get("rows"))
        ):
            totals = _merge_projection_totals(totals, provider_totals)

        source_strategy_ids = [
            str(strategy_id).strip()
            for strategy_id in binding.source_strategy_ids
            if str(strategy_id).strip()
        ]

        raw_rows = provider_snapshot.get("rows")
        if not isinstance(raw_rows, list):
            continue
        for row_index, row in enumerate(raw_rows):
            if not isinstance(row, Mapping):
                continue
            normalized = dict(row)
            normalized.setdefault("strategy_id", profile_id)
            normalized["source_scope"] = source_scope
            if account_scope_id:
                normalized["account_scope_id"] = account_scope_id
                normalized["row_id"] = _profile_account_row_id(
                    profile_id=profile_id,
                    account_scope_id=account_scope_id,
                    row=normalized,
                    row_index=row_index,
                )
            if source_strategy_ids:
                normalized["source_strategy_ids"] = list(source_strategy_ids)
            if projection_status_mapping is not None:
                stale = _projection_scope_stale(projection_status_mapping)
                normalized.setdefault("stale", stale)
                normalized.setdefault("include_in_reconciliation", not stale)
            rows.append(normalized)

    payload = {
        "profile_id": profile_id,
        "account_scope_ids": account_scope_ids,
        "rows": rows,
        "totals": totals,
        "server_ts_ms": int(ts_ms),
    }
    if scope_status:
        payload["scope_status"] = scope_status
    return payload


def encode_profile_account_snapshot(payload: Mapping[str, Any]) -> str:
    return json.dumps(payload, separators=(",", ":"))


def decode_profile_account_snapshot(raw: Any) -> dict[str, Any] | None:
    if raw is None:
        return None
    if isinstance(raw, bytes):
        raw = raw.decode("utf-8", errors="replace")
    if isinstance(raw, str):
        try:
            raw = json.loads(raw)
        except Exception:
            return None
    if not isinstance(raw, Mapping):
        return None
    return dict(raw)


__all__ = (
    "AccountProjectionProvider",
    "ProfileAccountProviderBinding",
    "build_profile_account_snapshot",
    "decode_profile_account_snapshot",
    "encode_profile_account_snapshot",
    "projection_totals_identity",
)
