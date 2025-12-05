# -------------------------------------------------------------------------------------------------
#  Test script for Order Flow Indicators
# -------------------------------------------------------------------------------------------------
"""
Simple test to verify order flow indicators work correctly.
"""

from datetime import datetime, timezone
from decimal import Decimal

from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId, TradeId
from nautilus_trader.model.objects import Price, Quantity

from nautilus_trader.examples.indicators.orderflow import (
    VolumeProfile,
    VWAPBands,
    InitialBalance,
    CumulativeDelta,
    FootprintAggregator,
    StackedImbalanceDetector,
)


def create_trade_tick(
    price: float,
    size: float,
    aggressor: AggressorSide,
    ts_ns: int,
) -> TradeTick:
    """Create a test trade tick."""
    return TradeTick(
        instrument_id=InstrumentId.from_str("BTCUSDT.BINANCE"),
        price=Price.from_str(str(price)),
        size=Quantity.from_str(str(size)),
        aggressor_side=aggressor,
        trade_id=TradeId(str(ts_ns)),
        ts_event=ts_ns,
        ts_init=ts_ns,
    )


def test_cumulative_delta():
    """Test CumulativeDelta indicator."""
    print("\n=== Testing CumulativeDelta ===")
    delta = CumulativeDelta()

    # Simulate some trades
    base_ts = 1700000000_000_000_000  # Some timestamp in nanoseconds

    # Buy trades (positive delta)
    delta.handle_trade_tick(create_trade_tick(100.0, 10.0, AggressorSide.BUYER, base_ts))
    delta.handle_trade_tick(create_trade_tick(100.5, 5.0, AggressorSide.BUYER, base_ts + 1000))

    # Sell trades (negative delta)
    delta.handle_trade_tick(create_trade_tick(100.2, 3.0, AggressorSide.SELLER, base_ts + 2000))

    print(f"  Cumulative Delta: {delta.value}")
    print(f"  Buy Volume: {delta.buy_volume}")
    print(f"  Sell Volume: {delta.sell_volume}")
    print(f"  Delta Ratio: {delta.delta_ratio:.4f}")

    assert delta.value == 12.0, f"Expected 12.0, got {delta.value}"
    assert delta.buy_volume == 15.0
    assert delta.sell_volume == 3.0
    print("  ✓ CumulativeDelta test passed!")


def test_volume_profile():
    """Test VolumeProfile indicator."""
    print("\n=== Testing VolumeProfile ===")
    vp = VolumeProfile(tick_size=1.0)

    base_ts = 1700000000_000_000_000

    # Add volume at different price levels
    vp.handle_trade_tick(create_trade_tick(100.0, 50.0, AggressorSide.BUYER, base_ts))
    vp.handle_trade_tick(create_trade_tick(101.0, 100.0, AggressorSide.BUYER, base_ts + 1000))  # POC
    vp.handle_trade_tick(create_trade_tick(102.0, 30.0, AggressorSide.SELLER, base_ts + 2000))
    vp.handle_trade_tick(create_trade_tick(99.0, 20.0, AggressorSide.SELLER, base_ts + 3000))

    print(f"  POC: {vp.poc}")
    print(f"  VAH: {vp.vah}")
    print(f"  VAL: {vp.val}")
    print(f"  Total Volume: {vp.total_volume}")

    assert vp.poc == 101.0, f"Expected POC at 101.0, got {vp.poc}"
    print("  ✓ VolumeProfile test passed!")


def test_vwap_bands():
    """Test VWAPBands indicator."""
    print("\n=== Testing VWAPBands ===")
    vwap = VWAPBands(reset_hour_utc=0, num_std_bands=3)

    base_ts = 1700000000_000_000_000

    # Add some trades
    vwap.handle_trade_tick(create_trade_tick(100.0, 10.0, AggressorSide.BUYER, base_ts))
    vwap.handle_trade_tick(create_trade_tick(102.0, 20.0, AggressorSide.BUYER, base_ts + 1000))
    vwap.handle_trade_tick(create_trade_tick(101.0, 15.0, AggressorSide.SELLER, base_ts + 2000))

    print(f"  VWAP: {vwap.vwap:.4f}")
    print(f"  Std Dev: {vwap.std_dev:.4f}")
    print(f"  Upper Band 1: {vwap.upper_bands[0]:.4f}")
    print(f"  Lower Band 1: {vwap.lower_bands[0]:.4f}")

    assert vwap.initialized
    assert vwap.vwap > 0
    print("  ✓ VWAPBands test passed!")


def test_footprint():
    """Test FootprintAggregator indicator."""
    print("\n=== Testing FootprintAggregator ===")
    fp = FootprintAggregator(tick_size=1.0, imbalance_threshold=3.0)

    base_ts = 1700000000_000_000_000

    # Add trades at same price level
    fp.handle_trade_tick(create_trade_tick(100.0, 30.0, AggressorSide.BUYER, base_ts))
    fp.handle_trade_tick(create_trade_tick(100.0, 5.0, AggressorSide.SELLER, base_ts + 1000))

    level = fp.get_level(100.0)
    print(f"  Level 100.0 - Ask Vol: {level.ask_volume}, Bid Vol: {level.bid_volume}")
    print(f"  Level Delta: {level.delta}")
    print(f"  POC Price: {fp.poc_price}")
    print(f"  Total Delta: {fp.total_delta}")

    imbalances = fp.get_imbalanced_levels()
    print(f"  Imbalanced Levels: {imbalances}")

    assert level.ask_volume == 30.0
    assert level.bid_volume == 5.0
    assert 100.0 in imbalances  # Should be ASK imbalance (30/5 = 6 > 3)
    print("  ✓ FootprintAggregator test passed!")


def test_stacked_imbalance():
    """Test StackedImbalanceDetector indicator."""
    print("\n=== Testing StackedImbalanceDetector ===")
    si = StackedImbalanceDetector(tick_size=1.0, imbalance_ratio=3.0, min_stack_count=3)

    base_ts = 1700000000_000_000_000

    # Create stacked ask imbalances at consecutive levels
    for i in range(5):
        price = 100.0 + i
        si.handle_trade_tick(create_trade_tick(price, 40.0, AggressorSide.BUYER, base_ts + i * 1000))
        si.handle_trade_tick(create_trade_tick(price, 5.0, AggressorSide.SELLER, base_ts + i * 1000 + 100))

    print(f"  Bullish Signals: {si.has_bullish_signal}")
    print(f"  Bearish Signals: {si.has_bearish_signal}")
    print(f"  Stacked Ask Imbalances: {len(si.stacked_ask_imbalances)}")

    assert si.has_bullish_signal, "Expected bullish signal from stacked ask imbalances"
    print("  ✓ StackedImbalanceDetector test passed!")


if __name__ == "__main__":
    print("=" * 60)
    print("Order Flow Indicators Test Suite")
    print("=" * 60)

    test_cumulative_delta()
    test_volume_profile()
    test_vwap_bands()
    test_footprint()
    test_stacked_imbalance()

    print("\n" + "=" * 60)
    print("All tests passed! ✓")
    print("=" * 60)

