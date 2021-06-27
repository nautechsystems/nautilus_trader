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

import os

import sphinx_rtd_theme


# -- Project information -----------------------------------------------------

project = "NautilusTrader"
copyright = "2015-2021, Nautech Systems Pty Ltd."
author = "Nautech Systems"

version = ""

if "READTHEDOCS" not in os.environ:
    # if developing locally
    from nautilus_trader import __version__  # noqaE402

    version = __version__

# release version
release = version


# -- General configuration ---------------------------------------------------

# Add any Sphinx extension module names here, as strings. They can be
# extensions coming with Sphinx (named 'sphinx.ext.*') or your custom
# ones.
extensions = [
    "sphinx.ext.autodoc",
    "sphinx.ext.doctest",
    "sphinx.ext.coverage",
    "sphinx.ext.napoleon",
    "sphinx.ext.intersphinx",
    "numpydoc",
]

# The suffix(es) of source filenames.
# You can specify multiple suffix as a list of string:
# source_suffix = [".rst", ".md"]
source_suffix = ".rst"

# The master toctree document.
master_doc = "index"

# The name of the Pygments (syntax highlighting) style to use.
pygments_style = "sphinx"

# do not prepend module name to functions
add_module_names = False
todo_include_todos = False

autosummary_generate = True
autodoc_member_order = "bysource"
numpydoc_show_class_members = True

napoleon_google_docstring = False
napoleon_use_rtype = False

# Add any paths that contain templates here, relative to this directory.
templates_path = ["_templates"]

# List of patterns, relative to source directory, that match files and
# directories to ignore when looking for source files.
# This pattern also affects html_static_path and html_extra_path.
exclude_patterns = []

# -- Options for HTML output -------------------------------------------------

html_theme = "sphinx_rtd_theme"
html_theme_path = [sphinx_rtd_theme.get_html_theme_path()]
html_copy_source = True
html_show_sourcelink = True

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
