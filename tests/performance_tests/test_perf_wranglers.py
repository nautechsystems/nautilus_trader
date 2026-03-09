from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.persistence.wranglers import TradeTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_quote_tick_data_wrangler_process_tick_data(benchmark):
    usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")

    wrangler = QuoteTickDataWrangler(instrument=usdjpy)
    provider = TestDataProvider()

    def wrangler_process():
        # 1000 ticks in data
        wrangler.process(
            data=provider.read_csv_ticks("truefx/usdjpy-ticks.csv"),
            default_volume=1_000_000,
        )

    benchmark(wrangler_process)


def test_trade_tick_data_wrangler_process(benchmark):
    ethusdt = TestInstrumentProvider.ethusdt_binance()
    wrangler = TradeTickDataWrangler(instrument=ethusdt)
    provider = TestDataProvider()

    def wrangler_process():
        # 69_806 ticks in data
        wrangler.process(data=provider.read_csv_ticks("binance/ethusdt-trades.csv"))

    benchmark(wrangler_process)
