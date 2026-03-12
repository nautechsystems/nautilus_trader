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


def build_profile_account_snapshot(
    *,
    profile_id: str,
    bindings: Sequence[ProfileAccountProviderBinding],
    ts_ms: int,
) -> dict[str, Any]:
    rows: list[dict[str, Any]] = []
    account_scope_ids: list[str] = []

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
            rows.append(normalized)

    return {
        "profile_id": profile_id,
        "account_scope_ids": account_scope_ids,
        "rows": rows,
        "server_ts_ms": int(ts_ms),
    }


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
)
