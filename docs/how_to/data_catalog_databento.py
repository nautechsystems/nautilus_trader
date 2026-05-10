# %% [markdown]
# # Data Catalog with Databento
#
# Set up a Nautilus Parquet data catalog with market data from Databento. The
# catalog provides efficient storage and querying for backtests and research.
#
# [View source on GitHub](https://github.com/nautechsystems/nautilus_trader/blob/develop/docs/how_to/data_catalog_databento.py).

# %% [markdown]
# ## Prerequisites
#
# - Python 3.12+
# - [NautilusTrader](https://pypi.org/project/nautilus_trader/) latest release installed (`pip install nautilus_trader`)
# - [databento](https://pypi.org/project/databento/) Python client library (`pip install databento`)
# - [Databento](https://databento.com) account with API key set as `DATABENTO_API_KEY`

# %% [markdown]
# ## Request data
#
# Initialize a Databento historical client. The client reads your API key from
# the `DATABENTO_API_KEY` environment variable by default.

# %%
import databento as db


client = db.Historical()  # Uses the DATABENTO_API_KEY environment variable

# %% [markdown]
# **Every historical streaming request from `timeseries.get_range` incurs a cost (even for the same data), so**:
# - Check the cost before making a request
# - Avoid requesting the same data twice
# - Write responses to disk as zstd compressed DBN files

# %% [markdown]
# Use the metadata [get_cost endpoint](https://databento.com/docs/api-reference-historical/metadata/metadata-get-cost?historical=python&live=python) to quote the cost before each request. Only request data that does not already exist on disk.
#
# The response is in USD, displayed as fractional cents.

# %% [markdown]
# The following request is for a small amount of data (as used in this Medium article [Building high-frequency trading signals in Python with Databento and sklearn](https://databento.com/blog/hft-sklearn-python)) to demonstrate the workflow.

# %%
from pathlib import Path

from databento import DBNStore

# %% [markdown]
# We'll prepare a directory for the raw Databento DBN format data, which we'll use for the rest of the tutorial.

# %%
DATABENTO_DATA_DIR = Path("databento")
DATABENTO_DATA_DIR.mkdir(exist_ok=True)

# %%
# Request cost quote (USD) - this endpoint is 'free'
client.metadata.get_cost(
    dataset="GLBX.MDP3",
    symbols=["ES.n.0"],
    stype_in="continuous",
    schema="mbp-10",
    start="2023-12-06T14:30:00",
    end="2023-12-06T20:30:00",
)

# %% [markdown]
# Use the historical API to request the data used in the Medium article.

# %%
path = DATABENTO_DATA_DIR / "es-front-glbx-mbp10.dbn.zst"

if not path.exists():
    # Request data
    client.timeseries.get_range(
        dataset="GLBX.MDP3",
        symbols=["ES.n.0"],
        stype_in="continuous",
        schema="mbp-10",
        start="2023-12-06T14:30:00",
        end="2023-12-06T20:30:00",
        path=path,  # <-- Passing a `path` writes the data to disk
    )

# %% [markdown]
# Read the data from disk and convert to a pandas.DataFrame

# %%
data = DBNStore.from_file(path)

df = data.to_df()
df

# %% [markdown]
# ## Write to data catalog

# %%
import shutil
from pathlib import Path

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.model import InstrumentId
from nautilus_trader.persistence.catalog import ParquetDataCatalog

# %%
CATALOG_PATH = Path.cwd() / "catalog"

# Clear if it already exists
if CATALOG_PATH.exists():
    shutil.rmtree(CATALOG_PATH)
CATALOG_PATH.mkdir()

# Create a catalog instance
catalog = ParquetDataCatalog(CATALOG_PATH)

# %% [markdown]
# Use a `DatabentoDataLoader` to decode and load the data into Nautilus objects.

# %%
loader = DatabentoDataLoader()

# %% [markdown]
# Load Rust PyO3 objects by setting `as_legacy_cython=False`.
#
# Passing an `instrument_id` is optional but speeds up loading by skipping symbology mapping. If provided, use the Nautilus `symbol.venue` format (e.g., "ES.GLBX").

# %%
path = DATABENTO_DATA_DIR / "es-front-glbx-mbp10.dbn.zst"

# Option 1 (recommended): Let the loader infer the instrument ID from DBN metadata
depth10 = loader.from_dbn_file(
    path=path,
    as_legacy_cython=False,
)

# Option 2: Explicitly specify a valid Nautilus instrument ID (symbol.venue format)
# instrument_id = InstrumentId.from_str("ESZ3.GLBX")  # E-mini S&P December 2023 futures on Globex
# depth10 = loader.from_dbn_file(
#     path=path,
#     instrument_id=instrument_id,
#     as_legacy_cython=False,
# )

# %%
# Write data to catalog (this takes ~20 seconds or ~250,000/second for writing MBP-10 at the moment)
catalog.write_data(depth10)

# %%
# Test reading from catalog
depths = catalog.order_book_depth10()
len(depths)

# %% [markdown]
# ## Preparing a month of AAPL trades

# %% [markdown]
# Now we'll expand on this workflow by preparing a month of AAPL trades on the Nasdaq exchange using the Databento `trade` schema, which will translate to Nautilus `TradeTick` objects.

# %%
# Request cost quote (USD) - this endpoint is 'free'
client.metadata.get_cost(
    dataset="XNAS.ITCH",
    symbols=["AAPL"],
    schema="trades",
    start="2024-01",
)

# %% [markdown]
# Pass a `path` parameter when requesting historical data to write it to disk.

# %%
path = DATABENTO_DATA_DIR / "aapl-xnas-202401.trades.dbn.zst"

if not path.exists():
    # Request data
    client.timeseries.get_range(
        dataset="XNAS.ITCH",
        symbols=["AAPL"],
        schema="trades",
        start="2024-01",
        path=path,  # <-- Passing a `path` parameter
    )

# %% [markdown]
# Read the data from disk and convert to a pandas.DataFrame

# %%
data = DBNStore.from_file(path)

df = data.to_df()
df

# %% [markdown]
# We'll use an `InstrumentId` of `"AAPL.XNAS"`, where XNAS is the ISO 10383 MIC (Market Identifier Code) for the Nasdaq venue.
#
# Passing an `instrument_id` speeds up loading by skipping symbology mapping. Setting `as_legacy_cython=False` is more efficient when writing to the catalog.

# %%
instrument_id = InstrumentId.from_str("AAPL.XNAS")

trades = loader.from_dbn_file(
    path=path,
    instrument_id=instrument_id,
    as_legacy_cython=False,
)

# %% [markdown]
# Here we organize data as one file per month. A file per day works equally well.

# %%
# Write data to catalog
catalog.write_data(trades)

# %%
trades = catalog.trade_ticks([instrument_id])

# %%
len(trades)
