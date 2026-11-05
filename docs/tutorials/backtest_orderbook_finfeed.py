# %% [markdown]
# # Backtest with Polymarket Order Book Data (FinFeed)
#
# Retrieve historical Polymarket L2 order book data through FinFeed, cache
# it in a Parquet catalog, and run an order book imbalance backtest.
# This example uses the Yes outcome of a daily Bitcoin price threshold
# market. The replay ends before expiry and closes the simulated position
# without relying on a settlement payout.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_finfeed.py).

# %% [markdown]
# ## Prerequisites
#
# - Python 3.13, 3.14, or 3.15 and NautilusTrader 2.x with the FinFeed adapter.
#   Install a [development wheel](../getting_started/installation.md#development-wheels)
#   or build from source. Install pandas with `pip install pandas`.
# - A FinFeed key with Prediction Markets historical order book access
#   and sufficient allowance for four requests on the first run, including
#   two book pages of up to 10,000 records each.
#   See [FinFeed authentication](../integrations/finfeed.md#authentication-and-environment).
#
# Run from the repository root after building the Python extension:
#
# ```bash
# uv run --project python --no-sync --with pandas \
#     python docs/tutorials/backtest_orderbook_finfeed.py
# ```

# %%
import asyncio
import json
import os
import shutil
from collections.abc import Sequence
from datetime import UTC
from datetime import datetime
from decimal import Decimal
from getpass import getpass
from hashlib import sha256
from pathlib import Path
from tempfile import mkdtemp

import pandas as pd

from nautilus_trader.adapters.finfeed import FINFEED_CLIENT_ID
from nautilus_trader.adapters.finfeed import FinFeedDataClientConfig
from nautilus_trader.adapters.finfeed import FinFeedDataClientFactory
from nautilus_trader.adapters.finfeed import FinFeedInstrumentSelection
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.common import DataActor
from nautilus_trader.common import Environment
from nautilus_trader.common import LogLevel
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import DataCatalogConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import LoggerConfig
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.live import LiveNode
from nautilus_trader.model import AccountType
from nautilus_trader.model import BinaryOption
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import Quantity
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import OrderBookImbalanceConfig


# %% [markdown]
# ## Select the outcome and historical window
#
# FinFeed identifies each outcome separately. The `_YES` suffix selects
# the Yes shares, not the whole event or its No shares. Keep the provider's
# ID under `FINFEED_POLYMARKET`; native Polymarket execution uses different
# IDs. The adapter loads the checked `BinaryOption` definition, including
# its price increment, share precision, and `pUSD` currency.
# The definition reflects metadata available at collection time; it does
# not reconstruct historical venue trading rules.
# FinFeed sets this outcome's `EndDateIso` to 00:00 UTC on November 3.
# The adapter uses that value as its simulated trading cutoff, so this
# example selects November 2. The cutoff does not establish actual
# resolution time or payout.
#
# The first run reads `FINFEED_API_KEY` or prompts without echoing it.
# Later runs use the cached catalog without credentials or network access.
# Data stays under `NAUTILUS_DATA_DIR/FinFeed`, defaulting to
# `~/Downloads/Data/FinFeed`.
# In a notebook with an active event loop, replace
# `asyncio.run(collect_book(api_key))` below with `await collect_book(api_key)`.

# %%
MARKET_ID = "BITCOIN-ABOVE-76K-ON-NOVEMBER-3-2026_YES"
INSTRUMENT_ID = InstrumentId.from_str(f"{MARKET_ID}.FINFEED_POLYMARKET")
START = datetime(2026, 11, 2, 20, 0, 5, tzinfo=UTC)
END = datetime(2026, 11, 2, 20, 2, tzinfo=UTC)
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "FinFeed"
catalog_path = DATA_DIR / "btc-above-76k-nov3-20261102-20"
book_type = BookType.L2_MBP

# %% [markdown]
# ## Request historical deltas through the adapter
#
# Use a data-only `LiveNode` to load the definition and deliver the
# historical request. No execution client or live subscription is needed.
# Bootstrap uses two managed requests for exchange discovery and the
# outcome definition. History uses at least one more; busy intervals need
# continuation pages. Size the local attempt allowance for that workload.
# It does not establish provider billing units or remaining account quota.
#
# Book history starts with covering UTC-hour selectors and reconciles
# overlapping 10,000-record pages. Source order and exchange timestamps
# stay intact; a derived monotone `ts_init` controls replay bounds.
# It does not measure receipt time. The start hour must contain a snapshot
# at or before `START` on that clock. Missing seeds, ambiguous continuation,
# exhausted allowance, and unsupported actions reject the request. See
# [historical book limits](../integrations/finfeed.md#historical-books).
# The caller has its own response deadline because an asynchronous request
# failure logs an error without delivering a historical response.
# This run allows 120 seconds for bootstrap and 900 seconds for the full
# response. It uses the adapter's 300-second HTTP timeout and disables retries.


# %%
class HistoricalBook(DataActor):
    """
    Collect one complete historical response for the selected outcome.
    """

    def __init__(self) -> None:
        """
        Retain the returned deltas and completion signal.
        """
        super().__init__()
        self.deltas: list[OrderBookDelta] = []
        self.received = asyncio.Event()

    def on_start(self) -> None:
        """
        Request the selected window after the definition loads.
        """
        self.request_book_deltas(
            INSTRUMENT_ID,
            start=START,
            end=END,
            client_id=FINFEED_CLIENT_ID,
        )

    def on_historical_book_deltas(self, deltas: Sequence[OrderBookDelta]) -> None:
        """
        Retain the complete response in its supplied order.
        """
        self.deltas.extend(deltas)
        self.received.set()


async def collect_book(api_key: str) -> tuple[BinaryOption, list[OrderBookDelta]]:
    """
    Retrieve the definition and deltas, then stop and dispose the data node.
    """
    config = FinFeedDataClientConfig(
        api_key=api_key,
        instruments=[FinFeedInstrumentSelection("POLYMARKET", [MARKET_ID])],
        attempt_budget=4,
        min_request_interval_ms=10_000,
        max_concurrent_requests=1,
        max_retries=0,
    )
    data_node = (
        LiveNode.builder("FINFEED-HISTORICAL-BOOK", TraderId("TUTORIAL-001"), Environment.LIVE)
        .with_load_state(load_state=False)
        .with_save_state(save_state=False)
        .with_reconciliation(reconciliation=False)
        .with_timeout_connection(130)
        .with_timeout_portfolio(0)
        .with_logging(LoggerConfig(stdout_level=LogLevel.ERROR))
        .with_delay_post_stop_secs(0)
        .with_delay_shutdown_secs(0)
        .add_data_client(None, FinFeedDataClientFactory(), config)
        .build()
    )
    collector = HistoricalBook()
    data_node.add_actor(collector)
    handle = data_node.handle()
    task = asyncio.create_task(data_node.run_async())
    response = asyncio.create_task(collector.received.wait())
    try:
        await asyncio.wait([task, response], timeout=900, return_when=asyncio.FIRST_COMPLETED)
        if not response.done():
            if task.done():
                await task
            raise TimeoutError("FinFeed historical book response did not complete")
        instrument = collector.cache.instrument(INSTRUMENT_ID)
        if instrument is None or not collector.deltas:
            raise RuntimeError("Historical response has no definition or book deltas")
        return instrument, collector.deltas
    finally:
        response.cancel()
        handle.stop()
        try:
            await task
        finally:
            data_node.dispose()


def book_digest(deltas: Sequence[OrderBookDelta], instrument: BinaryOption) -> str:
    """
    Hash delta values with the instrument's precision, including zero CLEAR fields.
    """
    digest = sha256()

    for delta in deltas:
        record = delta.to_dict()
        record["order"]["price"] = format(
            delta.order.price.as_decimal(),
            f".{instrument.price_precision}f",
        )
        record["order"]["size"] = format(
            delta.order.size.as_decimal(),
            f".{instrument.size_precision}f",
        )
        digest.update(json.dumps(record, sort_keys=True).encode())
    return digest.hexdigest()


# %% [markdown]
# ## Cache the response and reconstruct the book
#
# Publish a new catalog only after the full request succeeds. A failed
# collection leaves no reusable cache. The manifest records the selection
# and a SHA-256 digest of the normalized deltas. Each cached replay checks
# that digest before running the strategy.
#
# The adapter initializes the series with `CLEAR` and snapshot levels.
# Apply deltas in their original order and check book integrity at every
# `F_LAST` boundary. Only completed batches supply the preview and signal.
#
# A successful reconstruction validates the returned events. It does not
# establish complete venue coverage: replay relies on the provider's
# snapshot and update semantics, and missing source events can leave gaps.
# FinFeed polling is a separate, sampled data path and is not used here.

# %%
DATA_DIR.mkdir(parents=True, exist_ok=True)
selection = {"market_id": MARKET_ID, "start": START.isoformat(), "end": END.isoformat()}

if not catalog_path.exists():
    api_key = os.environ.get("FINFEED_API_KEY") or getpass("FinFeed Prediction Markets API key: ")
    instrument, deltas = asyncio.run(collect_book(api_key))
    del api_key
    partial_path = Path(mkdtemp(prefix="finfeed-catalog-", dir=DATA_DIR))
    try:
        catalog = ParquetDataCatalog(str(partial_path))
        catalog.write_instruments([instrument])
        catalog.write_order_book_deltas(deltas)
        manifest = {**selection, "deltas": len(deltas), "sha256": book_digest(deltas, instrument)}
        (partial_path / "manifest.json").write_text(
            json.dumps(manifest, indent=2),
            encoding="utf-8",
        )
        del catalog
        partial_path.rename(catalog_path)
    finally:
        if partial_path.exists():
            shutil.rmtree(partial_path)

catalog = ParquetDataCatalog(str(catalog_path))
manifest = json.loads((catalog_path / "manifest.json").read_text(encoding="utf-8"))
assert {key: manifest[key] for key in selection} == selection
[instrument] = catalog.instruments(instrument_ids=[str(INSTRUMENT_ID)])
deltas = catalog.query_order_book_deltas(identifiers=[str(INSTRUMENT_ID)])
assert instrument.id == INSTRUMENT_ID
assert instrument.outcome == "Yes"
assert instrument.expiration_ns > pd.Timestamp(END).value
assert len(deltas) == manifest["deltas"]
assert deltas[0].action == BookAction.CLEAR
assert RecordFlag.F_SNAPSHOT.matches(deltas[0].flags)
assert RecordFlag.F_LAST.matches(deltas[-1].flags)

assert book_digest(deltas, instrument) == manifest["sha256"]
book = OrderBook(INSTRUMENT_ID, book_type)
top_of_book_rows = []
batch_count = 0

for delta in deltas:
    book.apply_delta(delta)
    if not RecordFlag.F_LAST.matches(delta.flags):
        continue
    book.check_integrity()
    batch_count += 1
    bid = book.best_bid_price()
    ask = book.best_ask_price()
    bid_size = book.best_bid_size()
    ask_size = book.best_ask_size()
    if bid is None or ask is None or bid_size is None or ask_size is None:
        continue
    if ask <= bid:
        continue
    bid_qty = bid_size.as_decimal()
    ask_qty = ask_size.as_decimal()
    top_of_book_rows.append(
        {
            "timestamp": delta.ts_init,
            "bid": bid.as_decimal(),
            "ask": ask.as_decimal(),
            "spread": ask.as_decimal() - bid.as_decimal(),
            "bid_size": bid_qty,
            "ask_size": ask_qty,
            "imbalance": (bid_qty - ask_qty) / (bid_qty + ask_qty),
        },
    )

assert batch_count > 1
assert top_of_book_rows
assert book.best_bid_price() is not None
assert book.best_ask_price() is not None
assert book.spread() > 0

top_of_book = pd.DataFrame(top_of_book_rows)
top_of_book.index = pd.to_datetime(top_of_book.pop("timestamp"), utc=True)
print(f"Catalog: {catalog_path}")
print(f"Instrument: {instrument.id}, outcome: {instrument.outcome}")
print(f"Loaded {len(deltas):,} deltas in {batch_count:,} completed batches")
print(f"Delta SHA-256: {manifest['sha256']}")
print(top_of_book.head().to_string())
print(f"Spread range: {top_of_book['spread'].min()} to {top_of_book['spread'].max()} pUSD")

# %% [markdown]
# ## Configure the backtest
#
# Use the built-in Rust `OrderBookImbalance` through Python, as in the other book tutorials.
# When the smaller best-quote size is below 20% of the larger and the
# larger exceeds 100 shares, it sends a fill-or-kill limit order against
# the thinner side. Each order is at most one share, with at least one
# second between triggers.
#
# Start a cash account with 1,000 pUSD. Buys acquire Yes shares; sells
# reduce inventory. The matching engine rejects a sell that exceeds the
# shares held. The strategy can therefore produce rejected orders when
# its signal asks to sell without inventory. An opposing signal does not
# buy No shares. The final market sell closes any remaining Yes position.
#
# Enable `emit_quotes_from_book` so the risk engine can price the closing
# market order. This teaching run sets maker and taker fees to zero and
# assumes zero execution latency. It does not model live order routing,
# venue fees, queue position, or settlement. Simulated fills and PnL do
# not establish an edge or predict live execution.

# %%
config = BacktestRunConfig(
    engine=BacktestEngineConfig(
        logging=LoggerConfig(stdout_level=LogLevel.ERROR),
        data_engine=DataEngineConfig(emit_quotes_from_book=True),
    ),
    data=[
        BacktestDataConfig(
            catalog=DataCatalogConfig(path=str(catalog_path)),
            data_type=NautilusDataType.OrderBookDelta,
            instrument_id=instrument.id,
        ),
    ],
    venues=[
        BacktestVenueConfig(
            name="FINFEED_POLYMARKET",
            oms_type=OmsType.NETTING,
            account_type=AccountType.CASH,
            base_currency=instrument.currency,
            starting_balances=["1000 pUSD"],
            book_type=book_type,
            fee_model=MakerTakerFeeModel(maker_rate=Decimal(0), taker_rate=Decimal(0)),
        ),
    ],
    dispose_on_completion=False,
)
strategy_config = OrderBookImbalanceConfig(
    instrument_id=instrument.id,
    book_type=book_type,
    max_trade_size=Quantity.from_str("1.000000"),
    trigger_min_size=Decimal("100.0"),
    trigger_imbalance_ratio=Decimal("0.20"),
    min_seconds_between_triggers=1.0,
)

# %% [markdown]
# ## Run the backtest and inspect the results
#
# The reference run loads **13,900 deltas in 13,774 completed batches**
# from November 2, 2026, between 20:00:05 and 20:02:00 UTC. The spread
# ranges from 0.0050 to 0.0230 pUSD. Its normalized delta SHA-256 is:
#
# ```text
# df25d4f73bb70f3543407902776e4acaeb26cb630cef8d6bf1b21e0060322230
# ```
#
# Of 37 orders, two fill and 35 are rejected. The fills are:
#
# | Time (UTC)         | Side | Shares   | Price (pUSD) |
# | ------------------ | ---- | -------- | ------------ |
# | 20:00:58.591842700 | Buy  | 1.000000 | 0.7620       |
# | 20:01:02.811306200 | Sell | 1.000000 | 0.7440       |
#
# Both are fill-or-kill limit orders. The sell closes the position before
# the replay ends, so shutdown sends no closing market order. The final
# balance is **999.982000 pUSD**, a realized loss of **0.018000 pUSD**
# with zero fees. Compare your digest before comparing these results;
# refreshed provider history or metadata can change the run.
#
# Replay the full cached series so the initial snapshot precedes updates.
# Inspect order statuses, fills, positions, and cash balances together.
# Rejected orders have no fills, and the final position must be flat.
# Keep the printed data digest with your results when comparing runs.

# %%
node = BacktestNode(configs=[config])
try:
    node.build()
    node.add_builtin_strategy(config.id, "OrderBookImbalance", strategy_config)
    results = node.run()
    orders = node.generate_orders_report(config.id)
    executions = node.generate_fills_report(config.id)
    positions = node.generate_positions_report(config.id)
    account = node.generate_account_report(config.id, venue=Venue("FINFEED_POLYMARKET"))
    print(results)
    print(orders["status"].value_counts().to_string())
    print(executions[["order_side", "order_type", "last_qty", "last_px", "commission"]].head())
    print(
        positions[
            ["side", "quantity", "peak_qty", "avg_px_open", "avg_px_close", "realized_pnl"]
        ].to_string(),
    )
    print(account[["total", "locked", "free", "currency"]].tail().to_string())
    assert not positions.empty, "No positions opened; inspect the selected window and signal"
    assert positions["ts_closed"].notna().all(), "The strategy left an open position"
    assert positions["side"].eq("FLAT").all()
    assert positions["quantity"].map(Decimal).eq(Decimal(0)).all()
finally:
    node.dispose()

# %% [markdown]
# ## Visualize the run
#
# These panels use the same cached series, configuration, and native
# reports as the backtest above. Lines show successive states as steps;
# fill markers show individual executions.
#
# ![Best bid and ask with the one-share buy and sell fills][panel_book]
#
# **Figure 1.** The strategy buys at the ask and sells at the bid about
# four seconds later. Both executions use the reconstructed book.
#
# ![Spread, best-quote sizes, and signed size imbalance over the replay][panel_liquidity]
#
# **Figure 2.** Large ask sizes often favor a sell signal, but this cash
# account can only sell shares it holds. The plotted signed imbalance is
# `(bid_size - ask_size) / (bid_size + ask_size)`; the strategy compares
# the smaller size with the larger size instead.
#
# ![Yes-share inventory rises to one share and returns to zero][panel_position]
#
# **Figure 3.** The position holds one Yes share for about four seconds
# and stays flat for the rest of the replay.
#
# ![Equity marked at the midpoint ends at a loss of 0.018000 pUSD][panel_pnl]
#
# **Figure 4.** The equity panel adds cash to the shares held at the
# current midpoint. Cash changes from buying shares are not
# realized losses. Once the position closes, the equity change equals
# this run's net PnL.
#
# ### Regenerate the panels
#
# Panel generation uses Plotly and Kaleido. Kaleido also requires a
# Chrome-compatible browser. From a source checkout, run:
#
# ```bash
# uv run --project python --no-sync --with pandas --with plotly --with kaleido \
#     python docs/tutorials/render_finfeed_panels.py
# ```
#
# If Kaleido reports that Chrome is missing, install its browser once with:
#
# ```bash
# uv run --project python --no-sync --with plotly --with kaleido plotly_get_chrome -y
# ```
#
# The renderer writes PNGs to
# `docs/tutorials/assets/backtest_orderbook_finfeed/`. It reuses the cache
# or follows the first-collection procedure above. All calculations use
# exact decimals until conversion to plotting coordinates.
#
# [panel_book]: ./assets/backtest_orderbook_finfeed/panel_a_top_book.png
# [panel_liquidity]: ./assets/backtest_orderbook_finfeed/panel_b_liquidity.png
# [panel_position]: ./assets/backtest_orderbook_finfeed/panel_c_position.png
# [panel_pnl]: ./assets/backtest_orderbook_finfeed/panel_d_pnl.png
