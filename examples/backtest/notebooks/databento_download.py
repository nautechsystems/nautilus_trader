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
# # Databento Data Client with Backtest Node
#
# This example demonstrates how to use the Databento data client with a backtest node.

# %% [markdown]
# ## Imports

# %%
# Note: Use the jupytext python extension to be able to open this python file in jupyter as a notebook

# %%
import asyncio

import nautilus_trader.adapters.databento.data_utils as db_data_utils
from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.factories import DatabentoLiveDataClientFactory
from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.core.datetime import time_object_to_dt
from nautilus_trader.model.data import BarType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.persistence.config import DataCatalogConfig


# from nautilus_trader.live.config import RoutingConfig


# %%
# We need to use nest_asyncio in a jupyter notebook to be able to run async code as sync for market data
# requests in a backtest
try:
    asyncio.get_running_loop()
except RuntimeError:
    pass  # No loop running
else:
    import nest_asyncio

    nest_asyncio.apply()

# %% [markdown]
# ## Parameters

# %%
# Set the data path for Databento data
# DATA_PATH = "/path/to/your/data"  # Use your own value here
# db_data_utils.DATA_PATH = DATA_PATH

catalog_folder = "download_catalog"
catalog = db_data_utils.load_catalog(catalog_folder)

# Small amount of data for testing
start_time_1 = "2024-05-07T10:00"
start_time_2 = "2024-05-08T10:00"
end_time_1 = "2024-05-08T10:01"
end_time_2 = "2024-05-08T10:04"
end_time_3 = "2024-05-08T10:06"

# %% [markdown]
# ## Strategy

# %%
# Configure the data catalog
catalog_config = DataCatalogConfig(path=catalog.path)

data_clients: dict = {
    "databento-001": DatabentoDataClientConfig(),
}
# api_key=None,  # 'DATABENTO_API_KEY' env var
# routing=RoutingConfig(
#     default=False,
#     venues=frozenset(["XCME"]),
# ),

# Create the backtest node
node = BacktestNode([])

# Register the Databento data client factory
node.add_data_client_factory("databento", DatabentoLiveDataClientFactory)

# Build download engine
node.setup_download_engine(catalog_config, data_clients)

# %%
node.download_data(
    "request_instrument",
    instrument_id=InstrumentId.from_str("ESM4.XCME"),
    start=time_object_to_dt(start_time_1),
    end=time_object_to_dt(end_time_1),
)

node.download_data(
    "request_bars",
    bar_type=BarType.from_str("ESM4.XCME-1-MINUTE-LAST-EXTERNAL"),
    start=time_object_to_dt(start_time_2),
    end=time_object_to_dt(end_time_3),
)

# %%
node.download_data(
    "request_order_book_depth",
    instrument_id=InstrumentId.from_str("ESM4.XCME"),
    start=time_object_to_dt(start_time_2),
    end=time_object_to_dt(end_time_1),
)

# %%
# # Clean up
node.dispose()
