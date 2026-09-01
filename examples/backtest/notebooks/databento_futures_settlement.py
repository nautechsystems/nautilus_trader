"""
Example of databento futures settlement.
"""

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
# # Futures settlement at expiry
#
# Replay the bundled Databento BBO sample across the ESZ5 expiry. The strategy
# opens one ESZ5 contract before expiry while ESH6 quotes advance the clock past
# settlement.

# %%
from pathlib import Path
from typing import Self

import pandas as pd

from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


class FuturesSettlementConfig(StrategyConfig):
    """
    Collect futures settlement config tests.
    """

    _CUSTOM_FIELDS = ("future_id", "next_future_id")

    def __new__(cls, *args: object, **kwargs: object) -> Self:
        """
        Create a new instance.
        """
        for field in cls._CUSTOM_FIELDS:
            kwargs.pop(field, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        future_id: InstrumentId,
        next_future_id: InstrumentId,
        **_kwargs: object,
    ) -> None:
        """
        Initialize the helper.
        """
        super().__init__()
        self.future_id = future_id
        self.next_future_id = next_future_id


class FuturesSettlementStrategy(Strategy):
    """
    Collect futures settlement strategy tests.
    """

    def __init__(self, config: FuturesSettlementConfig) -> None:
        """
        Initialize the helper.
        """
        super().__init__(config)
        self._future_id = config.future_id
        self._next_future_id = config.next_future_id
        self.order_submitted = False

    def on_start(self) -> None:
        """
        On start.
        """
        self.subscribe_quotes(self._future_id)
        self.subscribe_quotes(self._next_future_id)

    def on_quote(self, quote: QuoteTick) -> None:
        """
        On quote.
        """
        if quote.instrument_id != self._future_id or self.order_submitted:
            return

        order = self.order_factory.market(
            instrument_id=self._future_id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(1),
        )
        self.submit_order(order)
        self.order_submitted = True


# %%
if __name__ == "__main__":
    repo_root = Path(__file__).resolve().parents[3]
    data_dir = repo_root / "test_data" / "databento" / "futures_settlement" / "databento"
    loader = DatabentoDataLoader(
        repo_root / "crates" / "adapters" / "databento" / "publishers.json",
    )

    instruments = loader.load_instruments(
        data_dir / "futures_settlement_definition.dbn.zst",
        use_exchange_as_venue=True,
    )
    quotes = loader.load_bbo_quotes(
        data_dir / "futures_settlement_bbo-1m_2025-12-19T14-25-00_2025-12-19T14-35-00.dbn.zst",
    )

    future_id = InstrumentId.from_str("ESZ5.XCME")
    next_future_id = InstrumentId.from_str("ESH6.XCME")
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

    for instrument in instruments:
        engine.add_instrument(instrument)
    engine.add_data(quotes)
    engine.add_strategy(
        FuturesSettlementStrategy(
            FuturesSettlementConfig(
                future_id=future_id,
                next_future_id=next_future_id,
            ),
        ),
    )
    engine.run()

    with pd.option_context("display.max_columns", None, "display.width", 300):
        print(engine.generate_account_report(XCME))
        print(engine.generate_order_fills_report())
        print(engine.generate_positions_report())

    engine.reset()
    engine.dispose()
