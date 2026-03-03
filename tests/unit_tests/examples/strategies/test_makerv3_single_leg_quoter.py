from decimal import Decimal
from importlib.util import module_from_spec
from importlib.util import spec_from_file_location
from pathlib import Path

import pytest

try:
    from nautilus_trader.examples.strategies.makerv3_single_leg_quoter import (
        build_ladder_targets,
    )
except ModuleNotFoundError:
    module_path = (
        Path(__file__).resolve().parents[4]
        / "nautilus_trader"
        / "examples"
        / "strategies"
        / "makerv3_single_leg_quoter.py"
    )
    spec = spec_from_file_location("makerv3_single_leg_quoter", module_path)
    module = module_from_spec(spec)
    assert spec is not None and spec.loader is not None
    spec.loader.exec_module(module)
    build_ladder_targets = module.build_ladder_targets


def test_build_ladder_targets_three_bands_is_deterministic():
    bid_prices, ask_prices = build_ladder_targets(
        anchor_bid=Decimal("100.0"),
        anchor_ask=Decimal("101.0"),
        bid_edges=(Decimal("0.10"), Decimal("0.30"), Decimal("0.80")),
        ask_edges=(Decimal("0.20"), Decimal("0.50"), Decimal("1.20")),
        distances=(Decimal("0.05"), Decimal("0.10"), Decimal("0.20")),
        n_orders=(2, 1, 3),
    )

    assert bid_prices == [
        Decimal("99.90"),
        Decimal("99.85"),
        Decimal("99.70"),
        Decimal("99.20"),
        Decimal("99.00"),
        Decimal("98.80"),
    ]
    assert ask_prices == [
        Decimal("101.20"),
        Decimal("101.25"),
        Decimal("101.50"),
        Decimal("102.20"),
        Decimal("102.40"),
        Decimal("102.60"),
    ]

    bid_prices2, ask_prices2 = build_ladder_targets(
        anchor_bid=Decimal("100.0"),
        anchor_ask=Decimal("101.0"),
        bid_edges=(Decimal("0.10"), Decimal("0.30"), Decimal("0.80")),
        ask_edges=(Decimal("0.20"), Decimal("0.50"), Decimal("1.20")),
        distances=(Decimal("0.05"), Decimal("0.10"), Decimal("0.20")),
        n_orders=(2, 1, 3),
    )
    assert bid_prices2 == bid_prices
    assert ask_prices2 == ask_prices


def test_build_ladder_targets_skips_empty_bands():
    bid_prices, ask_prices = build_ladder_targets(
        anchor_bid=Decimal("10"),
        anchor_ask=Decimal("10.5"),
        bid_edges=(Decimal("0.1"), Decimal("0.2"), Decimal("0.3")),
        ask_edges=(Decimal("0.1"), Decimal("0.2"), Decimal("0.3")),
        distances=(Decimal("0.01"), Decimal("0.02"), Decimal("0.03")),
        n_orders=(0, 2, 0),
    )

    assert bid_prices == [Decimal("9.8"), Decimal("9.78")]
    assert ask_prices == [Decimal("10.7"), Decimal("10.72")]


def test_build_ladder_targets_requires_three_band_params():
    with pytest.raises(ValueError, match="expected three bands"):
        build_ladder_targets(
            anchor_bid=Decimal("1"),
            anchor_ask=Decimal("2"),
            bid_edges=(Decimal("0.1"), Decimal("0.2")),
            ask_edges=(Decimal("0.1"), Decimal("0.2"), Decimal("0.3")),
            distances=(Decimal("0.01"), Decimal("0.02"), Decimal("0.03")),
            n_orders=(1, 1, 1),
        )
