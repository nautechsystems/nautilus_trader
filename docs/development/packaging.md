# Packaging for PyPI

Ensure version has been bumped and not pre-release.

Create the distribution package tar.gz

    python setup.py sdist


Ensure this is the only distribution in the /dist directory

Push package to PyPI using twine;

    twine upload --repository pypi dist/*
    
Username is \__token__
Use the pypi token
