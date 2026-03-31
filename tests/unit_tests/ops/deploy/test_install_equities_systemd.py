from __future__ import annotations

import os
import stat
import subprocess
from pathlib import Path
import textwrap


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content, encoding="utf-8")


def _write_strategy_config(root: Path, strategy_id: str) -> None:
    _write(
        root / f"deploy/equities/strategies/{strategy_id}.toml",
        textwrap.dedent(
            f"""
            [identity]
            strategy_id = "{strategy_id}"
            strategy_instance_id = "{strategy_id}"
            external_strategy_id = "{strategy_id}"
            """,
        ).strip()
        + "\n",
    )


def _strategy_contract(strategy_id: str) -> str:
    symbol = strategy_id.split("_", 1)[0].upper()
    if "_binance_perp_" in strategy_id:
        maker_venue = "BINANCE_PERP"
        maker_symbol = f"{symbol}USDT"
        maker_instrument_id = f"{symbol}USDT-PERP.BINANCE_PERP"
    else:
        maker_venue = "HYPERLIQUID"
        maker_symbol = symbol
        maker_instrument_id = f"xyz:{symbol}-USD-PERP.HYPERLIQUID"

    return textwrap.dedent(
        f"""
        [[strategy_contracts]]
        strategy_id = "{strategy_id}"
        portfolio_asset_id = "{symbol}"
        maker_venue = "{maker_venue}"
        maker_symbol = "{maker_symbol}"
        market_type = "perp"
        maker_instrument_id = "{maker_instrument_id}"
        reference_instrument_id = "{symbol}.NASDAQ"
        execution_account_scope_id = "execution.{symbol.lower()}"
        reference_account_scope_id = "reference.{symbol.lower()}"
        hedge_account_scope_id = "hedge.{symbol.lower()}"
        """,
    ).strip()


def _write_live_config(root: Path, strategy_ids: tuple[str, ...]) -> None:
    strategy_ids_toml = ", ".join(f'"{strategy_id}"' for strategy_id in strategy_ids)
    contracts = "\n\n".join(_strategy_contract(strategy_id) for strategy_id in strategy_ids)
    _write(
        root / "deploy/equities/equities.live.toml",
        textwrap.dedent(
            f"""
            [api]
            strategy_class = "equities_maker"
            equities_strategy_ids = [{strategy_ids_toml}]

            {contracts}
            """,
        ).strip()
        + "\n",
    )


def _make_release_root(
    root: Path,
    *,
    strategy_id: str | None = None,
    strategy_ids: tuple[str, ...] = ("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
) -> None:
    if strategy_id is not None:
        strategy_ids = (strategy_id,)
    _write_live_config(root, strategy_ids)
    for strategy_id in strategy_ids:
        _write_strategy_config(root, strategy_id)
    _write(root / ".flux-release/release.env", "DEPLOY_LANE=prod\nSTACK_NAME=equities\nRELEASE_ID=test\n")
    python_bin = root / ".venv/bin/python"
    _write(python_bin, "#!/usr/bin/env bash\nexec python3 \"$@\"\n")
    python_bin.chmod(python_bin.stat().st_mode | stat.S_IXUSR)
    helper = root / "ops/scripts/deploy/list_equities_node_groups.py"
    _write(
        helper,
        textwrap.dedent(
            """\
            #!/usr/bin/env python3
            from __future__ import annotations

            import argparse
            from pathlib import Path


            def derive_group_id(strategy_id: str) -> str:
                for suffix in ("_maker", "_taker"):
                    if strategy_id.endswith(suffix):
                        return strategy_id[: -len(suffix)]
                return strategy_id


            def main() -> None:
                parser = argparse.ArgumentParser()
                parser.add_argument("--shared-config")
                parser.add_argument("--strategies-dir", required=True)
                args = parser.parse_args()

                groups: dict[str, list[Path]] = {}
                for path in sorted(Path(args.strategies_dir).glob("*.toml")):
                    if path.name == "equities.strategy.template.toml":
                        continue
                    groups.setdefault(derive_group_id(path.stem), []).append(path)
                for node_group_id, paths in groups.items():
                    print("\\t".join([node_group_id, *[str(path) for path in paths]]))


            if __name__ == "__main__":
                main()
            """,
        ),
    )
    helper.chmod(helper.stat().st_mode | stat.S_IXUSR)


def _run_installer_snippet(snippet: str, *, env: dict[str, str]) -> subprocess.CompletedProcess[str]:
    repo_root = _repo_root()
    script_path = repo_root / "ops/scripts/deploy/install_equities_systemd.sh"
    return subprocess.run(  # noqa: S603 - controlled test invocation of repo shell helper
        [
            "/usr/bin/bash",
            "-lc",
            f'source "{script_path}"\n{snippet}\n',
        ],
        check=False,
        capture_output=True,
        text=True,
        cwd=repo_root,
        env=env,
    )


def test_resolve_deploy_root_honors_explicit_release_root(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/prod/equities/current"
    _make_release_root(deploy_root)
    _write(common_env_path, "WORKDIR=/tmp/old-common-root\n")

    result = _run_installer_snippet(
        "resolve_deploy_root\n",
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
        },
    )

    assert result.returncode == 0
    assert result.stdout.strip() == str(deploy_root)


def test_resolve_deploy_root_preserves_existing_service_root_on_rerun(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    stable_root = tmp_path / "releases/prod/equities/current"
    _make_release_root(stable_root)
    _write(common_env_path, "WORKDIR=/tmp/dev-checkout\n")
    _write(env_dir / "equities-api.env", f"WORKDIR={stable_root}\n")

    result = _run_installer_snippet(
        "resolve_deploy_root\n",
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
        },
    )

    assert result.returncode == 0
    assert result.stdout.strip() == str(stable_root)


def test_resolve_deploy_root_prefers_lane_root_for_first_pilot_rollout(tmp_path: Path) -> None:
    prod_root = tmp_path / "releases/prod/equities/current"
    pilot_root = tmp_path / "releases/pilot/equities/current"
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    _make_release_root(prod_root)
    _make_release_root(pilot_root)
    _write(common_env_path, f"WORKDIR={prod_root}\n")

    result = _run_installer_snippet(
        "resolve_deploy_root\n",
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_LANE": "pilot",
            "RELEASES_ROOT": str(tmp_path / "releases"),
        },
    )

    assert result.returncode == 0
    assert result.stdout.strip() == str(pilot_root)


def test_require_deploy_root_rejects_git_checkout(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    checkout_root = tmp_path / "checkout"
    checkout_root.mkdir(parents=True)
    subprocess.run(  # noqa: S603 - controlled test setup
        ["git", "init", str(checkout_root)],
        check=True,
        capture_output=True,
        text=True,
    )

    result = _run_installer_snippet(
        "initialize_stack_context\n",
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(checkout_root),
        },
    )

    assert result.returncode != 0
    assert "must not be a git checkout" in result.stderr


def test_render_pilot_envs_use_lane_aware_grouped_node_service_ids(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/pilot/equities/current"
    env_dir.mkdir(parents=True)
    _make_release_root(
        deploy_root,
        strategy_ids=("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
    )

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "discover_node_groups",
                "render_api_env",
                "render_portfolio_env",
                "render_bridge_env",
                "render_node_envs",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
            "EQUITIES_DEPLOY_LANE": "pilot",
        },
    )

    assert result.returncode == 0

    api_env = (env_dir / "equities-pilot-api.env").read_text(encoding="utf-8")
    portfolio_env = (env_dir / "equities-pilot-portfolio.env").read_text(encoding="utf-8")
    bridge_env = (env_dir / "equities-pilot-bridge.env").read_text(encoding="utf-8")
    node_env = (env_dir / "equities-pilot-node-aapl_tradexyz.env").read_text(encoding="utf-8")

    assert "PULSE_GROUP_KEY=equities-pilot" in api_env
    assert "PULSE_GROUP_LABEL=Equities Pilot" in api_env
    assert "PULSE_SELF_SERVICE_ID=equities-pilot-api" in api_env
    assert "PORT=5124" in api_env
    assert f"WORKDIR={deploy_root}" in api_env
    assert f"PYTHONPATH={deploy_root}" in api_env

    assert "PULSE_GROUP_KEY=equities-pilot" in portfolio_env
    assert "PULSE_GROUP_KEY=equities-pilot" in bridge_env
    assert "PULSE_GROUP_KEY=equities-pilot" in node_env
    assert "EQUITIES_REDIS_DB=1" in api_env
    assert "EQUITIES_REDIS_DB=1" in portfolio_env
    assert "EQUITIES_REDIS_DB=1" in bridge_env
    assert "EQUITIES_REDIS_DB=1" in node_env
    assert f"{deploy_root}/.venv/bin/python" in node_env
    assert f"--config {deploy_root}/deploy/equities/strategies/aapl_tradexyz_maker.toml" in node_env
    assert f"--config {deploy_root}/deploy/equities/strategies/aapl_tradexyz_taker.toml" in node_env
    assert not (env_dir / "equities-pilot-node-aapl_tradexyz_maker.env").exists()
    assert not (env_dir / "equities-pilot-node-aapl_tradexyz_taker.env").exists()


def test_render_envs_include_flux_owned_ibkr_reference_publisher_service(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    systemd_dir = tmp_path / "etc" / "systemd" / "system"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/prod/equities/current"
    env_dir.mkdir(parents=True)
    systemd_dir.mkdir(parents=True)
    _make_release_root(deploy_root)

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "discover_node_groups",
                "render_target",
                "render_publisher_env",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "SYSTEMD_DIR": str(systemd_dir),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
        },
    )

    assert result.returncode == 0

    publisher_env = (env_dir / "equities-ibkr-reference-publisher.env").read_text(
        encoding="utf-8",
    )
    target_text = (systemd_dir / "flux-equities.target").read_text(encoding="utf-8")

    assert "PULSE_SELF_SERVICE_ID=equities-ibkr-reference-publisher" in publisher_env
    assert "run_ibkr_reference_publisher" in publisher_env
    assert f"--config {deploy_root}/deploy/equities/equities.live.toml" in publisher_env
    assert f"WORKDIR={deploy_root}" in publisher_env
    assert "Wants=flux@equities-ibkr-reference-publisher.service" in target_text


def test_render_grouped_node_envs_use_node_group_ids_and_multiple_configs(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/prod/equities/current"
    env_dir.mkdir(parents=True)
    _make_release_root(
        deploy_root,
        strategy_ids=("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
    )

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "discover_node_groups",
                "render_node_envs",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
        },
    )

    assert result.returncode == 0

    grouped_env = (env_dir / "equities-node-aapl_tradexyz.env").read_text(encoding="utf-8")
    assert not (env_dir / "equities-node-aapl_tradexyz_maker.env").exists()
    assert not (env_dir / "equities-node-aapl_tradexyz_taker.env").exists()
    assert grouped_env.count("--config") == 2
    assert f"--config {deploy_root}/deploy/equities/strategies/aapl_tradexyz_maker.toml" in grouped_env
    assert f"--config {deploy_root}/deploy/equities/strategies/aapl_tradexyz_taker.toml" in grouped_env
    assert f"--shared-config {deploy_root}/deploy/equities/equities.live.toml" in grouped_env


def test_render_grouped_target_uses_grouped_node_service_ids(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    systemd_dir = tmp_path / "etc" / "systemd" / "system"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/prod/equities/current"
    env_dir.mkdir(parents=True)
    systemd_dir.mkdir(parents=True)
    _make_release_root(
        deploy_root,
        strategy_ids=("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
    )

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "discover_node_groups",
                "render_target",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "SYSTEMD_DIR": str(systemd_dir),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
        },
    )

    assert result.returncode == 0
    target_text = (systemd_dir / "flux-equities.target").read_text(encoding="utf-8")
    assert "Wants=flux@equities-node-aapl_tradexyz.service" in target_text
    assert "Wants=flux@equities-node-aapl_tradexyz_maker.service" not in target_text
    assert "Wants=flux@equities-node-aapl_tradexyz_taker.service" not in target_text


def test_cleanup_obsolete_envs_removes_stale_per_strategy_node_envs(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/prod/equities/current"
    env_dir.mkdir(parents=True)
    _make_release_root(
        deploy_root,
        strategy_ids=("aapl_tradexyz_maker", "aapl_tradexyz_taker"),
    )
    _write(env_dir / "equities-node-aapl_tradexyz_maker.env", "stale maker\n")
    _write(env_dir / "equities-node-aapl_tradexyz_taker.env", "stale taker\n")

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "discover_node_groups",
                "cleanup_obsolete_envs",
                "render_node_envs",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
        },
    )

    assert result.returncode == 0
    assert (env_dir / "equities-node-aapl_tradexyz.env").exists()
    assert not (env_dir / "equities-node-aapl_tradexyz_maker.env").exists()
    assert not (env_dir / "equities-node-aapl_tradexyz_taker.env").exists()


def test_render_pilot_target_uses_lane_aware_grouped_node_service_ids(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    systemd_dir = tmp_path / "etc" / "systemd" / "system"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/pilot/equities/current"
    env_dir.mkdir(parents=True)
    systemd_dir.mkdir(parents=True)
    _make_release_root(
        deploy_root,
        strategy_ids=(
            "aapl_tradexyz_maker",
            "aapl_tradexyz_taker",
            "amzn_binance_perp_maker",
            "amzn_binance_perp_taker",
        ),
    )

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "discover_node_groups",
                "render_target",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "SYSTEMD_DIR": str(systemd_dir),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
            "EQUITIES_DEPLOY_LANE": "pilot",
        },
    )

    assert result.returncode == 0

    target_text = (systemd_dir / "flux-equities-pilot.target").read_text(encoding="utf-8")
    assert "Description=Flux Equities Pilot Stack" in target_text
    assert "Wants=flux@equities-pilot-api.service" in target_text
    assert "Wants=flux@equities-pilot-portfolio.service" in target_text
    assert "Wants=flux@equities-pilot-bridge.service" in target_text
    assert "Wants=flux@equities-pilot-node-aapl_tradexyz.service" in target_text
    assert "Wants=flux@equities-pilot-node-amzn_binance_perp.service" in target_text
    assert "Wants=flux@equities-pilot-node-aapl_tradexyz_maker.service" not in target_text
    assert "Wants=flux@equities-pilot-node-aapl_tradexyz_taker.service" not in target_text
    assert target_text.count("Wants=flux@equities-pilot-node-") == 2


def test_render_pilot_envs_allow_lane_redis_db_override(tmp_path: Path) -> None:
    repo_root = _repo_root()
    env_dir = tmp_path / "etc" / "flux"
    common_env_path = env_dir / "common.env"
    deploy_root = tmp_path / "releases/pilot/equities/current"
    env_dir.mkdir(parents=True)
    _make_release_root(deploy_root)

    result = _run_installer_snippet(
        "\n".join(
            [
                "initialize_stack_context",
                "discover_node_groups",
                "render_api_env",
            ],
        ),
        env={
            **os.environ,
            "ROOT_DIR": str(repo_root),
            "ENV_DIR": str(env_dir),
            "COMMON_ENV_PATH": str(common_env_path),
            "EQUITIES_DEPLOY_ROOT": str(deploy_root),
            "EQUITIES_DEPLOY_LANE": "pilot",
            "EQUITIES_LANE_REDIS_DB": "7",
        },
    )

    assert result.returncode == 0
    api_env = (env_dir / "equities-pilot-api.env").read_text(encoding="utf-8")
    assert "EQUITIES_REDIS_DB=7" in api_env
