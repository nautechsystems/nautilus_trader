from __future__ import annotations

from collections.abc import Iterable
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True, slots=True)
class AccountScopeConfig:
    """
    Canonical shared account-provider contract for an equities profile.
    """

    scope_id: str
    provider: str
    venue: str
    ibg_host: str | None = None
    ibg_port: int | None = None
    ibg_client_id: int | None = None
    account_id: str | None = None
    dockerized_gateway: dict[str, Any] | None = None


def _required_text(row: Mapping[str, Any], field_name: str) -> str:
    raw = row.get(field_name)
    if raw is None:
        raise ValueError(f"`{field_name}` is required")
    if not isinstance(raw, str):
        raise TypeError(f"`{field_name}` must be a string")
    text = raw.strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


def _optional_text(row: Mapping[str, Any], field_name: str) -> str | None:
    raw = row.get(field_name)
    if raw is None:
        return None
    if not isinstance(raw, str):
        raise TypeError(f"`{field_name}` must be a string when provided")
    text = raw.strip()
    return text or None


def _optional_int(row: Mapping[str, Any], field_name: str) -> int | None:
    raw = row.get(field_name)
    if raw is None:
        return None
    if isinstance(raw, bool) or not isinstance(raw, int):
        raise TypeError(f"`{field_name}` must be an integer when provided")
    return raw


def _optional_mapping(row: Mapping[str, Any], field_name: str) -> dict[str, Any] | None:
    raw = row.get(field_name)
    if raw is None:
        return None
    if not isinstance(raw, Mapping):
        raise TypeError(f"`{field_name}` must be a mapping when provided")
    return dict(raw)


def decode_account_scopes(rows: Iterable[Mapping[str, Any]]) -> tuple[AccountScopeConfig, ...]:
    decoded: list[AccountScopeConfig] = []
    for index, row in enumerate(rows):
        if not isinstance(row, Mapping):
            raise TypeError(f"account scope row {index} must be a mapping")
        decoded.append(
            AccountScopeConfig(
                scope_id=_required_text(row, "scope_id"),
                provider=_required_text(row, "provider"),
                venue=_required_text(row, "venue"),
                ibg_host=_optional_text(row, "ibg_host"),
                ibg_port=_optional_int(row, "ibg_port"),
                ibg_client_id=_optional_int(row, "ibg_client_id"),
                account_id=_optional_text(row, "account_id"),
                dockerized_gateway=_optional_mapping(row, "dockerized_gateway"),
            ),
        )
    return tuple(decoded)


__all__ = ("AccountScopeConfig", "decode_account_scopes")
