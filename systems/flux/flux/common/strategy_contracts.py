from __future__ import annotations

from collections.abc import Iterable
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True, slots=True)
class StrategyContractEntry:
    """
    Canonical equities strategy identity and shared-account scope contract.
    """

    strategy_id: str
    portfolio_asset_id: str
    maker_instrument_id: str
    reference_instrument_id: str
    execution_account_scope_id: str
    reference_account_scope_id: str
    hedge_account_scope_id: str | None = None


def _required_text(row: Mapping[str, Any], field_name: str) -> str:
    raw = row.get(field_name)
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


def decode_strategy_contracts(rows: Iterable[Mapping[str, Any]]) -> tuple[StrategyContractEntry, ...]:
    """
    Normalize TOML/JSON manifest rows into immutable contract entries.
    """
    decoded: list[StrategyContractEntry] = []
    for index, row in enumerate(rows):
        if not isinstance(row, Mapping):
            raise TypeError(f"strategy contract manifest row {index} must be a mapping")
        decoded.append(
            StrategyContractEntry(
                strategy_id=_required_text(row, "strategy_id"),
                portfolio_asset_id=_required_text(row, "portfolio_asset_id"),
                maker_instrument_id=_required_text(row, "maker_instrument_id"),
                reference_instrument_id=_required_text(row, "reference_instrument_id"),
                execution_account_scope_id=_required_text(row, "execution_account_scope_id"),
                reference_account_scope_id=_required_text(row, "reference_account_scope_id"),
                hedge_account_scope_id=_optional_text(row, "hedge_account_scope_id"),
            ),
        )
    return tuple(decoded)


__all__ = ("StrategyContractEntry", "decode_strategy_contracts")
