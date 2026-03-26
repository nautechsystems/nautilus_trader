from __future__ import annotations

from collections.abc import Iterable
from collections.abc import Mapping
from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True, slots=True)
class StrategyContractEntry:
    """
    Canonical equities strategy-route identity and shared-account scope contract.
    """

    strategy_id: str
    portfolio_asset_id: str
    maker_instrument_id: str
    reference_instrument_id: str
    execution_account_scope_id: str
    reference_account_scope_id: str
    hedge_account_scope_id: str | None = None
    maker_venue: str | None = None
    maker_symbol: str | None = None
    market_type: str | None = None


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


def _derive_maker_venue(maker_instrument_id: str) -> str | None:
    if maker_instrument_id.endswith(".BINANCE_PERP"):
        return "BINANCE_PERP"
    if maker_instrument_id.endswith(".HYPERLIQUID"):
        return "HYPERLIQUID"
    return None


def _derive_maker_symbol(maker_instrument_id: str) -> str | None:
    symbol = maker_instrument_id.split(".", 1)[0]
    if ":" in symbol:
        symbol = symbol.split(":", 1)[1]
    if symbol.endswith("-USD-PERP"):
        return symbol.removesuffix("-USD-PERP")
    if symbol.endswith("-PERP"):
        return symbol.removesuffix("-PERP")
    return symbol or None


def decode_strategy_contracts(rows: Iterable[Mapping[str, Any]]) -> tuple[StrategyContractEntry, ...]:
    """
    Normalize TOML/JSON manifest rows into immutable contract entries.
    """
    decoded: list[StrategyContractEntry] = []
    for index, row in enumerate(rows):
        if not isinstance(row, Mapping):
            raise TypeError(f"strategy contract manifest row {index} must be a mapping")
        maker_instrument_id = _required_text(row, "maker_instrument_id")
        decoded.append(
            StrategyContractEntry(
                strategy_id=_required_text(row, "strategy_id"),
                portfolio_asset_id=_required_text(row, "portfolio_asset_id"),
                maker_instrument_id=maker_instrument_id,
                reference_instrument_id=_required_text(row, "reference_instrument_id"),
                execution_account_scope_id=_required_text(row, "execution_account_scope_id"),
                reference_account_scope_id=_required_text(row, "reference_account_scope_id"),
                hedge_account_scope_id=_optional_text(row, "hedge_account_scope_id"),
                maker_venue=_optional_text(row, "maker_venue") or _derive_maker_venue(maker_instrument_id),
                maker_symbol=_optional_text(row, "maker_symbol") or _derive_maker_symbol(maker_instrument_id),
                market_type=_optional_text(row, "market_type") or "perp",
            ),
        )
    return tuple(decoded)


def shared_observation_group_by_strategy_id(
    rows: Iterable[Mapping[str, Any]],
    *,
    allowlist: Iterable[str] | None = None,
) -> dict[str, str]:
    """
    Group same-asset strategies that observe one shared execution position.
    """
    allowlist_set = set(allowlist or ())
    use_allowlist = allowlist is not None
    grouped: dict[str, list[str]] = {}
    for contract in decode_strategy_contracts(rows):
        if use_allowlist and contract.strategy_id not in allowlist_set:
            continue
        group_key = "|".join(
            (
                contract.portfolio_asset_id.upper(),
                contract.execution_account_scope_id,
                contract.maker_instrument_id,
            ),
        )
        strategy_ids = grouped.setdefault(group_key, [])
        if contract.strategy_id not in strategy_ids:
            strategy_ids.append(contract.strategy_id)
    return {
        strategy_id: group_key
        for group_key, strategy_ids in grouped.items()
        if len(strategy_ids) > 1
        for strategy_id in strategy_ids
    }


__all__ = (
    "StrategyContractEntry",
    "decode_strategy_contracts",
    "shared_observation_group_by_strategy_id",
)
