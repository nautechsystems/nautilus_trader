from __future__ import annotations

from pathlib import Path

import pytest

import ops.scripts.deploy.binance_equities_universe as binance_equities_universe


def test_discover_active_equity_perps_only_keeps_live_tradifi_symbols() -> None:
    exchange_info = {
        "symbols": [
            {
                "symbol": "PLTRUSDT",
                "status": "TRADING",
                "contractType": "TRADIFI_PERPETUAL",
                "underlyingType": "EQUITY",
                "baseAsset": "PLTR",
            },
            {
                "symbol": "AAPLUSDT",
                "status": "TRADING",
                "contractType": "TRADIFI_PERPETUAL",
                "underlyingType": "EQUITY",
                "baseAsset": "AAPL",
            },
            {
                "symbol": "HOODUSDT",
                "status": "PENDING_TRADING",
                "contractType": "TRADIFI_PERPETUAL",
                "underlyingType": "EQUITY",
                "baseAsset": "HOOD",
            },
            {
                "symbol": "BTCUSDT",
                "status": "TRADING",
                "contractType": "PERPETUAL",
                "underlyingType": "COIN",
                "baseAsset": "BTC",
            },
        ],
    }

    discovered = binance_equities_universe.discover_active_equity_perps(exchange_info)

    assert [(row.symbol, row.base_asset) for row in discovered] == [
        ("AAPLUSDT", "AAPL"),
        ("PLTRUSDT", "PLTR"),
    ]


def test_main_prints_discovery_diff_without_auto_enrollment(
    monkeypatch,
    tmp_path: Path,
    capsys,
) -> None:
    config_path = tmp_path / "equities.live.toml"
    config_path.write_text(
        """
[api]
equities_strategy_ids = ["pltr_binance_perp_makerv4", "pltr_tradexyz_makerv4"]

[[strategy_contracts]]
strategy_id = "pltr_binance_perp_makerv4"
portfolio_asset_id = "PLTR"
maker_venue = "BINANCE_PERP"
maker_symbol = "PLTRUSDT"
maker_instrument_id = "PLTRUSDT-PERP.BINANCE_PERP"

[[strategy_contracts]]
strategy_id = "tsla_binance_perp_makerv4"
portfolio_asset_id = "TSLA"
maker_venue = "BINANCE_PERP"
maker_symbol = "TSLAUSDT"
maker_instrument_id = "TSLAUSDT-PERP.BINANCE_PERP"

[[strategy_contracts]]
strategy_id = "pltr_tradexyz_makerv4"
portfolio_asset_id = "PLTR"
maker_venue = "HYPERLIQUID"
maker_symbol = "PLTR"
maker_instrument_id = "xyz:PLTR-USD-PERP.HYPERLIQUID"
""".strip(),
        encoding="utf-8",
    )

    monkeypatch.setattr(
        binance_equities_universe,
        "fetch_exchange_info",
        lambda *_args, **_kwargs: {
            "symbols": [
                {
                    "symbol": "PLTRUSDT",
                    "status": "TRADING",
                    "contractType": "TRADIFI_PERPETUAL",
                    "underlyingType": "EQUITY",
                    "baseAsset": "PLTR",
                },
                {
                    "symbol": "AAPLUSDT",
                    "status": "TRADING",
                    "contractType": "TRADIFI_PERPETUAL",
                    "underlyingType": "EQUITY",
                    "baseAsset": "AAPL",
                },
                {
                    "symbol": "HOODUSDT",
                    "status": "PENDING_TRADING",
                    "contractType": "TRADIFI_PERPETUAL",
                    "underlyingType": "EQUITY",
                    "baseAsset": "HOOD",
                },
            ],
        },
    )

    exit_code = binance_equities_universe.main(["--config", str(config_path)])

    assert exit_code == 0
    output = capsys.readouterr().out
    assert "Discovered active Binance equity perps (2):" in output
    assert "- AAPLUSDT -> AAPL" in output
    assert "- PLTRUSDT -> PLTR" in output
    assert "HOODUSDT" not in output
    assert "Discovered but not enrolled (1):" in output
    assert "- AAPLUSDT" in output
    assert "Enrolled but not currently live on Binance (0):" in output
    assert "- TSLAUSDT" not in output
    assert "does not modify strategy rows or allowlists" in output


@pytest.mark.parametrize(
    ("config", "expected"),
    [
        ({}, ()),
        ({"api": {}, "strategy_contracts": [{"strategy_id": "pltr_binance_perp_makerv4", "maker_venue": "BINANCE_PERP", "maker_symbol": "PLTRUSDT"}]}, ()),
        ({"api": {"equities_strategy_ids": []}, "strategy_contracts": [{"strategy_id": "pltr_binance_perp_makerv4", "maker_venue": "BINANCE_PERP", "maker_symbol": "PLTRUSDT"}]}, ()),
    ],
)
def test_enrolled_binance_equity_symbols_requires_non_empty_equities_allowlist(
    config: dict[str, object],
    expected: tuple[str, ...],
) -> None:
    assert binance_equities_universe.enrolled_binance_equity_symbols(config) == expected
