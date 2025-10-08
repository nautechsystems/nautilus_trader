import math

from nautilus_pyo3.nautilus_pyo3 import MoneyFlowIndex


def test_mfi_update_returns_float_and_neutral_first():
    mfi = MoneyFlowIndex(period=10)
    # First update has no prior delta -> neutral 0.5
    val = mfi.update(close=10.0, high=10.0, low=10.0, volume=100.0)
    assert isinstance(val, float)
    assert abs(val - 0.5) < 1e-12  # should be exactly neutral


def test_mfi_directionality():
    mfi = MoneyFlowIndex(period=3)
    # Seed with a baseline typical price
    mfi.update(close=10.0, high=10.0, low=10.0, volume=100.0)
    
    # Price increase -> positive flow -> value > 0.5
    val_up = mfi.update(close=11.0, high=11.0, low=11.0, volume=100.0)
    assert val_up > 0.5
    
    # Price decrease -> negative flow
    val_down = mfi.update(close=10.5, high=10.5, low=10.5, volume=100.0)
    assert val_down < val_up  # trend reversal should reduce MFI


def test_mfi_extreme_values():
    # All positive flow should saturate to 1.0
    mfi = MoneyFlowIndex(period=3)
    mfi.update(close=100.0, high=100.0, low=100.0, volume=1000.0)
    for i in range(3):
        mfi.update(close=101.0 + i, high=101.0 + i, low=101.0 + i, volume=1000.0)
    assert abs(mfi.value - 1.0) < 1e-10
    
    # All negative flow should drop to 0.0
    mfi2 = MoneyFlowIndex(period=3)
    mfi2.update(close=100.0, high=100.0, low=100.0, volume=1000.0)
    for i in range(3):
        mfi2.update(close=99.0 - i, high=99.0 - i, low=99.0 - i, volume=1000.0)
    assert abs(mfi2.value - 0.0) < 1e-10