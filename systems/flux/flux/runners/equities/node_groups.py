from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Any
import tomllib


_SPLIT_SUFFIXES = ("_maker", "_taker")


@dataclass(frozen=True)
class EquitiesNodeGroup:
    node_group_id: str
    strategy_ids: tuple[str, ...]
    config_paths: tuple[Path, ...]
    portfolio_asset_id: str
    maker_venue: str
    maker_symbol: str
    market_type: str
    maker_instrument_id: str
    reference_instrument_id: str
    execution_account_scope_id: str
    reference_account_scope_id: str
    hedge_account_scope_id: str | None


def derive_equities_node_group_id(strategy_id: str) -> str:
    for suffix in _SPLIT_SUFFIXES:
        if strategy_id.endswith(suffix):
            return strategy_id[: -len(suffix)]
    raise ValueError(f"Unsupported split equities strategy id: {strategy_id}")


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[5]


def _load_toml(path: Path) -> dict[str, Any]:
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    if not isinstance(data, dict):
        raise ValueError(f"Expected TOML table at {path}")
    return data


def load_equities_node_groups(
    *,
    repo_root: Path | None = None,
    live_config_path: Path | None = None,
    strategies_dir: Path | None = None,
) -> tuple[EquitiesNodeGroup, ...]:
    resolved_repo_root = repo_root or _repo_root()
    config_path = live_config_path or (resolved_repo_root / "deploy/equities/equities.live.toml")
    strategy_paths_root = strategies_dir or (resolved_repo_root / "deploy/equities/strategies")
    live_config = _load_toml(config_path)
    contracts = live_config.get("strategy_contracts")
    if not isinstance(contracts, list):
        raise ValueError("deploy/equities/equities.live.toml must define [[strategy_contracts]]")

    grouped_rows: dict[str, dict[str, Any]] = {}
    group_order: list[str] = []
    shared_contract_fields = (
        "portfolio_asset_id",
        "maker_venue",
        "maker_symbol",
        "market_type",
        "maker_instrument_id",
        "reference_instrument_id",
        "execution_account_scope_id",
        "reference_account_scope_id",
        "hedge_account_scope_id",
    )

    for contract in contracts:
        if not isinstance(contract, dict):
            raise ValueError("Each strategy_contract entry must be a TOML table")
        strategy_id = str(contract["strategy_id"])
        node_group_id = derive_equities_node_group_id(strategy_id)
        config_path_for_strategy = strategy_paths_root / f"{strategy_id}.toml"
        if not config_path_for_strategy.exists():
            raise FileNotFoundError(config_path_for_strategy)
        strategy_config = _load_toml(config_path_for_strategy)
        identity = strategy_config.get("identity")
        if not isinstance(identity, dict):
            raise ValueError(f"{config_path_for_strategy} is missing [identity]")
        if identity.get("strategy_id") != strategy_id:
            raise ValueError(
                f"{config_path_for_strategy} identity.strategy_id does not match {strategy_id}",
            )

        entry = grouped_rows.get(node_group_id)
        if entry is None:
            entry = {
                "strategy_ids": [],
                "config_paths": [],
                "contract": {field: contract.get(field) for field in shared_contract_fields},
            }
            grouped_rows[node_group_id] = entry
            group_order.append(node_group_id)
        else:
            for field in shared_contract_fields:
                if entry["contract"].get(field) != contract.get(field):
                    raise ValueError(
                        f"Grouped node {node_group_id} has inconsistent {field}",
                    )

        entry["strategy_ids"].append(strategy_id)
        entry["config_paths"].append(config_path_for_strategy)

    return tuple(
        EquitiesNodeGroup(
            node_group_id=node_group_id,
            strategy_ids=tuple(grouped_rows[node_group_id]["strategy_ids"]),
            config_paths=tuple(grouped_rows[node_group_id]["config_paths"]),
            portfolio_asset_id=str(grouped_rows[node_group_id]["contract"]["portfolio_asset_id"]),
            maker_venue=str(grouped_rows[node_group_id]["contract"]["maker_venue"]),
            maker_symbol=str(grouped_rows[node_group_id]["contract"]["maker_symbol"]),
            market_type=str(grouped_rows[node_group_id]["contract"]["market_type"]),
            maker_instrument_id=str(grouped_rows[node_group_id]["contract"]["maker_instrument_id"]),
            reference_instrument_id=str(
                grouped_rows[node_group_id]["contract"]["reference_instrument_id"],
            ),
            execution_account_scope_id=str(
                grouped_rows[node_group_id]["contract"]["execution_account_scope_id"],
            ),
            reference_account_scope_id=str(
                grouped_rows[node_group_id]["contract"]["reference_account_scope_id"],
            ),
            hedge_account_scope_id=(
                None
                if grouped_rows[node_group_id]["contract"]["hedge_account_scope_id"] is None
                else str(grouped_rows[node_group_id]["contract"]["hedge_account_scope_id"])
            ),
        )
        for node_group_id in group_order
    )
