from __future__ import annotations

from pathlib import Path

from flux.runners.equities.node_groups import derive_equities_node_group_id
from flux.runners.equities.node_groups import load_equities_node_groups


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


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
