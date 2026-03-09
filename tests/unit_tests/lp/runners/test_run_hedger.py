from __future__ import annotations

from pathlib import Path

from lp.runners import run_hedger
from lp.runners.run_hedger import LpHedgerServiceRunner
from lp.runners.run_hedger import resolve_bybit_credentials
from lp.runners.run_hedger import resolve_config_path


def write_ini(tmp_path: Path, content: str, *, name: str) -> Path:
    path = tmp_path / name
    path.write_text(content.strip(), encoding="utf-8")
    return path


def write_band1_hedger_ini(tmp_path: Path) -> Path:
    return write_ini(
        tmp_path,
        """
        [identity]
        id = eth_plume_lp
        state_key = eth_plume_lp_hedger

        [lp_pool]
        mode = onchain
        pool_address = 0xpool
        token0_symbol = WETH
        token1_symbol = WPLUME
        token0_decimals = 18
        token1_decimals = 18
        initial_eth = 1.6085
        initial_plume = 169377
        price_lower = 85000
        price_upper = 111000

        [target]
        target_net_eth = 0
        target_net_plume = 0

        [bybit]
        api_key = from_hedger
        api_secret = from_hedger_secret
        eth_symbol = ETHUSDT
        plume_symbol = PLUMEUSDT
        eth_qty_step = 0.001
        plume_qty_step = 1

        [rebalance]
        poll_interval_sec = 3
        price_move_pct = 2.0
        eth_exposure_usd_threshold = 1000
        plume_exposure_usd_threshold = 1000
        min_order_qty_eth = 0.01
        min_order_qty_plume = 10
        """,
        name="eth_plume_lp_hedger.ini",
    )


def write_band2_hedger_ini(tmp_path: Path) -> Path:
    return write_ini(
        tmp_path,
        """
        [identity]
        id = eth_plume_lp_band2
        state_key = eth_plume_lp_hedger_band2

        [lp_pool]
        mode = onchain
        pool_address = 0xpool
        token0_symbol = WETH
        token1_symbol = WPLUME
        token0_decimals = 18
        token1_decimals = 18
        initial_eth = 1.6085
        initial_plume = 169377
        price_lower = 85000
        price_upper = 111000

        [target]
        target_net_eth = 0
        target_net_plume = 0

        [bybit]
        eth_symbol = ETHUSDT
        plume_symbol = PLUMEUSDT
        eth_qty_step = 0.001
        plume_qty_step = 1

        [rebalance]
        poll_interval_sec = 3
        price_move_pct = 2.0
        eth_exposure_usd_threshold = 1000
        plume_exposure_usd_threshold = 1000
        min_order_qty_eth = 0.01
        min_order_qty_plume = 10
        """,
        name="eth_plume_lp_hedger_band2.ini",
    )


def write_system_ini(tmp_path: Path) -> Path:
    return write_ini(
        tmp_path,
        """
        [redis]
        url = redis://config-example

        [bybit]
        api_key = from_default
        secret = from_default_secret

        [bybit_hedger]
        api_key = from_shared_hedger
        secret = from_shared_hedger_secret

        [bybit_hedger_band2]
        api_key = from_band2
        secret = from_band2_secret
        """,
        name="config.ini",
    )


def test_run_hedger_uses_same_credential_precedence_as_chainsaw(tmp_path: Path) -> None:
    system_config = write_system_ini(tmp_path)
    hedger_config = write_band1_hedger_ini(tmp_path)

    creds = resolve_bybit_credentials(
        system_config_path=system_config,
        hedger_config_path=hedger_config,
        hedger_id="eth_plume_lp",
    )

    assert creds == ("from_hedger", "from_hedger_secret")


def test_band2_runner_prefers_dedicated_system_credentials(tmp_path: Path) -> None:
    system_config = write_system_ini(tmp_path)
    hedger_config = write_band2_hedger_ini(tmp_path)

    creds = resolve_bybit_credentials(
        system_config_path=system_config,
        hedger_config_path=hedger_config,
        hedger_id="eth_plume_lp_band2",
    )

    assert creds == ("from_band2", "from_band2_secret")


def test_band2_runner_does_not_delete_redis_url(tmp_path: Path, monkeypatch) -> None:
    system_config = write_system_ini(tmp_path)
    hedger_config = write_band2_hedger_ini(tmp_path)
    monkeypatch.setenv("REDIS_URL", "redis://example")
    captured: dict[str, str] = {}

    def fake_get_redis_client():
        captured["redis_url"] = run_hedger.os.environ["REDIS_URL"]
        return object()

    monkeypatch.setattr(run_hedger, "get_redis_client", fake_get_redis_client)
    runner = LpHedgerServiceRunner(
        config_path=hedger_config,
        system_config_path=system_config,
        dry_run=False,
    )

    runner.build_redis_client()

    assert captured["redis_url"] == "redis://example"
    assert run_hedger.os.environ["REDIS_URL"] == "redis://example"


def test_run_hedger_resolves_config_from_chainsaw_env_var(
    tmp_path: Path,
    monkeypatch,
) -> None:
    hedger_config = write_band1_hedger_ini(tmp_path)
    args = run_hedger.parse_args([])
    monkeypatch.setenv("ETH_PLUME_LP_HEDGER_CONFIG", str(hedger_config))

    assert resolve_config_path(args) == hedger_config


def test_cli_config_path_wins_over_chainsaw_env_var(tmp_path: Path, monkeypatch) -> None:
    cli_config = write_band1_hedger_ini(tmp_path)
    env_config = write_band2_hedger_ini(tmp_path)
    args = run_hedger.parse_args(["--config", str(cli_config)])
    monkeypatch.setenv("ETH_PLUME_LP_HEDGER_BAND2_CONFIG", str(env_config))

    assert resolve_config_path(args) == cli_config
