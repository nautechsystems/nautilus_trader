from __future__ import annotations

import sys
from collections.abc import Iterable
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any

from .account_scopes import AccountScopeConfig
from .account_scopes import writer_domain_identity
from .strategy_contracts import StrategyContractEntry
from .strategy_contracts import strategy_writer_account_scope_id


if __name__ == "flux.common.controller_scopes":
    sys.modules.setdefault("nautilus_trader.flux.common.controller_scopes", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.common.controller_scopes":
    sys.modules.setdefault("flux.common.controller_scopes", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class ControllerScopeConfig:
    controller_scope_id: str
    profile_id: str
    writer_account_scope_id: str
    account_scope_ids: tuple[str, ...]
    canary: bool = False


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


def _required_texts(row: Mapping[str, Any], field_name: str) -> tuple[str, ...]:
    raw = row.get(field_name)
    if raw is None:
        raise ValueError(f"`{field_name}` is required")
    if isinstance(raw, str) or not isinstance(raw, Iterable):
        raise TypeError(f"`{field_name}` must be an iterable of strings")
    decoded: list[str] = []
    seen: set[str] = set()
    for index, item in enumerate(raw):
        if not isinstance(item, str):
            raise TypeError(f"`{field_name}` item {index} must be a string")
        text = item.strip()
        if not text:
            raise ValueError(f"`{field_name}` item {index} must be a non-empty string")
        if text in seen:
            raise ValueError(f"`{field_name}` must not contain duplicates")
        seen.add(text)
        decoded.append(text)
    if not decoded:
        raise ValueError(f"`{field_name}` must contain at least one scope id")
    return tuple(decoded)


def _optional_bool(row: Mapping[str, Any], field_name: str, *, default: bool = False) -> bool:
    raw = row.get(field_name)
    if raw is None:
        return default
    if not isinstance(raw, bool):
        raise TypeError(f"`{field_name}` must be a boolean when provided")
    return raw


def decode_controller_scopes(rows: Iterable[Mapping[str, Any]]) -> tuple[ControllerScopeConfig, ...]:
    decoded: list[ControllerScopeConfig] = []
    seen: set[str] = set()
    for index, row in enumerate(rows):
        if not isinstance(row, Mapping):
            raise TypeError(f"controller scope manifest row {index} must be a mapping")
        controller_scope_id = _required_text(row, "controller_scope_id")
        if controller_scope_id in seen:
            raise ValueError(f"duplicate `controller_scope_id`: {controller_scope_id!r}")
        seen.add(controller_scope_id)
        decoded.append(
            ControllerScopeConfig(
                controller_scope_id=controller_scope_id,
                profile_id=_required_text(row, "profile_id"),
                writer_account_scope_id=_required_text(row, "writer_account_scope_id"),
                account_scope_ids=_required_texts(row, "account_scope_ids"),
                canary=_optional_bool(row, "canary"),
            ),
        )
    return tuple(decoded)


def _account_scope_map(
    account_scopes: Iterable[AccountScopeConfig],
) -> dict[str, AccountScopeConfig]:
    decoded: dict[str, AccountScopeConfig] = {}
    for scope in account_scopes:
        if scope.scope_id in decoded:
            raise ValueError(f"duplicate account scope_id: {scope.scope_id!r}")
        decoded[scope.scope_id] = scope
    return decoded


def _controller_scope_map(
    controller_scopes: Iterable[ControllerScopeConfig],
) -> dict[str, ControllerScopeConfig]:
    decoded: dict[str, ControllerScopeConfig] = {}
    for scope in controller_scopes:
        if scope.controller_scope_id in decoded:
            raise ValueError(f"duplicate controller_scope_id: {scope.controller_scope_id!r}")
        decoded[scope.controller_scope_id] = scope
    return decoded


def validate_controller_scope_contracts(
    *,
    account_scopes: Iterable[AccountScopeConfig],
    strategy_contracts: Iterable[StrategyContractEntry],
    controller_scopes: Iterable[ControllerScopeConfig],
) -> None:
    account_scope_map = _account_scope_map(account_scopes)
    controller_scope_map = _controller_scope_map(controller_scopes)

    for controller_scope in controller_scope_map.values():
        writer_scope = account_scope_map.get(controller_scope.writer_account_scope_id)
        if writer_scope is None:
            raise ValueError(
                f"controller_scope_id {controller_scope.controller_scope_id!r} references unknown "
                f"writer account scope {controller_scope.writer_account_scope_id!r}",
            )
        if controller_scope.writer_account_scope_id not in controller_scope.account_scope_ids:
            raise ValueError(
                f"controller_scope_id {controller_scope.controller_scope_id!r} must include "
                f"writer account scope {controller_scope.writer_account_scope_id!r}",
            )
        expected_identity = writer_domain_identity(writer_scope)
        for account_scope_id in controller_scope.account_scope_ids:
            scope = account_scope_map.get(account_scope_id)
            if scope is None:
                raise ValueError(
                    f"controller_scope_id {controller_scope.controller_scope_id!r} references "
                    f"unknown account scope {account_scope_id!r}",
                )
            if scope.controller_scope_id != controller_scope.controller_scope_id:
                raise ValueError(
                    f"account scope {account_scope_id!r} must declare controller_scope_id "
                    f"{controller_scope.controller_scope_id!r}",
                )
            if writer_domain_identity(scope) != expected_identity:
                raise ValueError(
                    f"controller_scope_id {controller_scope.controller_scope_id!r} cannot span "
                    "multiple writer domains",
                )

    for scope in account_scope_map.values():
        if scope.controller_scope_id is None:
            continue
        controller_scope = controller_scope_map.get(scope.controller_scope_id)
        if controller_scope is None:
            raise ValueError(
                f"account scope {scope.scope_id!r} references unknown controller_scope_id "
                f"{scope.controller_scope_id!r}",
            )
        if scope.scope_id not in controller_scope.account_scope_ids:
            raise ValueError(
                f"account scope {scope.scope_id!r} is missing from controller_scope_id "
                f"{scope.controller_scope_id!r}",
            )

    for contract in strategy_contracts:
        writer_account_scope_id = strategy_writer_account_scope_id(contract)
        writer_scope = account_scope_map.get(writer_account_scope_id)
        if writer_scope is None:
            raise ValueError(
                f"strategy_id {contract.strategy_id!r} references unknown writer account scope "
                f"{writer_account_scope_id!r}",
            )

        expected_controller_scope_id = writer_scope.controller_scope_id
        if expected_controller_scope_id is None:
            if contract.controller_scope_id is not None:
                raise ValueError(
                    f"strategy_id {contract.strategy_id!r} maps controller_scope_id "
                    f"{contract.controller_scope_id!r} for unmanaged writer domain "
                    f"{writer_account_scope_id!r}",
                )
            continue

        if contract.controller_scope_id is None:
            raise ValueError(
                f"strategy_id {contract.strategy_id!r} missing controller_scope_id for managed "
                f"writer domain {writer_account_scope_id!r}",
            )
        if contract.controller_scope_id != expected_controller_scope_id:
            raise ValueError(
                f"strategy_id {contract.strategy_id!r} has conflicting writer domain mapping for "
                f"{writer_account_scope_id!r}: expected controller_scope_id "
                f"{expected_controller_scope_id!r}, got {contract.controller_scope_id!r}",
            )

        controller_scope = controller_scope_map.get(contract.controller_scope_id)
        if controller_scope is None:
            raise ValueError(
                f"strategy_id {contract.strategy_id!r} references unknown controller_scope_id "
                f"{contract.controller_scope_id!r}",
            )
        if writer_account_scope_id != controller_scope.writer_account_scope_id:
            raise ValueError(
                f"strategy_id {contract.strategy_id!r} conflicts with controller writer domain "
                f"{controller_scope.writer_account_scope_id!r}",
            )


__all__ = (
    "ControllerScopeConfig",
    "decode_controller_scopes",
    "validate_controller_scope_contracts",
)
