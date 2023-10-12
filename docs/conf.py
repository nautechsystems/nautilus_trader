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
import sys

import nautilus_trader


sys.path.insert(0, os.path.abspath(".."))
sys.path.append(os.path.abspath("./_pygments"))

# -- Project information -----------------------------------------------------
project = "NautilusTrader"
author = "Nautech Systems Pty Ltd."
copyright = "2015-2023 Nautech Systems Pty Ltd"
version = nautilus_trader.__version__

# -- General configuration ---------------------------------------------------
extensions = [
    "myst_parser",
    "sphinx.ext.autodoc",
    "sphinx.ext.intersphinx",
    "sphinx.ext.napoleon",
    "sphinx_togglebutton",
    "sphinx_copybutton",
    "sphinx_comments",
]

# Add any paths that contain templates here, relative to this directory.
html_static_path = ["_static"]
html_css_files = ["custom.css"]
templates_path = ["_templates"]
html_js_files = ["script.js"]

comments_config = {"hypothesis": False, "utterances": False}
exclude_patterns = ["**.ipynb_checkpoints", ".DS_Store", "Thumbs.db", "_build"]
source_suffix = [".rst", ".md"]

# -- Options for HTML output -------------------------------------------------
html_theme = "sphinx_material"
html_logo = "_images/nt-white.png"
html_favicon = "_images/favicon-32x32.png"
html_title = ""
html_sidebars = {"**": ["logo-text.html", "globaltoc.html", "localtoc.html", "searchbox.html"]}
html_show_sphinx = False
html_show_sourcelink = False

# sphinx-material theme options (see theme.conf for more information)
html_theme_options = {
    "nav_title": "",
    "base_url": "",
    "repo_type": "github",
    "repo_url": "https://github.com/nautechsystems/nautilus_trader",
    "repo_name": "nautilus_trader",
    "google_analytics_account": "UA-XXXXX",
    "html_minify": False,
    "html_prettify": True,
    "color_primary": "#282f38",
    "color_accent": "#282f38",
    "theme_color": "#282f38",
    "touch_icon": "images/apple-icon-152x152.png",
    "master_doc": False,
    "globaltoc_collapse": False,
    "globaltoc_depth": 3,
    "nav_links": [
        {
            "href": "/getting_started/index",
            "internal": True,
            "title": "Getting Started",
        },
        {
            "href": "/user_guide/index",
            "internal": True,
            "title": "User Guide",
        },
        {
            "href": "/api_reference/index",
            "internal": True,
            "title": "Python API",
        },
        {
            "href": "/core/index",
            "internal": True,
            "title": "Rust API",
        },
        {
            "href": "/integrations/index",
            "internal": True,
            "title": "Integrations",
        },
        {
            "href": "/developer_guide/index",
            "internal": True,
            "title": "Developer Guide",
        },
        {
            "href": "https://github.com/nautechsystems/nautilus_trader/releases",
            "internal": False,
            "title": "Releases ⬀",
        },
        {
            "href": "https://nautilustrader.io/",
            "internal": False,
            "title": "nautilustrader.io ⬀",
        },
    ],
    "version_dropdown": True,
    "version_json": "_static/version.json",
    "table_classes": ["plain"],
}

myst_enable_extensions = [
    "colon_fence",
    "dollarmath",
    "linkify",
    "substitution",
    "tasklist",
]
myst_url_schemes = ("mailto", "http", "https")
suppress_warnings = ["myst.domains"]

# Do not auto-generate summary for class members
numpydoc_show_class_members = False

# Do not prepend module name to functions
add_module_names = False
todo_include_todos = False

autosummary_generate = True
autodoc_member_order = "bysource"

napoleon_google_docstring = False

# Do not show the return type as separate section
napoleon_use_rtype = False

pygments_style = "monokai.MonokaiStyle"
