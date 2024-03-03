# Python API

```{eval-rst}
.. toctree::
   :maxdepth: 1
   :glob:
   
   accounting.md
   adapters/index.md
   analysis.md
   backtest.md
   cache.md
   common.md
   config.md
   core.md
   data.md
   execution.md
   indicators.md
   live.md
   model/index.md
   persistence.md
   portfolio.md
   risk.md
   serialization.md
   system.md
   trading.md
```

Welcome to the Python API reference for NautilusTrader!

The API reference provides detailed technical documentation for the NautilusTrader framework,
including its modules, classes, methods, and functions. The reference is automatically generated
from the latest NautilusTrader source code using [Sphinx](https://www.sphinx-doc.org/en/master/).

Please note that there are separate references for different versions of NautilusTrader:

- **Latest**: This API reference is built from the head of the `master` branch and represents the latest stable release.
- **Nightly**: This API reference is built from the head of the `nightly` branch and represents bleeding edge and experimental changes/features currently in development.

You can select the desired API reference from the **Versions** top right drop down menu.

```{note}
If you select an item from the top level navigation, this will take you to the **Latest** API reference.
```

Use the right navigation sidebar to explore the available modules and their contents.
You can click on any item to view its detailed documentation, including parameter descriptions, and return value explanations.

## Why Python?

Python was originally created decades ago as a simple scripting language with a clean straight
forward syntax. It has since evolved into a fully fledged general purpose object-oriented
programming language. Based on the TIOBE index, Python is currently the most popular programming language in the world.
Not only that, Python has become the _de facto lingua franca_ of data science, machine learning, and artificial intelligence.

The language out of the box is not without its drawbacks however, especially in the context of
implementing large performance-critical systems. Cython has addressed a lot of these issues, offering all the advantages
of a statically typed language, embedded into Pythons rich ecosystem of software libraries and
developer/user communities.

