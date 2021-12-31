# Backtesting

***This guide is currently outdated and will be updated soon 1/1/2022***

## Environment setup
Backtesting can be setup and run through JupyterNotebook. As an initial step you can ensure you have the `ipykernel` setup correctly where NautilusTrader is installed.

    poetry run pip install ipykernel
    poetry run python -m ipykernel install --user --name nautilus_trader --display-name "Python (nautilus_trader)"

Launch Jupyter from a terminal and change to the `Python (nautilus_trader)` kernel, and restart.

Then running the following in the first notebook cell should output the path to the kernel which we just setup:

    import sys; sys.executable

## Backtest Data
***This section of NautilusTrader is under active development and breaking changes are very likely***

NautilusTrader currently supports loading data into a `BacktestEngine` manually, or using the new (alpha) `DataLoader` & `DataCatalog` classes.

### Loading data into NautilusTrader with `DataLoader`
NautilusTrader has added support for loading raw data into parquet files for easy of use with backtesting. It supports reading several common data formats (text/json, csv & parquet), and a variety of compressions and localities (local files, s3, gcs) via the excellent [fsspec](https://filesystem-spec.readthedocs.io/en/latest/index.html) library. There is some configuration required to get started, but this allows a lot more flexibility for users loading data.

#### Compression
The fsspec library handles compression automatically for a range of compressions algorithms, as long as files have the correct suffix for their respective algorithm (gzipped file ends with `.gz`).
For a full list of currently supported compression algorithms, see `fsspec.compression.compr`, and new types can be registered by simply adding them to the `compr` dict.

#### Instrument Providers
In many cases, loading historical data will require some form of `InstrumentProvider` which can be passed to parsing functions, so that raw messages can be converted to their correct instruments.
The `DataLoader` can be passed an `instrument_provider`, and the parsing classes have an optional `instrument_provider_update` kwarg which, if set, will be passed the raw chunk of data being loaded (before it is passed to the actual parsing function) - this gives the user a chance to add any logic required to load instruments before raw data parsing is attempted. See the final section for a motivating example using Betfair.

#### Parsers
The first configuration option is the format the data is stored in - currently, there are 3 formats available:
- `TextParser` for generic text data (JSON or other).
- `CSVParser` for CSV data.
- `ParquetParser` for parquet data.

Each parser has separate configuration based on its requirements (i.e. the `TextParser` will require an additional `line_parser` to convert the test line/JSON into actual Nautilus objects).

A simple example might be loading quote ticks from a CSV file:

```python
from nautilus_trader.backtest.data_loader import CSVParser

parser = CSVParser()
```

A more complex example would be loading *Betfair* data which is stored in JSON format and requires a `line_parser`:

```python
import orjson
from nautilus_trader.backtest.data_loader import TextParser
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.parsing import on_market_update

instrument_provider = BetfairInstrumentProvider.from_instruments([])
parser = TextParser(
    line_parser=lambda x: on_market_update(instrument_provider=instrument_provider, update=orjson.loads(x)),
)
```

### Discovering data with the `DataLoader`
Next, we instantiate a `DataLoader` with configuration about where the raw data is stored (See code docstring for full details) and the `Parser` created in the previous step.
For example, loading CSV file(s) from a local directory:

For example, loading CSV file(s) from a local directory:

```python
from nautilus_trader.backtest.data_loader import DataLoader
from nautilus_trader.backtest.data_loader import CSVParser

loader = DataLoader(
    path="/Users/MyName/Downloads/fx-data/",
    parser=CSVParser(),
    glob_pattern="*",
)
```

Loading from a remote location (with any compression) is just as simple with `fsspec`:

```python
from nautilus_trader.backtest.data_loader import DataLoader
from nautilus_trader.backtest.data_loader import CSVParser

loader = DataLoader(
    path="/mybucket/data",
    fs_protocol="s3",
    parser=CSVParser(),
    glob_pattern="*.gz",
)
```

The `DataLoader` has a `.path` attribute that will contain the list of files it matched with the `path/glob` configuration to ensure the correct files are ready to be loaded.

### Loading data into the `DataCatalog`
The final step is to load the data from the `DataLoader` into the `DataCatalog`. The `DataCatalog` (currently) needs an environment variable `NAUTILUS_BACKTEST_DIR` to be set - this is the root directory in which the data will be saved.
Then, loading the data can be simply done by instantiating a `DataCatalog` and calling `import_from_data_loader`:

```python
import os
from nautilus_trader.backtest.data_loader import DataCatalog

# Set environment variable
os.environ.update({"NAUTILUS_BACKTEST_DIR": "/Users/MyUser/data/nautilus/"})

# Using the loader from above
c = DataCatalog()
c.import_from_data_loader(loader, progress=True) # `progress`: show progress bar for files
```

### Accessing stored data via `DataCatalog`
The `DataCatalog` has methods for querying different data types from the cache, as well as a `load_backtest_data` to load data for a backtest. See the docstring for full details.

### A full example - Loading a historic *Betfair* file
Download one of the sample files from nautilus test fixtures locally (in a terminal or with `!` in jupyter notebook).

    wget https://github.com/nautechsystems/nautilus_trader/raw/master/tests/test_kit/data/betfair/1.180305278.bz2

The full example below shows loading data from a raw file, and building a dataset to pass to a backtest - run in a notebook or other IDE.

```python
import os
import orjson
from nautilus_trader.adapters.betfair.providers import BetfairInstrumentProvider
from nautilus_trader.adapters.betfair.parsing import on_market_update
from nautilus_trader.adapters.betfair.util import historical_instrument_provider_loader
from nautilus_trader.backtest.data.loaders import TextParser, DataLoader
from nautilus_trader.persistence.catalog import DataCatalog

os.environ.update({"NAUTILUS_BACKTEST_DIR": "/Users/MyUser/data/nautilus/"})


# We create an empty BetfairInstrumentProvider that we will load instruments into as we read the files
instrument_provider = BetfairInstrumentProvider.from_instruments([])

parser = TextParser(
    # use the standard `on_market_update` betfair parser that the adapter uses
    line_parser=lambda x: on_market_update(
        instrument_provider=instrument_provider, update=orjson.loads(x)
    ),
    # We also use a utility function `historical_instrument_provider_loader` to read the market definition and parse
    # the instruments, which gets passed to our instrument_provider (which adds the instruments)    
    instrument_provider_update=historical_instrument_provider_loader,
)

# We simply use the current directory as our path, and glob for the file we just downloaded. 
loader = DataLoader(
    path=os.getcwd(),
    parser=parser,
    glob_pattern="1.180305278.bz2",
    instrument_provider=instrument_provider,
)

c = DataCatalog()
c.import_from_data_loader(loader, progress=True) 

# Data now stored in parquet files ready for fast loading.
# Query instruments, individual datasets or use `load_backtest_data` to load and format for backtest engine
instruments = c.instruments()
trade_ticks = c.trade_ticks()
data = c.load_backtest_data(instrument_ids=[instruments[0].id])
```
