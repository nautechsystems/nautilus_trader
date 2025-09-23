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
# # Parquet Explorer
#
# This tutorial explores some basic query operations on Parquet files written by Nautilus. We'll utilize both the `datafusio`n and `pyarrow` libraries.
#
# Before proceeding, ensure that you have `datafusion` installed. If not, you can install it by running:
# ```bash
# pip install datafusion
# ```

# %% editable=true slideshow={"slide_type": ""}
import datafusion
import pyarrow.parquet as pq


# %%
trade_tick_path = "../../tests/test_data/nautilus/trades.parquet"
bar_path = "../../tests/test_data/nautilus/bars.parquet"

# %%
# Create a context
ctx = datafusion.SessionContext()

# %%
# Run this cell once (otherwise will error)
ctx.register_parquet("trade_0", trade_tick_path)
ctx.register_parquet("bar_0", bar_path)

# %% [markdown]
# ### TradeTick data

# %%
query = "SELECT * FROM trade_0 ORDER BY ts_init"
df = ctx.sql(query)

# %%
df.schema()

# %%
df

# %%
table = pq.read_table(trade_tick_path)

# %%
table.schema

# %% [markdown]
# ### Bar data

# %%
query = "SELECT * FROM bar_0 ORDER BY ts_init"
df = ctx.sql(query)

# %%
df.schema()

# %%
df

# %%
table = pq.read_table(bar_path)
table.schema

# %%
