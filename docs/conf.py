import os
import sys
from typing import Any, List


sys.path.insert(0, os.path.abspath("../.."))


author = "Nautech Systems Pty Ltd."
comments_config = {"hypothesis": False, "utterances": False}
copyright = "2015-2022"
exclude_patterns = ["**.ipynb_checkpoints", ".DS_Store", "Thumbs.db", "_build"]
execution_allow_errors = False
execution_excludepatterns: List[Any] = []
execution_in_temp = False
execution_timeout = 30
extensions = [
    "jupyter_book",
    # "myst_nb",
    "myst_parser",
    "numpydoc",
    "sphinx.ext.autodoc",
    "sphinx.ext.intersphinx",
    "sphinx.ext.napoleon",
    "sphinx_togglebutton",
    "sphinx_copybutton",
    "sphinx_thebe",
    "sphinx_comments",
    "sphinx_external_toc",
    "sphinx_panels",
    "sphinx_book_theme",
    "sphinx_jupyterbook_latex",
]
external_toc_exclude_missing = False
external_toc_path = "_toc.yml"
html_baseurl = ""
html_favicon = ""
html_logo = "artwork/shell.png"
html_sourcelink_suffix = ""
html_theme = "sphinx_book_theme"
html_theme_options = {
    "search_bar_text": "Search this book...",
    "launch_buttons": {
        "notebook_interface": "classic",
        "binderhub_url": "https://mybinder.org",
        "jupyterhub_url": "",
        "thebe": True,
        "colab_url": "",
    },
    "path_to_docs": "docs/",
    "repository_url": "https://github.com/nautechsystems/nautilus_trader",
    "repository_branch": "dev",
    "google_analytics_id": "",
    "extra_navbar": 'Powered by <a href="https://jupyterbook.org">Jupyter Book</a>',
    "extra_footer": "",
    "home_page_in_toc": True,
    "use_repository_button": True,
    "use_edit_page_button": False,
    "use_issues_button": True,
}
html_title = "NautilusTrader Docs"
jupyter_cache = ""
jupyter_execute_notebooks = "force"
language = None
latex_engine = "pdflatex"
myst_enable_extensions = [
    "colon_fence",
    "dollarmath",
    "linkify",
    "substitution",
    "tasklist",
]
myst_url_schemes = ["mailto", "http", "https"]
nb_output_stderr = "show"
numfig = True
panels_add_bootstrap_css = True
pygments_style = "sphinx"
suppress_warnings = ["myst.domains"]
use_jupyterbook_latex = True
use_multitoc_numbering = True

source_suffix = [".rst", ".md"]

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
