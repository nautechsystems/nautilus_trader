# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#       jupytext_version: 1.17.3
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# Note: Use the jupytext python package to be able to open this python file in jupyter as a notebook.
# Also run `jupytext-config set-default-viewer` to open jupytext python files as notebooks by default.

# %% [markdown]
# # Databento overview
#
# Databento documentation:
#
# * [https://databento.com/docs](https://databento.com/docs)
#
# ## 3 services
#
# Databento provides 3 types of services:
#
# 1. `Historical` - for market data data older than 24 hours
# 2. `Live` - for market data within the last 24 hours
# 3. `Reference` - for security master and corporate actions data
#
# ## 3 file formats
#
# Databento supports 3 formats for data:
#
# * `DBN` - Databento Binary Encoding (binary)
# * `csv` - comma separated values (text)
# * `json` - JavaScript Object notation (text)
#
# ## Python library
#
# Databento provides a simple Python library (used in this tutorial):
#
# `pip install -U databento`

# %% [markdown]
# ## Schemas
#
# Schema is just a sophisticated name for `type of data` you want.
#
# Most used schemas ordered from most detailed:
#
# | Schema | Type | Description |
# |--------|------|-------------|
# | `mbo` | L3 data | Provides every order book event across every price level, keyed by order ID. Allows determination of queue position for each order, offering highest level of granularity available. |
# | `mbp-10` | L2 data | Provides every order book event across top ten price levels, keyed by price. Includes trades and changes to aggregate market depth, with total size and order count at top ten price levels. |
# | `mbp-1` | L1 data | Provides every order book event updating the top price level (BBO). Includes trades and changes to book depth, with total size and order count at BBO. |
# | `bbo-1s` | L1 sampled | Similar to L1 data but sampled in 1 second intervals. Provides last best bid, best offer, and sale at 1-second intervals. |
# | `tbbo` | L1 trades | Provides every trade event alongside the BBO immediately before the effect of each trade. Subset of MBP-1. |
# | `trades` | Trade data | Provides every trade event. This is a subset of MBO data. |
# | `ohlcv-1s` | 1s bars | OHLCV bars aggregated from trades at 1-second intervals. |
# | `ohlcv-1m` | 1m bars | OHLCV bars aggregated from trades at 1-minute intervals. |
# | `ohlcv-1h` | 1h bars | OHLCV bars aggregated from trades at 1-hour intervals. |
# | `ohlcv-1d` | 1d bars | OHLCV bars aggregated from trades at 1-day intervals. |
# | `definition` | Reference | Provides reference information about instruments including symbol, name, expiration date, listing date, tick size, strike price. |
# | `status` | Exchange status | Provides updates about trading session like halts, pauses, short-selling restrictions, auction start, and other matching engine statuses. |
# | `statistics` | Exchange stats | Provides official summary statistics published by venue, including daily volume, open interest, settlement prices, and official open/high/low prices. |
#
# **How Databento generates lower-resolution data?**
#
# 1. Databento first collects the most detailed market data available from each source (mostly `mbo` if available)
# 2. and then derives all other formats from this most granular data to ensure 100% consistency across all data types (schemas).
#
# Additional sources:
#
# * Example tutorial how to convert tick/trades data into bars:
#     * [https://databento.com/docs/examples/basics-historical/tick-resampling/example](https://databento.com/docs/examples/basics-historical/tick-resampling/example)
# * All schemas explained in detail:
#     * [https://databento.com/docs/schemas-and-data-formats?historical=python&live=python&reference=python](https://databento.com/docs/schemas-and-data-formats?historical=python&live=python&reference=python)

# %% [markdown] jp-MarkdownHeadingCollapsed=true
# ## Symbology
#
# Symbology is just a sophisticated name for the naming convention of various instruments. Abbreviation `stypes` is often used in API and docs and means "symbology types".
#
# Databento supports 4 symbology types (naming conventions):
#
# | Symbology Type    | Description                                      | Example/Pattern                | Key Notes                                                                   |
# |:-----------------|:-------------------------------------------------|:------------------------------|:----------------------------------------------------------------------------|
# | `raw_symbol`     | Original string symbols used by data publisher    | `AAPL`, `ESH3`                | Best for direct market connectivity environments                             |
# | `instrument_id`  | Unique numeric IDs assigned by publisher          | `12345`, `9876543`            | Space-efficient but can be remapped daily by some publishers                 |
# | `parent`         | Groups related symbols using root symbol          | `ES.FUT`, `ES.OPT`            | Allows querying all futures/options for a root symbol at once                |
# | `continuous`     | References instruments that change over time      | `ES.c.0`, `CL.n.1`, `ZN.v.0`  | Roll rules: Calendar (c), Open Interest (n), Volume (v)                      |
#
# Additionally, Databento supports a special symbol value:
#
# | Special Value     | Description                                      | Usage                          | Key Notes                                                                   |
# |:-----------------|:-------------------------------------------------|:------------------------------|:----------------------------------------------------------------------------|
# | `ALL_SYMBOLS`    | Requests all symbols in dataset                  | `ALL_SYMBOLS`                  | Wildcard value for requesting all available symbols (not a symbology type) |
#
#
# When requesting data, **input** and **output** symbology can be specified. These 4 combinations are supported (for various exchanges / publishers):
#
# | SType in    | SType out      | DBEQ.BASIC | GLBX.MDP3 | IFEU.IMPACT | NDEX.IMPACT | OPRA.PILLAR | XNAS.ITCH |
# |:---------------|:-----------------|:-----------|:----------|:------------|:------------|:------------|:----------|
# | `parent`       | `instrument_id`  |            | ✓         | ✓           | ✓           | ✓           |           |
# | `continuous`   | `instrument_id`  |            | ✓         |             |             |             |           |
# | `raw_symbol`   | `instrument_id`  | ✓          | ✓         | ✓           | ✓           | ✓           | ✓         |
# | `instrument_id`| `raw_symbol`     | ✓          | ✓         | ✓           | ✓           | ✓           | ✓         |
#
# For more details:
#
# * [https://databento.com/docs/standards-and-conventions/symbology?historical=python&live=python&reference=python](https://databento.com/docs/standards-and-conventions/symbology?historical=python&live=python&reference=python)

# %% [markdown]
# ## Databento file format
#
# Databento uses its own file format for market-data. It is called **Databento Binary Encoding (DBN)**.
# Think of it like more performant + compressed alternative of CSV / JSON files.
#
# You can easily load DBN file and convert it into simple CSV / JSON data.
#
# For more details:
#
# * [https://databento.com/docs/standards-and-conventions/databento-binary-encoding#getting-started-with-dbn?historical=python&live=python&reference=python](https://databento.com/docs/standards-and-conventions/databento-binary-encoding#getting-started-with-dbn?historical=python&live=python&reference=python)

# %% [markdown]
# # Historical API examples

# %% [markdown]
# ## Authenticate & connect to Databento

# %%
import databento as db


# Establish connection and authenticate
API_KEY = "db-8VWGBis54s4ewGVciMRakNxLCJKen"  # put your API key here (existing key is just example, not real)
client = db.Historical(API_KEY)

# %% [markdown]
# ## Metadata
#
# ### List Publishers
#
# Shows all data publishers.

# %%
publishers = client.metadata.list_publishers()

# Show only first five from long list
publishers[:5]

# %% [markdown]
# Example output:
#
# ```python
# [{'publisher_id': 1,
#   'dataset': 'GLBX.MDP3',
#   'venue': 'GLBX',
#   'description': 'CME Globex MDP 3.0'},
#  {'publisher_id': 2,
#   'dataset': 'XNAS.ITCH',
#   'venue': 'XNAS',
#   'description': 'Nasdaq TotalView-ITCH'},
#  {'publisher_id': 3,
#   'dataset': 'XBOS.ITCH',
#   'venue': 'XBOS',
#   'description': 'Nasdaq BX TotalView-ITCH'},
#  {'publisher_id': 4,
#   'dataset': 'XPSX.ITCH',
#   'venue': 'XPSX',
#   'description': 'Nasdaq PSX TotalView-ITCH'},
#  {'publisher_id': 5,
#   'dataset': 'BATS.PITCH',
#   'venue': 'BATS',
#   'description': 'Cboe BZX Depth Pitch'}]
# ```

# %% [markdown]
# ### List Datasets
#
# Each dataset is in format: `PUBLISHER.DATASET`
#
# * Publisher / Market code is based on: [https://www.iso20022.org/market-identifier-codes](https://www.iso20022.org/market-identifier-codes)

# %%
datasets = client.metadata.list_datasets()
datasets

# %% [markdown]
# Example output:
#
# ```python
# ['ARCX.PILLAR',
#  'DBEQ.BASIC',
#  'EPRL.DOM',
#  'EQUS.SUMMARY',
#  'GLBX.MDP3',
#  'IEXG.TOPS',
#  'IFEU.IMPACT',
#  'NDEX.IMPACT',
#  'OPRA.PILLAR',
#  'XASE.PILLAR',
#  'XBOS.ITCH',
#  'XCHI.PILLAR',
#  'XCIS.TRADESBBO',
#  'XNAS.BASIC',
#  'XNAS.ITCH',
#  'XNYS.PILLAR',
#  'XPSX.ITCH']
# ```

# %% [markdown]
# ### List Schemas
#
# List all supported data formats in Databento.

# %%
schemas = client.metadata.list_schemas(dataset="GLBX.MDP3")
schemas

# %% [markdown]
# Example output:
#
# ```python
# ['mbo',
#  'mbp-1',
#  'mbp-10',
#  'tbbo',
#  'trades',
#  'bbo-1s',
#  'bbo-1m',
#  'ohlcv-1s',
#  'ohlcv-1m',
#  'ohlcv-1h',
#  'ohlcv-1d',
#  'definition',
#  'statistics',
#  'status']
# ```

# %% [markdown]
# ### Dataset condition
#
# Show data availability and quality.

# %%
conditions = client.metadata.get_dataset_condition(
    dataset="GLBX.MDP3",
    start_date="2022-06-06",
    end_date="2022-06-10",
)

conditions

# %% [markdown]
# Example output:
#
# ```python
# [{'date': '2022-06-06',
#   'condition': 'available',
#   'last_modified_date': '2024-05-18'},
#  {'date': '2022-06-07',
#   'condition': 'available',
#   'last_modified_date': '2024-05-21'},
#  {'date': '2022-06-08',
#   'condition': 'available',
#   'last_modified_date': '2024-05-21'},
#  {'date': '2022-06-09',
#   'condition': 'available',
#   'last_modified_date': '2024-05-21'},
#  {'date': '2022-06-10',
#   'condition': 'available',
#   'last_modified_date': '2024-05-22'}]
# ```

# %% [markdown]
# ### Dataset range
#
# Show available range for dataset.
#
# * Use this method to discover data availability.
# * The start and end values in the response can be used with the `timeseries.get_range` and `batch.submit_job` endpoints.

# %%
available_range = client.metadata.get_dataset_range(dataset="GLBX.MDP3")
available_range

# %% [markdown]
# Example output:
#
# ```python
# {'start': '2010-06-06T00:00:00.000000000Z',
#  'end': '2025-01-18T00:00:00.000000000Z'}
# ```

# %% [markdown]
# ### Record count
#
# Returns count of records return from data query.

# %%
record_count = client.metadata.get_record_count(
    dataset="GLBX.MDP3",
    symbols=["ESM2"],  # ES (S&P contract) expiring in June 2022
    schema="ohlcv-1h",  # 1 hour bars ; only time-ranges that are multiplies of 10-minutes (cannot be used for 1-min bars)
    start="2022-01-06",  # including start
    end="2022-01-07",  # excluding end
)

# There is one hour break on the exchange, so 23 hourly bars are OK
record_count

# %% [markdown]
# Example output:
#
# `23`

# %% [markdown]
# ### Costs
#
# Get costs = how much you pay for the data in US dollars.

# %%
cost = client.metadata.get_cost(
    dataset="GLBX.MDP3",
    symbols=["ESM2"],
    schema="ohlcv-1h",  # 1 hour bars ; only time-ranges that are multiplies of 10-minutes (cannot be used for 1-min bars)
    start="2022-01-06",  # including start
    end="2022-01-07",  # excluding end
)

cost

# %% [markdown]
# Example output:
#
# `0.00022791326`

# %% [markdown]
# ## Time series data
#
# ### `get_range`
#
# * Makes a streaming request for time series data from Databento.
# * This is the primary method for getting historical market data, instrument definitions, and status data directly into your application.
# * This method only returns after all of the data has been downloaded, which can take a long time.
#
# **Warning:**
# * `ts_event` represents start-time of aggregation. So if we download bars, the timestamp represents **opening time** for each bar.

# %%
data = client.timeseries.get_range(
    dataset="GLBX.MDP3",
    symbols=["ESM2"],  # ES (S&P contract) expiring in June 2022
    schema="ohlcv-1h",  # Hourly bars
    start="2022-06-01T00:00:00",
    end="2022-06-03T00:10:00",
    limit=5,  # Optional limit on count of results
)

# Data are received in DBNStore format
data

# %% [markdown] jp-MarkdownHeadingCollapsed=true
# Example output:
#
# `<DBNStore(schema=ohlcv-1h)>`

# %%
# Convert DBN format to pandas-dataframe
df = data.to_df()

# Preview
print(len(df))
df

# %% [markdown]
# Example output: *(not real data, just example of output format)*
#
# | ts_event | rtype | publisher_id | instrument_id | open | high | low | close | volume | symbol |
# |:--|:--|:--|:--|:--|:--|:--|:--|:--|:--|
# | 2022-06-01 00:00:00+00:00 | 34 | 1 | 3403 | 4149.25 | 4153.50 | 4149.00 | 4150.75 | 9281 | ESM2 |
# | 2022-06-01 01:00:00+00:00 | 34 | 1 | 3403 | 4151.00 | 4157.75 | 4149.50 | 4154.25 | 11334 | ESM2 |
# | 2022-06-01 02:00:00+00:00 | 34 | 1 | 3403 | 4154.25 | 4155.25 | 4146.50 | 4147.00 | 7258 | ESM2 |

# %% [markdown]
# Note:
#
# * `rtype` = 1-hour bars
# * More codes like this: [https://databento.com/docs/standards-and-conventions/common-fields-enums-types#rtype?historical=python&live=python&reference=python](https://databento.com/docs/standards-and-conventions/common-fields-enums-types#rtype?historical=python&live=python&reference=python)

# %% [markdown]
# ## Symbols
#
# ### `resolve`
#
# Resolve a list of symbols from an **input** symbology type, to an **output** symbology type.
#
# * Example: `raw_symbol` to an `instrument_id`: `ESM2` → `3403`

# %%
result = client.symbology.resolve(
    dataset="GLBX.MDP3",
    symbols=["ESM2"],
    stype_in="raw_symbol",
    stype_out="instrument_id",
    start_date="2022-06-01",
    end_date="2022-06-30",
)

result

# %% [markdown]
# Example output:
#
# ```python
# {'result': {'ESM2': [{'d0': '2022-06-01', 'd1': '2022-06-26', 's': '3403'}]},
#  'symbols': ['ESM2'],
#  'stype_in': 'raw_symbol',
#  'stype_out': 'instrument_id',
#  'start_date': '2022-06-01',
#  'end_date': '2022-06-30',
#  'partial': [],
#  'not_found': [],
#  'message': 'OK',
#  'status': 0}
# ```

# %% [markdown]
# Most important is the `result` and key-value pair `'s': '3403'`, which contains value of instrument_id.

# %% [markdown]
# ## DBNStore operations
#
# The `DBNStore` object is an helper class for working with `DBN` encoded data.

# %% [markdown]
# ### `from_bytes`
#
# Read data from a DBN byte stream.

# %%
dbn_data = client.timeseries.get_range(
    dataset="GLBX.MDP3",
    symbols=["ESM2"],
    schema="ohlcv-1h",
    start="2022-06-06",
    limit=3,
)

dbn_data.to_df()

# %% [markdown]
# Example output: *(not real data, just example of output format)*
#
# | ts_event | rtype | publisher_id | instrument_id | open | high | low | close | volume | symbol |
# |:--|:--|:--|:--|:--|:--|:--|:--|:--|:--|
# | 2022-06-06 00:00:00+00:00 | 34 | 1 | 3403 | 4109.50 | 4117.00 | 4105.50 | 4115.75 | 8541 | ESM2 |
# | 2022-06-06 01:00:00+00:00 | 34 | 1 | 3403 | 4115.75 | 4122.75 | 4113.00 | 4122.25 | 14008 | ESM2 |
# | 2022-06-06 02:00:00+00:00 | 34 | 1 | 3403 | 4122.25 | 4127.00 | 4120.75 | 4126.25 | 10150 | ESM2 |

# %%
# Save streamed data to file - recommended suffix is: `*.dbn.zst`
path = "./GLBX-ESM2-20220606.ohlcv-1h.dbn.zst"
dbn_data.to_file(path)

# %%
# Load data from previously saved file and create DBN object again
with open(path, "rb") as saved:
    loaded_dbn_data = db.DBNStore.from_bytes(saved)

loaded_dbn_data.to_df()

# %% [markdown]
# Example output *(not real data, just example of output format)*:
#
# | ts_event | rtype | publisher_id | instrument_id | open | high | low | close | volume | symbol |
# |:--|:--|:--|:--|:--|:--|:--|:--|:--|:--|
# | 2022-06-06 00:00:00+00:00 | 34 | 1 | 3403 | 4109.50 | 4117.00 | 4105.50 | 4115.75 | 8541 | ESM2 |
# | 2022-06-06 01:00:00+00:00 | 34 | 1 | 3403 | 4115.75 | 4122.75 | 4113.00 | 4122.25 | 14008 | ESM2 |
# | 2022-06-06 02:00:00+00:00 | 34 | 1 | 3403 | 4122.25 | 4127.00 | 4120.75 | 4126.25 | 10150 | ESM2 |

# %% [markdown]
# ### `from_file`
#
# Reads data from a DBN file.

# %%
loaded_dbn_data = db.DBNStore.from_file(path)
loaded_dbn_data.to_df()

# %% [markdown]
# Example output: *(not real data, just example of output format)*
#
# | ts_event | rtype | publisher_id | instrument_id | open | high | low | close | volume | symbol |
# |:--|:--|:--|:--|:--|:--|:--|:--|:--|:--|
# | 2022-06-06 00:00:00+00:00 | 34 | 1 | 3403 | 4109.50 | 4117.00 | 4105.50 | 4115.75 | 8541 | ESM2 |
# | 2022-06-06 01:00:00+00:00 | 34 | 1 | 3403 | 4115.75 | 4122.75 | 4113.00 | 4122.25 | 14008 | ESM2 |
# | 2022-06-06 02:00:00+00:00 | 34 | 1 | 3403 | 4122.25 | 4127.00 | 4120.75 | 4126.25 | 10150 | ESM2 |

# %% [markdown]
# ### `to_csv`
#
# Write data to a file in CSV format.

# %%
dbn_data = client.timeseries.get_range(
    dataset="GLBX.MDP3",
    symbols=["ESM2"],
    schema="ohlcv-1h",
    start="2022-06-06",
    limit=3,
)

# Export to CSV file
dbn_data.to_csv("GLBX-ESM2-20220606-ohlcv-1h.csv")

# %% [markdown]
# ### `to_df`
#
# Converts DBN data to a pandas DataFrame.

# %%
# Export to pandas DataFrame
dbn_data.to_df()

# %% [markdown]
# Example output: *(not real data, just example of output format)*
#
# | ts_event | rtype | publisher_id | instrument_id | open | high | low | close | volume | symbol |
# |:--|:--|:--|:--|:--|:--|:--|:--|:--|:--|
# | 2022-06-06 00:00:00+00:00 | 34 | 1 | 3403 | 4109.50 | 4117.00 | 4105.50 | 4115.75 | 8541 | ESM2 |
# | 2022-06-06 01:00:00+00:00 | 34 | 1 | 3403 | 4115.75 | 4122.75 | 4113.00 | 4122.25 | 14008 | ESM2 |
# | 2022-06-06 02:00:00+00:00 | 34 | 1 | 3403 | 4122.25 | 4127.00 | 4120.75 | 4126.25 | 10150 | ESM2 |

# %% [markdown]
# ### `to_json`
#
# Write data to a file in JSON format.

# %%
# Export to pandas DataFrame
dbn_data.to_json("GLBX-ESM2-20220606-ohlcv-1h.json")

# %% [markdown]
# ### `to_file`
#
# Write data to a DBN file.

# %%
# Export to DBN file
dbn_data.to_file("GLBX-ESM2-20220606.ohlcv-1h.dbn.zst")

# %% [markdown]
# ### `to_ndarray`
#
# * Converts data to a numpy N-dimensional array.
# * Each element will contain a Python representation of the binary fields as a `Tuple`.

# %%
# Export to numpy-array
ndarray = dbn_data.to_ndarray()
ndarray

# %% [markdown]
# ### `to_parquet`
#
# * Write data to a file in [Apache parquet](https://parquet.apache.org/) format.

# %%
# Export to Apache Parquet file
dbn_data.to_parquet("GLBX-ESM2-20220606-ohlcv-1h.parquet")

# %% [markdown]
# ### `for` cycle
#
# * You can use standard python `for` cycle to iterate over DBN file content.

# %%
# Let's load some data first
dbn_data = client.timeseries.get_range(
    dataset="GLBX.MDP3",
    symbols=["ESM2"],
    schema="ohlcv-1h",
    start="2022-06-06",
    limit=3,
)

# Contains 3 hourly bars
dbn_data.to_df()

# %% [markdown]
# Example output: *(not real data, just example of output format)*
#
# | ts_event | rtype | publisher_id | instrument_id | open | high | low | close | volume | symbol |
# |:--|:--|:--|:--|:--|:--|:--|:--|:--|:--|
# | 2022-06-06 00:00:00+00:00 | 34 | 1 | 3403 | 4109.50 | 4117.00 | 4105.50 | 4115.75 | 8541 | ESM2 |
# | 2022-06-06 01:00:00+00:00 | 34 | 1 | 3403 | 4115.75 | 4122.75 | 4113.00 | 4122.25 | 14008 | ESM2 |
# | 2022-06-06 02:00:00+00:00 | 34 | 1 | 3403 | 4122.25 | 4127.00 | 4120.75 | 4126.25 | 10150 | ESM2 |

# %%
# We can use DBN data in for-cycle:
for bar in dbn_data:
    print(bar)  # print full bar data
    break  # intentionally break to see only 1st bar

# %% [markdown]
# Example output:
#
# ```
# OhlcvMsg {
#     hd: RecordHeader {
#         length: 14,
#         rtype: Ohlcv1H,
#         publisher_id: GlbxMdp3Glbx,
#         instrument_id: 3403,
#         ts_event: 1654473600000000000
#     },
#     open: 4109.500000000,
#     high: 4117.000000000,
#     low: 4105.500000000,
#     close: 4115.750000000,
#     volume: 4543
# }
# ```

# %%
for bar in dbn_data:
    print(f"Bar open: {bar.open}")  # print only bar-open information
    break  # intentionally break to see only 1st bar

# %% [markdown]
# Example output:
#
# `Bar open: 4108500000000`

# %% [markdown]
# # Examples
#
# ## Download 1-min 6E data

# %%
from datetime import timedelta

import pandas as pd
import pytz


pd.set_option("display.max_columns", None)
pd.set_option("display.max_rows", None)

# %%
# Settings
dataset = "GLBX.MDP3"
symbol = "6E.v.0"
stype_in = "continuous"
schema = "ohlcv-1m"
start = "2025-01-01"
end = "2025-01-05"

# %%
# Check costs in dollars
cost = client.metadata.get_cost(
    dataset=dataset,
    symbols=[symbol],
    stype_in=stype_in,
    schema=schema,
    start=start,
    end=end,
)

print(f"{cost:.2f}$")

# %% [markdown]
# Example output:
#
# `0.01$`

# %%
# Download data
data = client.timeseries.get_range(
    dataset=dataset,
    symbols=[symbol],
    stype_in=stype_in,
    schema=schema,
    start=start,
    end=end,
)

# Export data in DBNStore format (CSV data are 10x bigger)
data.to_file(f"{dataset}_{symbol}_{start}-{end}.{schema}.dbn.zst")

# %%
# Cleanup and view data as DataFrame
df = (
    data.to_df()
    .reset_index()
    .rename(columns={"ts_event": "datetime"})
    .drop(columns=["rtype", "publisher_id", "instrument_id"])
    # Nice order of columns
    .reindex(columns=["symbol", "datetime", "open", "high", "low", "close", "volume"])
    # Localize datetime to Bratislava
    .assign(datetime=lambda df: pd.to_datetime(df["datetime"], utc=True))  # Mark as UTC datetime
    .assign(
        datetime=lambda df: df["datetime"].dt.tz_convert(pytz.timezone("Europe/Bratislava")),
    )  # Convert to Bratislava timezone
    # Add 1-minute, so datetime represents closing time of the bar (not opening time)
    .assign(datetime=lambda df: df["datetime"] + timedelta(minutes=1))
)

# Preview
print(len(df))
df.head(3)

# %% [markdown]
# Example output: *(not real data, just example of output format)*
#
# `2734`
#
# | symbol | datetime | open | high | low | close | volume |
# |:--|:--|:--|:--|:--|:--|:--|
# | 6E.v.0 | 2025-01-02 00:01:00+01:00 | 1.03890 | 1.03930 | 1.03845 | 1.03905 | 291 |
# | 6E.v.0 | 2025-01-02 00:02:00+01:00 | 1.03900 | 1.03900 | 1.03870 | 1.03880 | 311 |
# | 6E.v.0 | 2025-01-02 00:03:00+01:00 | 1.03880 | 1.03890 | 1.03870 | 1.03885 | 140 |
#
