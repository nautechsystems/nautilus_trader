#!/usr/bin/env python3

from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


if __name__ == "__main__":
    SIM = Venue("SIM")
    AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD", SIM)
    GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD", SIM)
    ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()

    provider = TestDataProvider()

    # Set up wranglers
    trade_tick_wrangler = TradeTickDataWrangler(instrument=ETHUSDT_BINANCE)
    quote_tick_wrangler = QuoteTickDataWrangler(instrument=AUDUSD_SIM)
    bid_wrangler = BarDataWrangler(
        bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-BID-EXTERNAL"),
        instrument=GBPUSD_SIM,
    )
    ask_wrangler = BarDataWrangler(
        bar_type=BarType.from_str("GBP/USD.SIM-1-MINUTE-ASK-EXTERNAL"),
        instrument=GBPUSD_SIM,
    )

    count = 0
    total_runs = 128
    while count < total_runs:
        count += 1
        print(f"Run: {count}/{total_runs}")

        # Process data
        ticks = quote_tick_wrangler.process(provider.read_csv_ticks("truefx/audusd-ticks.csv"))
        ticks = trade_tick_wrangler.process(provider.read_csv_ticks("binance/ethusdt-trades.csv"))

        # Add data
        bid_bars = bid_wrangler.process(
            data=provider.read_csv_bars("fxcm/gbpusd-m1-bid-2012.csv")[:10_000],
        )
        ask_bars = ask_wrangler.process(
            data=provider.read_csv_bars("fxcm/gbpusd-m1-ask-2012.csv")[:10_000],
        )
