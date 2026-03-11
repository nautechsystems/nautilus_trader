from __future__ import annotations

from decimal import Decimal
from pathlib import Path

from lp.config import load_lp_hedger_config


def write_ini(tmp_path: Path, content: str, *, name: str = "hedger.ini") -> Path:
    path = tmp_path / name
    path.write_text(content.strip(), encoding="utf-8")
    return path


def test_config_loader_accepts_chainsaw_keys_and_token_aliases(tmp_path: Path) -> None:
    cfg = write_ini(
        tmp_path,
        """
        [identity]
        id = eth_plume_lp
        label = ETH/PLUME LP Band1
        state_key = eth_plume_lp_hedger
        job_id = service-eth-plume-lp-hedger

        [lp_pool]
        token0_symbol = WETH
        token1_symbol = WPLUME
        initial_eth = 1.6085
        initial_plume = 169377
        price_lower = 85000
        price_upper = 111000

        [target]
        target_net_eth = 0.0
        target_net_plume = 0.0

        [bybit]
        eth_symbol = ETHUSDT
        plume_symbol = PLUMEUSDT
        eth_qty_step = 0.001
        plume_qty_step = 1
        max_slippage_bps = 30

        [rebalance]
        poll_interval_sec = 3
        price_move_pct = 2.0
        eth_exposure_usd_threshold = 1000
        plume_exposure_usd_threshold = 1000
        min_order_qty_eth = 0.01
        min_order_qty_plume = 10
        """,
    )

    loaded = load_lp_hedger_config(cfg)

    assert loaded.hedger_id == "eth_plume_lp"
    assert loaded.label == "ETH/PLUME LP Band1"
    assert loaded.state_key == "eth_plume_lp_hedger"
    assert loaded.job_id == "service-eth-plume-lp-hedger"
    assert loaded.token0_symbol == "WETH"
    assert loaded.token1_symbol == "WPLUME"
    assert loaded.initial_token0 == Decimal("1.6085")
    assert loaded.initial_token1 == Decimal(169377)
    assert loaded.initial_eth == Decimal("1.6085")
    assert loaded.initial_plume == Decimal(169377)
    assert loaded.target_net_token0 == Decimal("0.0")
    assert loaded.target_net_token1 == Decimal("0.0")
    assert loaded.target_net_eth == Decimal("0.0")
    assert loaded.target_net_plume == Decimal("0.0")
    assert loaded.perp_symbol_token0 == "ETHUSDT"
    assert loaded.perp_symbol_token1 == "PLUMEUSDT"
    assert loaded.eth_symbol == "ETHUSDT"
    assert loaded.plume_symbol == "PLUMEUSDT"
    assert loaded.order_qty_step_token0 == Decimal("0.001")
    assert loaded.order_qty_step_token1 == Decimal(1)
    assert loaded.eth_qty_step == Decimal("0.001")
    assert loaded.plume_qty_step == Decimal(1)


def test_config_loader_accepts_canonical_token_fields_and_masks_api_key(tmp_path: Path) -> None:
    cfg = write_ini(
        tmp_path,
        """
        [identity]
        id = hype_usdt_lp
        label = PLUME/USDT LP Hedger
        state_key = hype_usdt_lp_hedger
        job_id = service-hedger3

        [lp_pool]
        mode = synthetic
        chain = plume
        amm = rooster_v3
        pool_address = 0x0000000000000000000000000000000000000000
        token0_symbol = PLUME
        token1_symbol = USDT
        token0_decimals = 18
        token1_decimals = 6
        initial_token0 = 46159
        initial_token1 = 1000
        price_lower = 0.01854805801
        price_upper = 0.02690558829

        [target]
        target_net_token0 = 5
        target_net_token1 = 10

        [bybit]
        perp_symbol_token0 = PLUMEUSDT
        perp_symbol_token1 =
        order_qty_step_token0 = 0.001
        order_qty_step_token1 = 1
        max_slippage_bps = 30
        api_key = y0FKMMjbGioYzI29Xp
        api_secret = secret

        [rebalance]
        poll_interval_sec = 3
        price_move_pct = 2.0
        token0_exposure_usd_threshold = 1000
        token1_exposure_usd_threshold = 2000
        min_order_qty_token0 = 0.01
        min_order_qty_token1 = 10

        [hedge]
        hedge_token0 = 1
        hedge_token1 = 0
        """,
    )

    loaded = load_lp_hedger_config(cfg)

    assert loaded.lp_mode == "synthetic"
    assert loaded.initial_token0 == Decimal(46159)
    assert loaded.initial_token1 == Decimal(1000)
    assert loaded.target_net_token0 == Decimal(5)
    assert loaded.target_net_token1 == Decimal(10)
    assert loaded.hedge_token0 is True
    assert loaded.hedge_token1 is False
    assert loaded.api_key_hint == "y0FK...29Xp"
    assert loaded.summary()["api_key_hint"] == "y0FK...29Xp"


def test_config_loader_falls_back_to_registry_job_and_state_keys(tmp_path: Path) -> None:
    cfg = write_ini(
        tmp_path,
        """
        [identity]
        id = eth_plume_lp_band2

        [lp_pool]
        token0_symbol = WETH
        token1_symbol = WPLUME
        initial_token0 = 1.1
        initial_token1 = 200000
        price_lower = 80000
        price_upper = 120000

        [target]
        target_net_token0 = 5
        target_net_token1 = 10
        """,
    )

    loaded = load_lp_hedger_config(cfg)

    assert loaded.hedger_id == "eth_plume_lp_band2"
    assert loaded.job_id == "service-eth-plume-lp-hedger-band2"
    assert loaded.state_key == "eth_plume_lp_hedger_band2"
    assert loaded.perp_symbol_token0 == "ETHUSDT"
    assert loaded.order_qty_step_token0 == Decimal("0.001")
    assert loaded.order_qty_step_token1 == Decimal(1)
    assert loaded.poll_interval_sec == 3
