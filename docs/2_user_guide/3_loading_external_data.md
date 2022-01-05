---
jupyter:
  jupytext:
    formats: ipynb,md
    text_representation:
      extension: .md
      format_name: markdown
      format_version: '1.3'
      jupytext_version: 1.13.5
  kernelspec:
    display_name: Python (nautilus_trader)
    language: python
    name: nautilus_trader
---

# Loading External Data

This notebook runs through a example loading raw data (external to nautilus) into the Nautilus Trader `DataCatalog`, for use in backtesting.

## The Data Catalog

The data catalog is a central store for Nautilus data, persisted in the [Parquet](https://parquet.apache.org) file format.

We have chosen parquet as the storage format for the following reasons:
- It performs much better than CSV/JSON/HDF5/etc in terms of compression (storage size) and read performance.
- It does not require any separate running components (for example a database).
- It is quick and simple for someone to get up and running with.

<!-- #region tags=[] -->
## Getting some sample raw data

Before we start the notebook - as a once off we need to download some sample data for loading

For this notebook we will use Forex data from `histdata.com`, simply go to https://www.histdata.com/download-free-forex-historical-data/?/ascii/tick-data-quotes/ and select a Forex pair and one or more months of data to download.

Once you have downloaded the data, set the variable `input_files` below to the path containing the data. You can also use a glob to select multiple files, for example `"~/Downloads/HISTDATA_COM_ASCII_AUDUSD_*.zip"`
<!-- #endregion -->

```python
import fsspec
fs = fsspec.filesystem('file')

input_files = "~/Downloads/HISTDATA_COM_ASCII_AUDUSD_T202001.zip"
```

Run the cell below; you should see the files that you downloaded

```python
# Simple check that the file path is correct
assert len(fs.glob(input_files)), f"Could not find files with {input_files=}"
```

### Loading data via `Reader` classes

We can load data from various sources into the data catalog using helper methods in the `nautilus_trader.persistence.external.readers` module. The module contains methods for reading various data formats (csv, json, txt), minimising the amount of code required to get data loaded correctly into the data catalog.

There are a handful of readers available, some notes on when to use which:
- `CSVReader` - use when your data is CSV (comma separated values) and has a header row. Each row of the data typically is one "entry" and is linked to the header.
- `TextReader` - similar to CSVReader, but used when data may container multiple "entries" per line, for example JSON data with multiple orderbook or trade ticks in a single line. Typically does not have a header row and field names come from some definition elsewhere. 
- `ParquetReader` - for parquet files, will read chunks of the data and process similar to `CSVReader`

Each of the `Reader` classes takes a `line_parser` or `block_parser` function, a user defined function to convert a line or block (chunk / multiple rows) of data into nautilus object(s) (for example `QuoteTick` or `TradeTick`).


### Writing the `parser` function

The Forex data from `histdata` is stored in csv/text format, with fields `timestamp, bid_price, ask_price`. 

For this example, we will use the `CSVReader` class, but we need to manually pass a header as the files do not contain one. The `CSVReader` has a couple of options, we'll be setting and `chunked=False` to process the data line-by-line and `as_dataframe=False` to process the data as a string rather than DataFrame. See the [API Reference]("../3_api_reference/persistence") for more details.

```python
import datetime
import pandas as pd
from nautilus_trader.persistence.external.readers import CSVReader
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.core.datetime import dt_to_unix_nanos

def parser(data, instrument_id):
    """ Parser function for hist_data FX data, for use with CSV Reader """
    dt = pd.Timestamp(datetime.datetime.strptime(data['timestamp'].decode(), "%Y%m%d %H%M%S%f"), tz='UTC')
    yield QuoteTick(
        instrument_id=instrument_id,
        bid=Price.from_str(data['bid'].decode()),
        ask=Price.from_str(data['ask'].decode()),
        bid_size=Quantity.from_int(100_000),
        ask_size=Quantity.from_int(100_000),
        ts_event=dt_to_unix_nanos(dt),
        ts_init=dt_to_unix_nanos(dt),
    )
```

### Creating a Data Catalog if one does not exist

Now that we have our parser function, we instantiate a `DataCatalog` (passing in a directory where to store the data, by default we will just use the current directory)

```python
import os, shutil
CATALOG_PATH = os.getcwd() + "/catalog"

# Clear if it already exists, then create fresh
if os.path.exists(CATALOG_PATH):
    shutil.rmtree(CATALOG_PATH)
os.mkdir(CATALOG_PATH)
```

```python
# Create an instance of the DataCatalog
from nautilus_trader.persistence.catalog import DataCatalog
catalog = DataCatalog(CATALOG_PATH)
```

### Instruments

Nautilus needs to link market data to an `instrument_id`, and an `instrument_id` to an `Instrument` definition. This can be done at any time, but typically it makes sense to do it when you are loading market data into the catalog.

For our example, Nautilus contains some helpers for creating Forex pairs, which we will use. If, however, you were adding data for financial or crypto markets, you could need to create (and add to the catalog) an instrument corresponding to that instrument_id. Definitions for various other instruments can be found in `nautilus_trader.model.instruments`. 

See [Instruments](./4_instruments.md) for more details on creating other instruments



```python
from nautilus_trader.persistence.external.core import process_files, write_objects
from nautilus_trader.backtest.data.providers import TestInstrumentProvider

# Use nautilus test helpers to create a EUR/USD Forex instrument for our purposes
instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
```

We can now add our new instrument to the `DataCatalog`

```python
from nautilus_trader.persistence.external.core import write_objects

write_objects(catalog, [instrument])
```

And check its existence:

```python
catalog.instruments()
```

<!-- #region -->
### Loading the files 

One final note, our parsing function takes an `instrument_id` argument, as in our case with hist_data, the actual file does not contain information about the instrument, only the file name does. In our instance, we would likely need to split our loading per Forex pair, so we can determine which instrument we are loading. We will use a simple lambda function to pass our `instrument_id` to the parsing function.

We can now use the `process_files` function to load one or more files using our `Reader` class and `parsing` function as shown below. This function will loop over many files, as well as breaking up large files into chunks (protecting us from out of memory errors when reading large files) and save the results to the `DataCatalog`


For the hist_data, it should take less than a minute or two to load each Forex file (a progress bar will appear below).
<!-- #endregion -->

```python
from nautilus_trader.persistence.external.core import process_files

process_files(
    glob_path=input_files,
    reader=CSVReader(
        block_parser=lambda x: parser(x, instrument_id=instrument.id), 
        header=['timestamp', 'bid', 'ask', 'volume'],
        chunked=False, 
        as_dataframe=False,
    ),
    catalog=catalog,
)
```

## Using the Data Catalog 

Once data has been loaded into the catalog, the `catalog` instance can be used for loading data into the backtest engine, or simple for research purposes. It contains various methods to pull data from the catalog, like `quote_ticks` (show below)

```python
start = dt_to_unix_nanos(pd.Timestamp('2020-01-01', tz='UTC'))
end =  dt_to_unix_nanos(pd.Timestamp('2020-01-02', tz='UTC'))

catalog.quote_ticks(start=start, end=end)
```

Finally, clean up the catalog

```python
if os.path.exists(CATALOG_PATH):
    shutil.rmtree(CATALOG_PATH)
```
