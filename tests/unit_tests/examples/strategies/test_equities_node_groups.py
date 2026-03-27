from __future__ import annotations

from pathlib import Path
import textwrap

import pytest
from flux.runners.equities.node_groups import derive_equities_node_group_id
from flux.runners.equities.node_groups import load_equities_node_groups


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _write_strategy_config(strategies_dir: Path, strategy_id: str) -> None:
    (strategies_dir / f"{strategy_id}.toml").write_text(
        textwrap.dedent(
            f"""
            [identity]
            strategy_id = "{strategy_id}"
            strategy_instance_id = "{strategy_id}"
            external_strategy_id = "{strategy_id}"
            """
        ).strip()
        + "\n",
        encoding="utf-8",
    )


def _write_live_config(repo_root: Path, strategy_ids: list[str]) -> None:
    strategy_contract_rows = "\n\n".join(
        textwrap.dedent(
            f"""
            [[strategy_contracts]]
            strategy_id = "{strategy_id}"
            portfolio_asset_id = "AAPL"
            maker_venue = "HYPERLIQUID"
            maker_symbol = "AAPL"
            market_type = "perp"
            maker_instrument_id = "xyz:AAPL-USD-PERP.HYPERLIQUID"
            reference_instrument_id = "AAPL.NASDAQ"
            execution_account_scope_id = "hyperliquid.xyz.main"
            reference_account_scope_id = "ibkr.reference.main"
            hedge_account_scope_id = "ibkr.hedge.main"
            """
        ).strip()
        for strategy_id in strategy_ids
    )
    live_config = textwrap.dedent(
        f"""
        [api]
        equities_strategy_ids = {strategy_ids!r}

        {strategy_contract_rows}
        """
    ).strip()
    deploy_dir = repo_root / "deploy/equities"
    deploy_dir.mkdir(parents=True, exist_ok=True)
    (deploy_dir / "equities.live.toml").write_text(live_config + "\n", encoding="utf-8")


def _build_temp_repo(tmp_path: Path, strategy_ids: list[str]) -> Path:
    repo_root = tmp_path / "repo"
    strategies_dir = repo_root / "deploy/equities/strategies"
    strategies_dir.mkdir(parents=True)
    for strategy_id in set(strategy_ids):
        _write_strategy_config(strategies_dir, strategy_id)
    _write_live_config(repo_root, strategy_ids)
    return repo_root


def test_load_equities_node_groups_collapses_live_basket_into_19_groups() -> None:
    repo_root = _repo_root()

    groups = load_equities_node_groups(repo_root=repo_root)
    groups_by_id = {group.node_group_id: group for group in groups}

    assert len(groups) == 19
    assert groups_by_id["aapl_tradexyz"].strategy_ids == (
        "aapl_tradexyz_maker",
        "aapl_tradexyz_taker",
    )
    assert groups_by_id["aapl_tradexyz"].config_paths == (
        repo_root / "deploy/equities/strategies/aapl_tradexyz_maker.toml",
        repo_root / "deploy/equities/strategies/aapl_tradexyz_taker.toml",
    )
    assert groups_by_id["amzn_binance_perp"].strategy_ids == (
        "amzn_binance_perp_maker",
        "amzn_binance_perp_taker",
    )
    assert sum(len(group.strategy_ids) for group in groups) == 38


def test_derive_equities_node_group_id_resolves_every_live_strategy_once() -> None:
    repo_root = _repo_root()

    groups = load_equities_node_groups(repo_root=repo_root)
    strategy_to_group = {
        strategy_id: group.node_group_id
        for group in groups
        for strategy_id in group.strategy_ids
    }

    assert len(strategy_to_group) == 38
    assert strategy_to_group["aapl_tradexyz_maker"] == "aapl_tradexyz"
    assert strategy_to_group["aapl_tradexyz_taker"] == "aapl_tradexyz"
    assert strategy_to_group["amzn_binance_perp_maker"] == "amzn_binance_perp"
    assert strategy_to_group["amzn_binance_perp_taker"] == "amzn_binance_perp"
    assert derive_equities_node_group_id("tsla_tradexyz_maker") == "tsla_tradexyz"
    assert derive_equities_node_group_id("tsla_binance_perp_taker") == "tsla_binance_perp"


def test_load_equities_node_groups_rejects_duplicate_group_suffixes(tmp_path: Path) -> None:
    repo_root = _build_temp_repo(
        tmp_path,
        [
            "aapl_tradexyz_maker",
            "aapl_tradexyz_maker",
        ],
    )

    with pytest.raises(ValueError, match="duplicate.*maker"):
        load_equities_node_groups(repo_root=repo_root)


def test_load_equities_node_groups_rejects_more_than_two_group_members(tmp_path: Path) -> None:
    repo_root = _build_temp_repo(
        tmp_path,
        [
            "aapl_tradexyz_maker",
            "aapl_tradexyz_taker",
            "aapl_tradexyz_maker",
        ],
    )

    with pytest.raises(ValueError, match="at most one maker plus one taker"):
        load_equities_node_groups(repo_root=repo_root)
