# Packaging for PyPI

### CI Pipeline
The CI pipeline will now automatically package passing builds and upload
to PyPI via twine. The below manual packaging instructions are being kept
for historical reasons.

### Manually Packaging
Ensure version has been bumped and not pre-release.

Create the distribution package tar.gz

    python setup.py sdist


Ensure this is the only distribution in the /dist directory

Push package to PyPI using twine;

    twine upload --repository pypi dist/*
    
Username is \__token__

Use the pypi token
