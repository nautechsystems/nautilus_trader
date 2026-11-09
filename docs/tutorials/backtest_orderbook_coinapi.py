# %% [markdown]
# # Backtest with Pre-IPO Order Book Data (CoinAPI)
#
# Download one hour of Lighter ANTHROPIC pre-IPO perpetual order book data
# from CoinAPI, load it into a Parquet catalog, and run an order book
# imbalance backtest. The download is cached locally, so later runs work
# offline.
#
# This market provides synthetic exposure to Anthropic's pre-IPO valuation.
# Lighter normalizes the contract using an assumed share count of one
# billion and uses an internal pricing mechanism. The perpetual does not
# give ownership of company shares. See Lighter's
# [market specifications](https://docs.lighter.xyz/trading/real-world-assets-rwas/market-specifications)
# and [pre-IPO market rules](https://docs.lighter.xyz/trading/real-world-assets-rwas/pre-ipo-markets).
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/tutorials/backtest_orderbook_coinapi.py).

# %% [markdown]
# ## Prerequisites
#
# - Python 3.13, 3.14, or 3.15 and NautilusTrader 2.x with the CoinAPI adapter.
#   For Python 3.13 or 3.14, run `pip install -U --pre nautilus_trader`.
#   For Python 3.15, use a [source build](../getting_started/installation.md#from-source)
#   or [development wheel](../getting_started/installation.md#development-wheels).
#   Install the data packages with `pip install boto3 pandas`.
# - A CoinAPI key with Flat Files access and sufficient usage credits for
#   the first download. Listing and downloading files incur charges; see
#   [Flat Files access](https://www.coinapi.io/products/flat-files/docs?ref=nautilus).
#
# From a source checkout, run the tutorial from the repository root after
# building the Python extension:
#
# ```bash
# uv run --project python --no-sync --with boto3 --with pandas \
#     python docs/tutorials/backtest_orderbook_coinapi.py
# ```

# %%
import os
import shutil
from decimal import Decimal
from getpass import getpass
from hashlib import file_digest
from pathlib import Path
from tempfile import mkdtemp

import boto3
import pandas as pd
from botocore.config import Config

from nautilus_trader.adapters.coinapi import CoinApiDataLoader
from nautilus_trader.backtest import BacktestNode
from nautilus_trader.common import LogLevel
from nautilus_trader.config import BacktestDataConfig
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import BacktestVenueConfig
from nautilus_trader.config import DataCatalogConfig
from nautilus_trader.config import DataEngineConfig
from nautilus_trader.config import LoggerConfig
from nautilus_trader.execution import MakerTakerFeeModel
from nautilus_trader.model import AccountType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookType
from nautilus_trader.model import CryptoPerpetual
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderBook
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import StandardMarginModel
from nautilus_trader.model import Symbol
from nautilus_trader.model import Venue
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.trading import OrderBookImbalanceConfig


# %% [markdown]
# ## Download the full-book file
#
# Use `T-LIMITBOOK_FULL` for a snapshot followed by all recorded updates.
# The selected file covers `[00:00, 01:00)` UTC on 2026-10-01 and is about
# 279 KB compressed. Its CoinAPI symbol is `LIGHTER_PERP_ANTHROPIC_USD`.
#
# CoinAPI stores this hour in the historical `coinapi` bucket. The
# `coinapi-daily-tail` bucket retains only the previous day's daily files.
# See the [full-book file layout](https://www.coinapi.io/products/flat-files/docs/datasets/limitbook?ref=nautilus)
# when selecting another partition.
#
# The first run reads `COINAPI_API_KEY` or prompts for the key without
# echoing it. CoinAPI uses that key as the S3 access key ID and the literal
# `coinapi` as the secret access key; see
# [S3 authentication](https://www.coinapi.io/products/flat-files/docs/s3-api/authentication?ref=nautilus).
# Files go under `NAUTILUS_DATA_DIR/CoinAPI`, which defaults to
# `~/Downloads/Data/CoinAPI`.
# The checksum pins the exact export used for the results below. A
# mismatch stops the run before parsing; verify the file before replacing it.

# %%
DATA_DIR = Path(os.environ.get("NAUTILUS_DATA_DIR", "~/Downloads/Data")).expanduser() / "CoinAPI"
OBJECT_KEY = (
    "T-LIMITBOOK_FULL/D-2026100100/E-LIGHTER/"
    "IDDI-47534074+SC-LIGHTER_PERP_ANTHROPIC_USD+S-ANTHROPIC.csv.gz"
)
FILE_SHA256 = "2ed141be6480c56ff16322e08bbc43398edc710875fe81093e0635ff672e48e4"
filepath = DATA_DIR / OBJECT_KEY

if not filepath.is_file():
    api_key = os.environ.get("COINAPI_API_KEY") or getpass("CoinAPI Flat Files API key: ")
    client = boto3.client(
        "s3",
        endpoint_url="https://s3.flatfiles.coinapi.io",
        region_name="us-east-1",
        aws_access_key_id=api_key,
        aws_secret_access_key="coinapi",  # noqa: S106 (CoinAPI's public signing constant)
        config=Config(s3={"addressing_style": "path"}),
    )
    filepath.parent.mkdir(parents=True, exist_ok=True)
    response = client.get_object(Bucket="coinapi", Key=OBJECT_KEY)
    partial_path = filepath.with_suffix(".part")
    with response["Body"] as source, partial_path.open("wb") as destination:
        shutil.copyfileobj(source, destination)
    partial_path.replace(filepath)
    client.close()
    del api_key, client, response

with filepath.open("rb") as source:
    if file_digest(source, "sha256").hexdigest() != FILE_SHA256:
        raise ValueError(f"Unexpected SHA-256 for {filepath}; verify the selected export")

print(f"Data file: {filepath}")

# %% [markdown]
# ## Define the Lighter contract
#
# `CoinApiDataLoader` uses a registered instrument definition. It does not
# need the live client's instrument mapper, which does not support Lighter.
#
# Use the native ID `ANTHROPIC-PERP.LIGHTER` and USDC for the quote and
# settlement currency. `CryptoPerpetual` matches the native Lighter
# adapter's model for this contract. CoinAPI's `USD` symbol suffix does not
# change the native contract's settlement currency. Prices have one
# decimal place and sizes have five in this export, matching the venue's
# tick and lot increments.
#
# For this simulation, set a minimum order of 0.00250 contract units, a
# minimum notional of 10 USDC, initial margin of 20%, and maintenance margin
# of 12%.
# These limits and margin rates are assumptions based on native metadata,
# not a reconstruction of historical venue rules. Verify the terms for
# your research period before drawing conclusions from PnL.

# %%
USDC = Currency.from_str("USDC")

instrument = CryptoPerpetual(
    instrument_id=InstrumentId.from_str("ANTHROPIC-PERP.LIGHTER"),
    raw_symbol=Symbol("ANTHROPIC"),
    base_currency=Currency.from_str("ANTHROPIC"),
    quote_currency=USDC,
    settlement_currency=USDC,
    is_inverse=False,
    price_precision=1,
    size_precision=5,
    price_increment=Price.from_str("0.1"),
    size_increment=Quantity.from_str("0.00001"),
    min_quantity=Quantity.from_str("0.00250"),
    min_notional=Money.from_decimal(Decimal(10), USDC),
    margin_init=Decimal("0.20"),
    margin_maint=Decimal("0.12"),
    ts_event=0,
    ts_init=0,
)

book_type = BookType.L2_MBP

# %% [markdown]
# ## Reconstruct the book and write the catalog
#
# This file has empty order IDs, so it describes L2 market-by-price levels.
# The loader emits a `CLEAR` and snapshot levels, followed by incremental
# updates. `F_LAST` marks the end of each complete event.
#
# Register the contract with `CoinApiDataLoader` so every chunk uses its
# price and size precision. Local loading requires no credentials.
# Stream the entire file in its original order. Do not set a row `limit`:
# it can cut a snapshot or update event short. Streaming chunks preserve
# event boundaries and may exceed `chunk_size`. The loader derives the UTC
# date from the file's `D-2026100100` directory; pass `date` explicitly if
# you rename or move the file outside that partition.
#
# Each run creates a separate catalog under `DATA_DIR` and prints its path.
# Existing catalogs remain available for later research.
#
# Level updates in CoinAPI exports can have separate timestamps. `F_LAST`
# marks a loader event, not an atomic venue update. Other partitions can
# briefly produce locked or crossed books while applying level changes.
# Keep every delta in the catalog, but require a positive spread for both
# the preview below and the strategy's trading decisions.
# The opening snapshot and final book must pass integrity checks before
# running the backtest.

# %%
catalog_path = Path(mkdtemp(prefix="lighter-anthropic-catalog-", dir=DATA_DIR))
catalog = ParquetDataCatalog(str(catalog_path))
catalog.write_instruments([instrument])

book = OrderBook(instrument.id, book_type)
top_of_book_rows = []
delta_count = 0
nonpositive_spread_count = 0

loader = CoinApiDataLoader()
loader.add_instrument(instrument)

for chunk in loader.stream_book_deltas(
    filepath,
    instrument.id,
    chunk_size=100_000,
):
    if delta_count == 0:
        assert chunk[0].action == BookAction.CLEAR
        assert chunk[0].flags & RecordFlag.F_SNAPSHOT.value
    assert chunk[-1].flags & RecordFlag.F_LAST.value

    catalog.write_order_book_deltas(chunk)
    delta_count += len(chunk)

    for delta in chunk:
        book.apply_delta(delta)
        if not delta.flags & RecordFlag.F_LAST.value:
            continue

        if delta.flags & RecordFlag.F_SNAPSHOT.value:
            book.check_integrity()
        bid = book.best_bid_price()
        ask = book.best_ask_price()
        bid_size = book.best_bid_size()
        ask_size = book.best_ask_size()
        if bid is None or ask is None or bid_size is None or ask_size is None:
            continue

        spread = ask.as_decimal() - bid.as_decimal()
        if spread <= 0:
            nonpositive_spread_count += 1
            continue

        top_of_book_rows.append(
            {
                "ts_init": delta.ts_init,
                "bid": bid.as_decimal(),
                "ask": ask.as_decimal(),
                "spread": spread,
                "bid_size": bid_size.as_decimal(),
                "ask_size": ask_size.as_decimal(),
            },
        )

top_of_book = pd.DataFrame(top_of_book_rows)
top_of_book.index = pd.to_datetime(top_of_book.pop("ts_init"), unit="ns", utc=True)
book.check_integrity()
print(f"Catalog: {catalog_path}")
print(f"Loaded {delta_count:,} deltas, including the snapshot CLEAR")
print(f"Excluded {nonpositive_spread_count:,} locked or crossed states from the preview")
print(top_of_book.head())
print(f"Spread range: {top_of_book['spread'].min()} to {top_of_book['spread'].max()} USDC")

# %% [markdown]
# ## Configure the backtest
#
# Replay the catalog with `BacktestNode`. The engine reads the initial
# snapshot before updates, so it starts with a complete book. Starting a
# replay partway through the hour would omit that snapshot.
#
# Enable `emit_quotes_from_book` so the data engine caches top-of-book
# quotes derived from the deltas. The risk engine needs these quotes to
# price the strategy's market exit. Without them, it denies the closing
# order with `MARKET_PRICE_UNAVAILABLE` and leaves the position open.
#
# Use the built-in Rust `OrderBookImbalance` through Python, as in the Binance and Bybit tutorials.
# When one best-quote size is less than 20% of the other and the larger exceeds
# one contract unit, the strategy submits a fill-or-kill limit order
# against the thinner side. Each order is at most 0.01 contract units,
# with at least one second between triggers. The strategy closes its
# position when it stops.
#
# Use a simulated margin account with 100,000 USDC. `StandardMarginModel`
# applies the instrument's 20% and 12% rates without a leverage reduction.
# Maker and taker fees are explicitly zero for this teaching run.
# Funding, network latency,
# transaction costs, and IPO conversion or settlement are omitted.
# L2 data cannot establish individual order queue positions, and simulated
# fills do not predict live execution.
# This strategy demonstrates the data workflow; it has no demonstrated edge.

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
            name="LIGHTER",
            oms_type=OmsType.NETTING,
            account_type=AccountType.MARGIN,
            starting_balances=["100000 USDC"],
            book_type=book_type,
            margin_model=StandardMarginModel(),
            fee_model=MakerTakerFeeModel(
                maker_rate=Decimal(0),
                taker_rate=Decimal(0),
            ),
        ),
    ],
    dispose_on_completion=False,
)

strategy_config = OrderBookImbalanceConfig(
    instrument_id=instrument.id,
    book_type=book_type,
    max_trade_size=Quantity.from_str("0.01000"),
    trigger_min_size=Decimal("1.0"),
    trigger_imbalance_ratio=Decimal("0.20"),
    min_seconds_between_triggers=1.0,
)

# %% [markdown]
# ## Run the backtest and inspect the results
#
# The selected file produces 15,047 deltas. The default `F_LAST` batching
# replays the 234-delta opening snapshot as one event, giving 14,814
# backtest iterations. This configuration submits 234 orders, all filled.
# Of these, 233 are fill-or-kill limit orders: 232 sells and one buy.
# They build a short position with a peak size of 2.30689 contract units.
# A final market buy closes it. The average opening and closing prices
# are about 2120.30 and 2120.80 USDC, giving a realized loss of 1.157298 USDC
# and a final balance of 99,998.84270200 USDC under these assumptions.
#
# The reports show filled orders, positions, and account balances. Inspect
# commissions and final position exposure as well as PnL. The account
# report records balance changes; the position report contains realized
# PnL for closed positions. Order status counts include orders with no
# fills. A successful replay establishes that the data pipeline works,
# not that this strategy is profitable.

# %%
node = BacktestNode(configs=[config])
node.build()
node.add_builtin_strategy(config.id, "OrderBookImbalance", strategy_config)
results = node.run()

print(results)
orders = node.generate_orders_report(config.id)
fills = node.generate_order_fills_report(config.id)
executions = node.generate_fills_report(config.id)
positions = node.generate_positions_report(config.id)
account = node.generate_account_report(config.id, venue=Venue("LIGHTER"))
print(orders["status"].value_counts().to_string())
print(fills[["side", "filled_qty", "avg_px"]].head().to_string())
print(
    positions[
        [
            "entry",
            "side",
            "quantity",
            "peak_qty",
            "avg_px_open",
            "avg_px_close",
            "realized_pnl",
            "commissions",
        ]
    ].to_string(),
)
print(account[["total", "locked", "free", "currency"]].tail().to_string())

# %%
node.dispose()

# %% [markdown]
# ## Visualize the run
#
# These panels use the same pinned export, complete hour, configuration,
# and native reports as the backtest above. Lines show successive states
# as steps; fill markers show individual executions.
#
# ![ANTHROPIC best bid and ask with FOK and closing market fills](
# ./assets/backtest_orderbook_coinapi/panel_a_top_book.png)
#
# **Figure 1.** The FOK sells build the short position. One FOK buy reduces
# it, and the final market buy closes it through six fills at the same
# simulated timestamp. The closing fills can execute above the best ask
# because the order consumes several price levels. Sharp quote spikes
# include intermediate states lasting microseconds between recorded level
# updates. This zero-latency strategy can fill at these prices.
#
# ![Full-hour spread and best-quote sizes on a logarithmic depth axis](
# ./assets/backtest_orderbook_coinapi/panel_b_liquidity.png)
#
# **Figure 2.** Across all states, the spread is in `[0.1, 2.1]` USDC.
# For states lasting at least one millisecond, it is in `[0.1, 0.9]` USDC.
# Durations use receipt time until the next recorded update: 7,342 of
# 14,813 measured intervals are shorter than 10 microseconds. Best-quote
# sizes vary across several orders of magnitude, so depth uses a
# logarithmic axis. Sizes are contract units, not company shares.
#
# ![Net short position reaching 2.30689 units before closing flat](
# ./assets/backtest_orderbook_coinapi/panel_c_position.png)
#
# **Figure 3.** Cumulative signed execution quantities show the short
# reaching 2.30689 units. The final market order returns exposure to zero.
#
# ![Realized loss of 1.157298 USDC and final account balance](
# ./assets/backtest_orderbook_coinapi/panel_d_pnl.png)
#
# **Figure 4.** The account's total balance change gives realized PnL for
# this zero-fee run. The final drop books the loss accumulated while
# holding the short, plus the cost of consuming several ask levels to
# close. Unrealized PnL during the holding period is omitted.
#
# ### Regenerate the panels
#
# Panel generation uses Plotly and Kaleido. Kaleido also requires a
# Chrome-compatible browser. From a source checkout, run:
#
# ```bash
# uv run --project python --no-sync --with boto3 --with pandas \
#     --with plotly --with kaleido \
#     python docs/tutorials/assets/backtest_orderbook_coinapi/render_panels.py
# ```
#
# If Kaleido reports that Chrome is missing, install its browser once with:
#
# ```bash
# uv run --project python --no-sync --with plotly --with kaleido plotly_get_chrome -y
# ```
#
# The renderer runs this tutorial and writes PNGs to
# `docs/tutorials/assets/backtest_orderbook_coinapi/`. It reuses the cached
# export or follows the first-download procedure above.

# %% [markdown]
# To test another hour, list its available objects and select the ANTHROPIC file
# using CoinAPI's
# [S3 SDK guide](https://www.coinapi.io/products/flat-files/docs/s3-api/sdk?ref=nautilus).
# Update `OBJECT_KEY` and `FILE_SHA256` together after verifying the new file.
# Check the file schema, opening snapshot, and instrument precision before
# loading it. The [CoinAPI integration guide](../integrations/coinapi.md#flat-files)
# describes the accepted CSV layout and timestamp rules.
