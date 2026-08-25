# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.18.1
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# # Option exercise at expiry
#
# Replay the bundled Databento option and futures samples across expiry. The
# strategy buys one option before expiry. Futures bar closes are converted to
# trade ticks that supply the underlying price used to determine exercise.

# %%
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import BarType
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Quantity
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class OptionExerciseConfig(StrategyConfig):
    _CUSTOM_FIELDS = ("future_id", "option_id")

    def __new__(cls, *args, **kwargs):
        for field in cls._CUSTOM_FIELDS:
            kwargs.pop(field, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(self, future_id: InstrumentId, option_id: InstrumentId, **_kwargs):
        super().__init__()
        self.future_id = future_id
        self.option_id = option_id


class OptionExerciseStrategy(Strategy):
    def __init__(self, config: OptionExerciseConfig):
        super().__init__(config)
        self.order_submitted = False
        self.bar_type = BarType.from_str(f"{config.future_id}-1-MINUTE-LAST-EXTERNAL")

    def on_start(self):
        self.subscribe_quotes(self.config.option_id)
        self.subscribe_bars(self.bar_type)

    def on_quote(self, tick):
        if tick.instrument_id != self.config.option_id or self.order_submitted:
            return

        order = self.order_factory.market(
            instrument_id=self.config.option_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )
        self.submit_order(order)
        self.order_submitted = True


# %%
if __name__ == "__main__":
    repo_root = Path(__file__).resolve().parents[3]
    data_dir = repo_root / "test_data" / "databento" / "options_exercise" / "databento"
    loader = DatabentoDataLoader(
        repo_root / "crates" / "adapters" / "databento" / "publishers.json",
    )

    futures = loader.load_instruments(
        data_dir / "futures_definition.dbn.zst",
        use_exchange_as_venue=True,
    )
    options = loader.load_instruments(
        data_dir / "options_definition.dbn.zst",
        use_exchange_as_venue=True,
    )
    bars = loader.load_bars(data_dir / "futures_ohlcv-1m_2026-01-09T20-55_2026-01-09T21-05.dbn.zst")
    quotes = loader.load_bbo_quotes(
        data_dir / "options_bbo-1m_2026-01-09T20-55_2026-01-09T21-05.dbn.zst",
    )
    trades = [
        TradeTick(
            instrument_id=bar.bar_type.instrument_id,
            price=bar.close,
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId(f"BAR-{index}"),
            ts_event=bar.ts_event,
            ts_init=bar.ts_init,
        )
        for index, bar in enumerate(bars)
    ]

    future_id = InstrumentId.from_str("ESH6.XCME")
    option_id = InstrumentId.from_str("EW2F6 C7000.XCME")
    engine = BacktestEngine(
        BacktestEngineConfig(trader_id=TraderId.from_str("BACKTESTER-001")),
    )
    XCME = Venue("XCME")
    USD = Currency.from_str("USD")
    engine.add_venue(
        venue=XCME,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
    )

    for instrument in futures + options:
        engine.add_instrument(instrument)
    engine.add_data(quotes + bars + trades)
    engine.add_strategy(
        OptionExerciseStrategy(
            OptionExerciseConfig(future_id=future_id, option_id=option_id),
        ),
    )
    engine.run()

    with pd.option_context("display.max_columns", None, "display.width", 300):
        print(engine.generate_account_report(XCME))
        print(engine.generate_order_fills_report())
        print(engine.generate_positions_report())

    engine.reset()
    engine.dispose()
