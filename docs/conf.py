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
from typing import Any, List

sys.path.insert(0, os.path.abspath('..'))


# -- Project information -----------------------------------------------------
project = 'NautilusTrader'
author = 'Nautech Systems Pty Ltd.'
copyright = "2015-2022 Nautech Systems Pty Ltd"
version = "latest"
release = 'version'


# -- General configuration ---------------------------------------------------
extensions = [
    'myst_parser',
    'sphinx.ext.autodoc',
    'sphinx.ext.intersphinx',
    'sphinx.ext.napoleon',
    'sphinx_togglebutton',
    'sphinx_copybutton',
    #'sphinx_panels',
    'sphinx_comments',
]

# Add any paths that contain templates here, relative to this directory.
html_static_path = ['_static']
html_css_files = ["custom.css"]
templates_path = ['_templates']


comments_config = {"hypothesis": False, "utterances": False}
exclude_patterns = ["**.ipynb_checkpoints", ".DS_Store", "Thumbs.db", "_build"]

# -- Options for HTML output -------------------------------------------------

html_theme = 'sphinx_material'
html_logo = "artwork/nt-white.png"
html_title = "Documentation"

# material theme options (see theme.conf for more information)
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
    "color_accent": "#00bdd6",
    "theme_color": "#2C2E43", 
    "touch_icon": "images/apple-icon-152x152.png",
    "master_doc": False,
    "globaltoc_collapse": True,
    "globaltoc_depth": 4,
    "nav_links": [
        {
            "href": "/1_getting_started/0_index.html",
            "internal": True,
            "title": "Getting Started",
        },
        {
            "href": "/2_api_reference/0_index.html",
            "internal": True,
            "title": "User Guide",
        },
        {
            "href": "/3_api_reference/0_index.html",
            "internal": True,
            "title": "API Reference",
        },
        {
            "href": "/4_api_reference/0_index.html",
            "internal": True,
            "title": "Integrations",
        },
        {
            "href": "/5_api_reference/0_index.html",
            "internal": True,
            "title": "Developer Guide",
        },
        {
            "href": "Developer Guide",
            "internal": True,
            "title": "Releases",
        },
        {
            "href": "https://nautilustrader.io/",
            "internal": False,
            "title": "nautilustrader.io",
        },
    ],
    "heroes": {
        "index": "Documentation",
    },
    "version_dropdown": False,
    "version_json": "_static/versions.json",
    "version_info": {
        "Release": "https://bashtage.github.io/sphinx-material/",
        "Development": "https://bashtage.github.io/sphinx-material/devel/",
        "Release (rel)": "/sphinx-material/",
        "Development (rel)": "/sphinx-material/devel/",
    },
    "table_classes": ["plain"],
}

html_sidebars = {
    "**": ["logo-text.html", "globaltoc.html", "localtoc.html", "searchbox.html"]
}

source_suffix = [".rst", ".md"]
html_show_sphinx = False

myst_enable_extensions = [
    "colon_fence",
    "dollarmath",
    "linkify",
    "substitution",
    "tasklist",
]
myst_url_schemes = ["mailto", "http", "https"]
suppress_warnings = ["myst.domains"]

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

html_show_sphinx = False
