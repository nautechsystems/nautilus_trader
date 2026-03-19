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


def _shared_asset_primary_rank(strategy_id: str) -> int:
    normalized = strategy_id.strip().lower()
    if normalized.endswith("_maker") or normalized.endswith("_makerv4") or normalized.endswith("_makerv3"):
        return 0
    if normalized.endswith("_taker"):
        return 1
    return 2


def shared_asset_primary_strategy_ids(
    contracts: Iterable[StrategyContractEntry],
    *,
    strategy_ids: Iterable[str] | None = None,
) -> dict[str, str]:
    allowlist = {str(strategy_id).strip() for strategy_id in strategy_ids or () if str(strategy_id).strip()}
    use_allowlist = strategy_ids is not None
    grouped: dict[str, list[tuple[int, StrategyContractEntry]]] = {}
    for index, contract in enumerate(contracts):
        if use_allowlist and contract.strategy_id not in allowlist:
            continue
        grouped.setdefault(contract.portfolio_asset_id.upper(), []).append((index, contract))
    return {
        asset_id: min(
            entries,
            key=lambda item: (
                _shared_asset_primary_rank(item[1].strategy_id),
                item[0],
                item[1].strategy_id,
            ),
        )[1].strategy_id
        for asset_id, entries in grouped.items()
    }


__all__ = (
    "StrategyContractEntry",
    "decode_strategy_contracts",
    "shared_asset_primary_strategy_ids",
)
