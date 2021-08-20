# Configuration file for the Sphinx documentation builder.
#
# This file only contains a selection of the most common options. For a full
# list see the documentation:
# https://www.sphinx-doc.org/en/master/usage/configuration.html

# -- Path setup --------------------------------------------------------------

# If extensions (or modules to document with autodoc) are in another directory,
# add these directories to sys.path here. If the directory is relative to the
# documentation root, use os.path.abspath to make it absolute, like shown here.

import os
import sys
from typing import List


sys.path.insert(0, os.path.abspath("../.."))

# -- Project information -----------------------------------------------------

project = "NautilusTrader"
copyright = "2015-2021, Nautech Systems Pty Ltd."
author = "Nautech Systems"
version = "latest"
release = version


# -- General configuration ---------------------------------------------------

# Add any Sphinx extension module names here, as strings. They can be
# extensions coming with Sphinx (named 'sphinx.ext.*') or your custom
# ones.
extensions = [
    "sphinx.ext.autodoc",
    "sphinx.ext.napoleon",
    "numpydoc",
]

# The suffix(es) of source filenames
# You can specify multiple suffix as a list of string:
# source_suffix = [".rst", ".md"]
source_suffix = [".rst", ".md"]

# The master toctree document
master_doc = "index"

# The name of the Pygments (syntax highlighting) style to use
pygments_style = "friendly"

# Don't auto-generate summary for class members
numpydoc_show_class_members = False

# do not prepend module name to functions
add_module_names = False
todo_include_todos = False

autosummary_generate = True
autodoc_member_order = "bysource"

napoleon_google_docstring = False

# Do not show the return type as separate section
napoleon_use_rtype = False

# List of patterns, relative to source directory, that match files and
# directories to ignore when looking for source files.
# This pattern also affects html_static_path and html_extra_path
exclude_patterns: List[str] = []

# -- Options for HTML output -------------------------------------------------

html_theme = "sphinx_rtd_theme"

# Add any paths that contain custom static files (such as style sheets) here,
# relative to this directory. They are copied after the builtin static files,
# so a file named "default.css" will overwrite the builtin "default.css".
html_static_path = ["_static"]
html_style = "css/nautilus.css"
html_logo = "_static/img/nautilus-black.png"


def skip(app, what, name, obj, would_skip, options):  # noqa
    if name == "__init__":
        return False
    return would_skip


def setup(app):  # noqa
    app.connect("autodoc-skip-member", skip)
