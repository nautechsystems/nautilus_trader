# Configuration file for the Sphinx documentation builder.
#
# This file only contains a selection of the most common options. For a full
# list see the documentation:
# https://www.sphinx-doc.org/en/master/usage/configuration.html

# -- Path setup --------------------------------------------------------------

# If extensions (or modules to document with autodoc) are in another directory,
# add these directories to sys.path here. If the directory is relative to the
# documentation root, use os.path.abspath to make it absolute, like shown here.
#

import nautilus_trader


# -- Project information -----------------------------------------------------
project = "NautilusTrader"
author = "Nautech Systems Pty Ltd."
copyright = "2015-2024 Nautech Systems Pty Ltd"
version = nautilus_trader.__version__

# -- General configuration ---------------------------------------------------
extensions = [
    "myst_parser",
    "sphinx.ext.autodoc",
    "sphinx.ext.intersphinx",
    "sphinx.ext.napoleon",
    "sphinx_markdown_builder",
    "sphinx_comments",
]

comments_config = {"hypothesis": False, "utterances": False}
exclude_patterns = ["**.ipynb_checkpoints", ".DS_Store", "Thumbs.db", "_build"]
source_suffix = [".rst", ".md"]

myst_enable_extensions = [
    "colon_fence",
    "dollarmath",
    "linkify",
    "substitution",
    "tasklist",
]
myst_url_schemes = ("mailto", "http", "https")
suppress_warnings = ["myst.domains"]

add_module_names = False
todo_include_todos = False

autosummary_generate = True
autodoc_member_order = "bysource"
