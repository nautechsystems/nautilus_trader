from __future__ import annotations

from collections.abc import Mapping
from typing import Any

from flux.common.account_projection import decode_profile_account_snapshot
from flux.common.keys import FluxRedisKeys


def _optional_text(value: Any) -> str | None:
    if value is None:
        return None
    text = str(value).strip()
    return text or None


def _normalized_instrument_id(value: Any) -> str | None:
    text = _optional_text(value)
    if text is None:
        return None
    return text.upper()


def _row_ts_ms(row: Mapping[str, Any]) -> int:
    try:
        return int(row.get("ts_ms") or 0)
    except Exception:
        return 0


def read_matching_shared_account_position_row(
    *,
    redis_client: Any,
    profile_id: str,
    account_scope_id: str,
    instrument_id: str,
    namespace: str,
    schema_version: str,
) -> dict[str, Any] | None:
    key = FluxRedisKeys.profile_account_projection(
        profile_id=profile_id,
        account_scope_id=account_scope_id,
        namespace=namespace,
        schema_version=schema_version,
    )
    snapshot = decode_profile_account_snapshot(redis_client.get(key))
    if not isinstance(snapshot, Mapping):
        return None

    target_instrument_id = _normalized_instrument_id(instrument_id)
    if target_instrument_id is None:
        return None

    freshest_match: dict[str, Any] | None = None
    freshest_ts_ms = -1
    for row in snapshot.get("rows") or []:
        if not isinstance(row, Mapping):
            continue
        if _optional_text(row.get("kind")) != "position":
            continue
        if _normalized_instrument_id(row.get("instrument_id")) != target_instrument_id:
            continue
        row_ts_ms = _row_ts_ms(row)
        if freshest_match is None or row_ts_ms >= freshest_ts_ms:
            freshest_match = dict(row)
            freshest_ts_ms = row_ts_ms
    return freshest_match


__all__ = ("read_matching_shared_account_position_row",)
