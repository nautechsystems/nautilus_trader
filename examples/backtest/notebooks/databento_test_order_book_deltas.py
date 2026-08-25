"""
Example of databento test order book deltas.
"""

# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.19.0
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---


# %% [markdown]
# # Databento order-book deltas
#
# Load the tracked ESM4 MBO sample and replay it through the Rust-native book
# imbalance actor with an L3 matching engine.

# %%
from pathlib import Path

from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookType
from nautilus_trader.model import Currency
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.trading import BookImbalanceActorConfig


# %%
if __name__ == "__main__":
    repo_root = Path(__file__).resolve().parents[3]
    data_dir = repo_root / "test_data" / "databento" / "order_book_deltas_catalog" / "databento"
    loader = DatabentoDataLoader(
        repo_root / "crates" / "adapters" / "databento" / "publishers.json",
    )

    instruments = loader.load_instruments(
        data_dir / "orderbooks_definition.dbn.zst",
        use_exchange_as_venue=True,
    )
    deltas = loader.load_order_book_deltas(
        data_dir / "orderbooks_mbo_2024-05-08T00-00-00_2024-05-08T00-00-02.dbn.zst",
    )

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
        book_type=BookType.L3_MBO,
    )

    for instrument in instruments:
        engine.add_instrument(instrument)
    engine.add_data(deltas)
    engine.add_builtin_actor(
        "BookImbalanceActor",
        BookImbalanceActorConfig(
            instrument_ids=[instruments[0].id],
            log_interval=1_000,
        ),
    )
    engine.run()

    print(engine.get_result().summary)
    engine.reset()
    engine.dispose()
