import math

from nautilus_trader.indicators import MoneyFlowIndex


def test_mfi_update_returns_float_and_neutral_first():
    mfi = MoneyFlowIndex(period=10)
    # First update has no prior delta -> neutral 0.5
    val = mfi.update(close=10.0, high=10.0, low=10.0, volume=100.0)
    assert isinstance(val, float)
    assert abs(val - 0.5) < 1e-12


def test_mfi_directionality_positive_then_negative():
    mfi = MoneyFlowIndex(period=3)
    # Seed
    _ = mfi.update(close=10.0, high=10.0, low=10.0, volume=10.0)
    # Up move increases MFI
    v_up = mfi.update(close=12.0, high=12.0, low=12.0, volume=10.0)
    assert v_up > 0.5
    # Down move decreases MFI
    v_down = mfi.update(close=11.0, high=11.0, low=11.0, volume=10.0)
    assert v_down < v_up


def test_mfi_extremes_and_nan_propagation():
    mfi = MoneyFlowIndex(period=2)
    _ = mfi.update(close=1e-9, high=1e-9, low=1e-9, volume=1e-9)
    v = mfi.update(close=1e9, high=1e9, low=1e9, volume=1e6)
    assert 0.0 <= v <= 1.0
    # A zero volume step shouldn't crash and should keep value finite
    v2 = mfi.update(close=2.0, high=2.0, low=2.0, volume=0.0)
    assert math.isfinite(v2)


