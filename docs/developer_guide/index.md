# Developer Guide

Welcome to the developer guide for NautilusTrader!

Here you will find information related to developing and extending the NautilusTrader codebase. 
These guides will assist you in both adding functionality for your own trading operation, and/or 
acting as a guide to assist with valuable contributions.

We believe in using the right tool for the job. The overall design philosophy is to fully utilize 
the high level power of Python, with its rich eco-system of frameworks and libraries, whilst 
overcoming some of its inherent shortcomings in performance and lack of built-in type safety 
(with it being an interpreted dynamic language).

One of the advantages of Cython is that allocation and freeing of memory is handled by the C code 
generator during the ‘cythonization’ step of the build (unless you’re specifically utilizing some of 
its lower level features).

So we get the best of both worlds - with Pythons clean straight forward syntax, and a lot of 
potential to extract several orders of magnitude greater runtime performance through compiled C 
dynamic libraries.

The main development and runtime environment we are working in is of course Python. However with the 
introduction of Cython syntax throughout the production codebase in `.pyx` and `.pxd` files - it’s 
important to be aware of how the CPython implementation of Python interacts with the underlying 
CPython API, and the NautilusTrader C extension modules which Cython produces.

We recommend a thorough review of the [Cython docs](https://cython.readthedocs.io/en/latest/) to familiarize yourself with some of its core 
concepts, and where C typing is being introduced.

It's not necessary to become a C language expert, however it's helpful to understand how Cython C 
syntax is used in function and method definitions, in local code blocks, and the common primitive C 
types and how these map to their corresponding `PyObject` types.

```{eval-rst}
.. toctree::
   :maxdepth: 2
   :hidden:
   
   environment_setup.md
   coding_standards.md
   cython.md
   rust.md
   testing.md
   packaged_data.md
```
